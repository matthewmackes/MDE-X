//! `action/shell/<verb>` Bus responder (DBUS-2 — retires the
//! `dev.mackes.MDE.Portal` D-Bus interface per the Q96 Bus-canonical
//! lock).
//!
//! mde-open (the `mde://` URI handler) and mackesd (daemon-driven
//! events: CRITICAL-alert navigation, idle-lock) publish shell commands
//! to `action/shell/{goto,focus,lock,open-uri,toggle-dnd,restart}`.
//! This loop runs on a background thread off the Iced render thread —
//! the same decoupling the old zbus service had — polls those topics,
//! dispatches to the exact side effects the D-Bus methods ran, and
//! replies on `reply/<ulid>` for callers that use a request/reply round
//! trip (fire-and-forget publishers simply ignore the reply).
//!
//! Mirrors the `mde-musicd` responder shape: a synchronous poll loop
//! keeps the non-`Send` `Persist` on its own thread and drives the
//! async side effects (process spawns, the Portal-full forward) through
//! a current-thread runtime's `block_on`.

use std::collections::HashMap;

use mde_bus::hooks::config::Priority;
use mde_bus::persist::Persist;
use mde_bus::rpc::INTERACTIVE_POLL_INTERVAL;

use crate::dbus::{portal_full_goto, PortalState};
use crate::uri::{action_to_uri, parse_mde_uri, Action};

/// The shell verbs served on `action/shell/<verb>`.
pub const SHELL_VERBS: [&str; 6] = ["goto", "focus", "lock", "open-uri", "toggle-dnd", "restart"];

/// Reply topic an `rpc::request` caller listens on for `<ulid>`.
fn reply_topic(ulid: &str) -> String {
    format!("reply/{ulid}")
}

/// Run the side effect for a parsed `mde://` action and return its
/// canonical URI form (so `open-uri` callers can log what dispatched).
///
/// This is the body the old `Portal.OpenUri` D-Bus method ran.
pub async fn dispatch_action(action: Action, state: &PortalState) -> String {
    match action {
        Action::Goto { ref layer, .. } => {
            portal_full_goto(layer).await;
        }
        Action::Lock => {
            let _ = tokio::process::Command::new("mde-popover").arg("lock").spawn();
        }
        Action::Focus => {
            // Raising Portal-full lives in the Dock's scratchpad-show wiring.
        }
        Action::ToggleDnd => {
            state.toggle_dnd_inner().await;
        }
        Action::Restart => {
            let _ = tokio::process::Command::new("systemctl")
                .args(["--user", "restart", "mde-portal"])
                .spawn();
        }
        Action::OpenApp(ref id) => {
            let _ = tokio::process::Command::new("gtk-launch").arg(id).spawn();
        }
        Action::OpenFile(ref path) => {
            let _ = tokio::process::Command::new("xdg-open").arg(path).spawn();
        }
        Action::Peer { .. } => {
            tracing::warn!("action/shell: cross-peer routing not yet wired (needs mesh RPC)");
        }
        Action::Unknown(ref raw) => {
            tracing::warn!(uri = %raw, "action/shell/open-uri: unknown verb");
        }
    }
    action_to_uri(&action)
}

/// Dispatch one `action/shell/<verb>` request. `body` carries the verb's
/// argument: the layer name for `goto`, the `mde://` URI for `open-uri`,
/// ignored for the rest. Returns the reply JSON written to `reply/<ulid>`.
pub async fn dispatch_shell(verb: &str, body: &str, state: &PortalState) -> String {
    match verb {
        "goto" => {
            tracing::info!(layer = body, "action/shell/goto");
            portal_full_goto(body).await;
            r#"{"ok":true}"#.to_string()
        }
        "focus" => {
            tracing::info!("action/shell/focus");
            r#"{"ok":true}"#.to_string()
        }
        "lock" => {
            tracing::info!("action/shell/lock: spawning mde-popover lock");
            match tokio::process::Command::new("mde-popover").arg("lock").spawn() {
                Ok(_) => r#"{"ok":true}"#.to_string(),
                Err(e) => format!(r#"{{"ok":false,"error":"spawn mde-popover lock: {e}"}}"#),
            }
        }
        "toggle-dnd" => {
            let dnd = state.toggle_dnd_inner().await;
            tracing::info!(dnd, "action/shell/toggle-dnd");
            format!(r#"{{"ok":true,"dnd":{dnd}}}"#)
        }
        "restart" => {
            tracing::info!("action/shell/restart: systemctl --user restart mde-portal");
            match tokio::process::Command::new("systemctl")
                .args(["--user", "restart", "mde-portal"])
                .spawn()
            {
                Ok(_) => r#"{"ok":true}"#.to_string(),
                Err(e) => format!(r#"{{"ok":false,"error":"systemctl restart: {e}"}}"#),
            }
        }
        "open-uri" => {
            let action = parse_mde_uri(body);
            tracing::info!(uri = body, action = ?action, "action/shell/open-uri");
            let canonical = dispatch_action(action, state).await;
            format!(r#"{{"ok":true,"uri":"{canonical}"}}"#)
        }
        other => format!(r#"{{"ok":false,"error":"unknown shell verb: {other}"}}"#),
    }
}

/// Run the `action/shell/*` responder loop on the calling thread until
/// `should_stop` returns true. Opens its own `Persist` (kept on this
/// thread — rusqlite isn't `Send`) and a current-thread runtime for the
/// async side effects.
///
/// # Errors
/// If the Bus data dir is unavailable or the store / runtime can't open.
pub fn serve<F: Fn() -> bool>(state: PortalState, should_stop: F) -> anyhow::Result<()> {
    let dir = mde_bus::default_data_dir()
        .ok_or_else(|| anyhow::anyhow!("no Bus data dir for action/shell responder"))?;
    let persist = Persist::open(dir)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut cursors: HashMap<String, String> = HashMap::new();
    tracing::info!("action/shell responder serving (DBUS-2 Bus-canonical)");
    while !should_stop() {
        for verb in SHELL_VERBS {
            let topic = format!("action/shell/{verb}");
            let since = cursors.get(&topic).map(String::as_str);
            let msgs = match persist.list_since(&topic, since) {
                Ok(m) => m,
                Err(_) => continue,
            };
            for msg in msgs {
                cursors.insert(topic.clone(), msg.ulid.clone());
                let reply = rt.block_on(dispatch_shell(verb, msg.body.as_deref().unwrap_or(""), &state));
                let _ = persist.write(&reply_topic(&msg.ulid), Priority::Default, None, Some(&reply));
            }
        }
        // goto/focus are latency-sensitive (DBUS-2 finding #1): poll at the
        // 40 ms interactive cadence so a keybind→goto reads as instant.
        std::thread::sleep(INTERACTIVE_POLL_INTERVAL);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn toggle_dnd_verb_flips_and_reports_state() {
        let state = PortalState::new();
        let r1 = dispatch_shell("toggle-dnd", "", &state).await;
        assert_eq!(r1, r#"{"ok":true,"dnd":true}"#);
        let r2 = dispatch_shell("toggle-dnd", "", &state).await;
        assert_eq!(r2, r#"{"ok":true,"dnd":false}"#);
    }

    #[tokio::test]
    async fn goto_verb_returns_ok_for_any_layer() {
        let state = PortalState::new();
        // Portal-full service is absent in tests; the forward is silent.
        assert_eq!(dispatch_shell("goto", "nonexistent", &state).await, r#"{"ok":true}"#);
    }

    #[tokio::test]
    async fn focus_verb_returns_ok() {
        let state = PortalState::new();
        assert_eq!(dispatch_shell("focus", "", &state).await, r#"{"ok":true}"#);
    }

    #[tokio::test]
    async fn open_uri_verb_returns_canonical_form() {
        let state = PortalState::new();
        let r = dispatch_shell("open-uri", "mde://hub", &state).await;
        assert_eq!(r, r#"{"ok":true,"uri":"mde://hub"}"#);
    }

    #[tokio::test]
    async fn open_uri_verb_toggles_dnd() {
        let state = PortalState::new();
        assert!(!state.dnd_enabled().await);
        let _ = dispatch_shell("open-uri", "mde://dnd-toggle", &state).await;
        assert!(state.dnd_enabled().await);
    }

    #[tokio::test]
    async fn unknown_verb_reports_error() {
        let state = PortalState::new();
        let r = dispatch_shell("flubber", "", &state).await;
        assert!(r.contains(r#""ok":false"#));
    }

    #[test]
    fn shell_verbs_cover_the_old_dbus_methods() {
        for v in ["goto", "focus", "lock", "open-uri", "toggle-dnd", "restart"] {
            assert!(SHELL_VERBS.contains(&v));
        }
    }
}
