//! Session lifecycle control surface (DBUS-1 — migrated to Bus).
//!
//! Per the Q96 Bus-canonical lock (EPIC-RETIRE-DBUS), the session
//! lifecycle verbs (logout / restart / shutdown / lock / save-layout)
//! are served on the Bus at `action/session/<verb>` instead of the
//! retired `dev.mackes.MDE.Session` D-Bus interface. The
//! `mde-logout-dialog` + panel publish a request on the action topic;
//! this responder applies the effect and replies on `reply/<ulid>`.
//!
//! The verb → action mapping ([`action_for_verb`]) is a pure function
//! (unit-tested); [`apply`] performs the side effects (kill / systemctl
//! / lock / wm get_tree), which are exercised at the §0.15 HW bench.

use std::collections::HashMap;
use std::sync::Arc;

use mde_bus::hooks::config::Priority;
use mde_bus::persist::Persist;
use mde_bus::rpc::reply_topic;
use serde_json::json;
use tokio::sync::Mutex;

/// Poll cadence for the action topics.
pub const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(400);

/// The lifecycle verbs served on `action/session/<verb>`.
pub const ACTION_VERBS: [&str; 5] = ["logout", "restart", "shutdown", "lock", "save-layout"];

/// Per-session state owned by the responder. Tracks whether the layout
/// was saved this session.
#[derive(Clone, Debug, Default)]
pub struct SessionState {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    layout_saved: bool,
}

impl SessionState {
    /// Construct a fresh session-state with `layout_saved=false`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// A session lifecycle action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    Logout,
    Restart,
    Shutdown,
    Lock,
    SaveLayout,
}

/// Map an `action/session/<verb>` verb to its action. Pure + testable.
#[must_use]
pub fn action_for_verb(verb: &str) -> Option<SessionAction> {
    match verb {
        "logout" => Some(SessionAction::Logout),
        "restart" => Some(SessionAction::Restart),
        "shutdown" => Some(SessionAction::Shutdown),
        "lock" => Some(SessionAction::Lock),
        "save-layout" => Some(SessionAction::SaveLayout),
        _ => None,
    }
}

/// Apply a lifecycle action (the side-effecting half).
///
/// # Errors
/// Returns a message when the effect's shell-out / IO fails.
pub async fn apply(action: SessionAction, state: &SessionState) -> Result<(), String> {
    match action {
        SessionAction::Logout => {
            tracing::info!("session: logout");
            // SIGTERM our own PID; systemd's graphical-session.target
            // tear-down handles the rest. (No `unsafe`: shell out to kill.)
            let pid = std::process::id();
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
            Ok(())
        }
        SessionAction::Restart => {
            tracing::info!("session: restart");
            run("systemctl", &["reboot"]).await
        }
        SessionAction::Shutdown => {
            tracing::info!("session: shutdown");
            run("systemctl", &["poweroff"]).await
        }
        SessionAction::Lock => {
            tracing::info!("session: lock");
            crate::lock::run_lock_command()
                .await
                .map_err(|e| format!("lock command failed: {e}"))
        }
        SessionAction::SaveLayout => {
            tracing::info!("session: save-layout");
            let layout = run_wm_get_tree()
                .await
                .map_err(|e| format!("wm get_tree failed: {e}"))?;
            let path = layout_save_path();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {} failed: {e}", parent.display()))?;
            }
            std::fs::write(&path, layout)
                .map_err(|e| format!("write {} failed: {e}", path.display()))?;
            state.inner.lock().await.layout_saved = true;
            Ok(())
        }
    }
}

async fn run(bin: &str, args: &[&str]) -> Result<(), String> {
    let status = tokio::process::Command::new(bin)
        .args(args)
        .status()
        .await
        .map_err(|e| format!("spawn {bin}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{bin} exited {}", status.code().unwrap_or(-1)))
    }
}

/// Run the Bus responder loop on the current thread, building a local
/// tokio runtime for the async effects (`Persist`/rusqlite isn't `Send`,
/// so this runs off the main async executor — see `mde-session` main).
/// Loops until `should_stop()` returns true.
pub fn serve_bus<F: Fn() -> bool>(persist: &Persist, state: &SessionState, should_stop: F) {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("session responder: runtime build failed: {e}");
            return;
        }
    };
    let mut cursors: HashMap<String, String> = HashMap::new();
    while !should_stop() {
        poll_once(persist, state, &rt, &mut cursors);
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// One poll sweep across the action verbs (split out so a test can drive
/// it without the sleep loop).
pub fn poll_once(
    persist: &Persist,
    state: &SessionState,
    rt: &tokio::runtime::Runtime,
    cursors: &mut HashMap<String, String>,
) {
    for verb in ACTION_VERBS {
        let topic = format!("action/session/{verb}");
        let since = cursors.get(&topic).map(String::as_str);
        let msgs = match persist.list_since(&topic, since) {
            Ok(m) => m,
            Err(_) => continue,
        };
        for msg in msgs {
            cursors.insert(topic.clone(), msg.ulid.clone());
            let reply = match action_for_verb(verb) {
                Some(action) => match rt.block_on(apply(action, state)) {
                    Ok(()) => json!({ "ok": true }).to_string(),
                    Err(e) => json!({ "ok": false, "error": e }).to_string(),
                },
                None => json!({ "ok": false, "error": "unknown verb" }).to_string(),
            };
            let _ = persist.write(&reply_topic(&msg.ulid), Priority::Default, None, Some(&reply));
        }
    }
}

/// Path of the saved-layout sidecar.
fn layout_save_path() -> std::path::PathBuf {
    let cache = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map_or_else(
            || dirs::home_dir().unwrap_or_default().join(".cache"),
            std::path::PathBuf::from,
        );
    cache.join("mde").join("session-layout.json")
}

/// Fetch the current WM tree as JSON.
/// - `wayland` feature: `swaymsg -t get_tree`
/// - `x11` feature: `i3-msg -t get_tree`
async fn run_wm_get_tree() -> anyhow::Result<String> {
    #[cfg(not(feature = "x11"))]
    let wm_msg = "swaymsg";
    #[cfg(feature = "x11")]
    let wm_msg = "i3-msg";

    let out = tokio::process::Command::new(wm_msg)
        .args(["-t", "get_tree"])
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!("{wm_msg} get_tree exited non-zero");
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verb_mapping_covers_all_and_rejects_unknown() {
        assert_eq!(action_for_verb("logout"), Some(SessionAction::Logout));
        assert_eq!(action_for_verb("restart"), Some(SessionAction::Restart));
        assert_eq!(action_for_verb("shutdown"), Some(SessionAction::Shutdown));
        assert_eq!(action_for_verb("lock"), Some(SessionAction::Lock));
        assert_eq!(action_for_verb("save-layout"), Some(SessionAction::SaveLayout));
        assert_eq!(action_for_verb("frobnicate"), None);
    }

    #[tokio::test]
    async fn session_state_starts_with_layout_not_saved() {
        let s = SessionState::new();
        assert!(!s.inner.lock().await.layout_saved);
    }

    #[test]
    fn layout_save_path_honors_xdg_cache_home() {
        let prev = std::env::var_os("XDG_CACHE_HOME");
        std::env::set_var("XDG_CACHE_HOME", "/tmp/test-cache-mde-session");
        assert_eq!(
            layout_save_path(),
            std::path::PathBuf::from("/tmp/test-cache-mde-session/mde/session-layout.json")
        );
        match prev {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
    }
}
