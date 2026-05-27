//! Portal-56 (v6.0, R12-Q21) — per-workspace focused-border tinting.
//!
//! Subscribes to sway's `EventType::Workspace`. On every
//! `WorkspaceChange::Focus`, the worker looks up the newly-focused
//! workspace's owning tag (Portal-18.a schema) + reads
//! `tag.group_color`. It then fires swayipc
//!
//!   `client.focused <color> <color> #f4f4f4 <color> <color>`
//!
//! (border, background, text, indicator, child_border) to tint the
//! focused-window border to the tag's color. Tagless workspaces
//! fall back to the platform default Carbon blue (`#2b9af3`,
//! matching `data/sway/config:60`).
//!
//! Unfocused / urgent / placeholder colors are NOT touched — only
//! the focused state is recolored. Operator-configured per-workspace
//! UNFOCUSED colors stay as they are.

#![cfg(feature = "async-services")]

use std::time::Duration;

use futures_util::StreamExt as _;
use mackes_mesh_types::TagStore;
use swayipc_async::{Connection, EventType};

use super::workspace_router::find_owning_tag;
use super::{ShutdownToken, Worker};

const RECONNECT_BACKOFF: Duration = Duration::from_secs(3);

/// Platform fallback focused-border color when a workspace has no
/// owning tag or its tag has no `group_color`. Matches the Carbon
/// blue locked in `data/sway/config:60` `client.focused`.
pub const DEFAULT_FOCUSED_COLOR: &str = "#2b9af3";

/// Fixed text/foreground color in the swayipc `client.focused`
/// command — never tag-tinted. Off-white for readability against
/// any owning-tag color.
pub const FOCUSED_TEXT_COLOR: &str = "#f4f4f4";

/// Empty-state worker — tag store reloads per-event.
pub struct BorderTinterWorker;

impl BorderTinterWorker {
    /// Construct a fresh worker.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for BorderTinterWorker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Worker for BorderTinterWorker {
    fn name(&self) -> &'static str {
        "border_tinter"
    }

    async fn run(&mut self, mut shutdown: ShutdownToken) -> anyhow::Result<()> {
        loop {
            if shutdown.is_shutdown() {
                return Ok(());
            }
            let mut cmd_conn = match Connection::new().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!(error = %e, "border_tinter cmd-conn connect failed; backing off");
                    sleep_or_shutdown(RECONNECT_BACKOFF, &mut shutdown).await;
                    continue;
                }
            };
            let event_conn = match Connection::new().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!(error = %e, "border_tinter event-conn connect failed; backing off");
                    sleep_or_shutdown(RECONNECT_BACKOFF, &mut shutdown).await;
                    continue;
                }
            };
            let mut events = match event_conn.subscribe([EventType::Workspace]).await {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::debug!(error = %e, "border_tinter subscribe failed; backing off");
                    sleep_or_shutdown(RECONNECT_BACKOFF, &mut shutdown).await;
                    continue;
                }
            };
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.wait() => return Ok(()),
                    next = events.next() => {
                        match next {
                            Some(Ok(swayipc_async::Event::Workspace(ws_ev))) => {
                                if ws_ev.change == swayipc_async::WorkspaceChange::Focus {
                                    if let Some(node) = ws_ev.current.as_ref() {
                                        if let Some(num) = node.num {
                                            handle_focus(&mut cmd_conn, num).await;
                                        }
                                    }
                                }
                            }
                            Some(Ok(_)) => {}
                            Some(Err(e)) => {
                                tracing::debug!(error = %e, "border_tinter event stream errored; reconnecting");
                                break;
                            }
                            None => {
                                tracing::debug!("border_tinter event stream ended; reconnecting");
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn sleep_or_shutdown(dur: Duration, shutdown: &mut ShutdownToken) {
    tokio::select! {
        _ = shutdown.wait() => {}
        _ = tokio::time::sleep(dur) => {}
    }
}

async fn handle_focus(conn: &mut Connection, num: i32) {
    let store = match TagStore::load_default() {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(error = %e, "border_tinter tag-store load failed; skipping focus event");
            return;
        }
    };
    let color = color_for_workspace(&store, num);
    let cmd = client_focused_command(&color);
    match conn.run_command(&cmd).await {
        Ok(_) => tracing::debug!(workspace = num, %color, "border_tinter applied"),
        Err(e) => tracing::warn!(workspace = num, %color, error = %e, "border_tinter command failed"),
    }
}

// ── Pure helpers ────────────────────────────────────────────────────────

/// Resolve the focused-border color for workspace `ws_num`. Returns
/// the owning tag's `group_color` if present, else
/// [`DEFAULT_FOCUSED_COLOR`].
#[must_use]
pub fn color_for_workspace(store: &TagStore, ws_num: i32) -> String {
    find_owning_tag(store, ws_num)
        .and_then(|t| t.group_color.clone())
        .filter(|c| is_valid_hex_color(c))
        .unwrap_or_else(|| DEFAULT_FOCUSED_COLOR.to_string())
}

/// `true` if `s` looks like a CSS hex color: leading `#` + 3, 4, 6,
/// or 8 hex digits. The validation is intentionally strict so
/// malformed tag.json entries don't pass garbage to swayipc (which
/// would silently no-op and confuse operators).
#[must_use]
pub fn is_valid_hex_color(s: &str) -> bool {
    let rest = match s.strip_prefix('#') {
        Some(r) => r,
        None => return false,
    };
    if !matches!(rest.len(), 3 | 4 | 6 | 8) {
        return false;
    }
    rest.chars().all(|c| c.is_ascii_hexdigit())
}

/// Build the swayipc `client.focused` command string with the
/// given tint color. Per the R12-Q21 design lock:
///
///   `client.focused <color> <color> #f4f4f4 <color> <color>`
///
/// — border, background, text, indicator, child_border. Text
/// stays off-white for readability against any owning-tag color.
#[must_use]
pub fn client_focused_command(color: &str) -> String {
    format!(
        "client.focused {color} {color} {text} {color} {color}",
        color = color,
        text = FOCUSED_TEXT_COLOR
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mackes_mesh_types::{Tag, TagFlavor, TagMember, TagStore};

    fn dev_tag_with_color(ws_num: i32, color: Option<&str>) -> Tag {
        Tag {
            name: "Dev".to_string(),
            flavor: TagFlavor::Manual,
            members: vec![TagMember::Workspace { num: ws_num }],
            group_color: color.map(String::from),
            preferred_output: None,
            default_layout: None,
            autostart: Vec::new(),
        }
    }

    /// Empty store / untagged workspace → default Carbon blue.
    /// Mirrors the bench acceptance "focus an untagged workspace
    /// → border returns to Carbon blue".
    #[test]
    fn untagged_workspace_returns_default_carbon_blue() {
        let store = TagStore::default();
        assert_eq!(color_for_workspace(&store, 1), DEFAULT_FOCUSED_COLOR);
    }

    /// Owning tag with valid group_color → returns the tag's color.
    /// Mirrors the bench acceptance "focus a Dev-tag workspace →
    /// focused window border turns Dev color".
    #[test]
    fn tagged_workspace_returns_tag_color() {
        let mut store = TagStore::default();
        store.add(dev_tag_with_color(1, Some("#42be65"))).unwrap();
        assert_eq!(color_for_workspace(&store, 1), "#42be65");
    }

    /// Owning tag with no `group_color` set → falls back to default.
    #[test]
    fn owning_tag_without_color_falls_back_to_default() {
        let mut store = TagStore::default();
        store.add(dev_tag_with_color(1, None)).unwrap();
        assert_eq!(color_for_workspace(&store, 1), DEFAULT_FOCUSED_COLOR);
    }

    /// Owning tag with a MALFORMED `group_color` (not a hex
    /// string) → falls back to default rather than passing
    /// garbage to swayipc. Locks the strict-validation contract.
    #[test]
    fn malformed_color_falls_back_to_default() {
        let mut store = TagStore::default();
        store.add(dev_tag_with_color(1, Some("rebeccapurple"))).unwrap();
        assert_eq!(color_for_workspace(&store, 1), DEFAULT_FOCUSED_COLOR);

        let mut store2 = TagStore::default();
        store2.add(dev_tag_with_color(1, Some("#xyz"))).unwrap();
        assert_eq!(color_for_workspace(&store2, 1), DEFAULT_FOCUSED_COLOR);
    }

    /// Hex-color validator accepts every standard form (#rgb,
    /// #rgba, #rrggbb, #rrggbbaa) and rejects everything else.
    #[test]
    fn hex_color_validator_locks_recognised_forms() {
        // Accepted forms — 3, 4, 6, 8 hex digits after `#`.
        assert!(is_valid_hex_color("#f00"));
        assert!(is_valid_hex_color("#f00f"));
        assert!(is_valid_hex_color("#42be65"));
        assert!(is_valid_hex_color("#42be65ff"));
        assert!(is_valid_hex_color("#FFFFFF"));
        // Rejected forms.
        assert!(!is_valid_hex_color("42be65")); // missing leading #
        assert!(!is_valid_hex_color("#42be6")); // 5 chars — not a recognised length
        assert!(!is_valid_hex_color("#42")); // 2 chars
        assert!(!is_valid_hex_color("#1234567")); // 7 chars
        assert!(!is_valid_hex_color("#x42")); // non-hex char
        assert!(!is_valid_hex_color("")); // empty string
        assert!(!is_valid_hex_color("#")); // bare # with no digits
        assert!(!is_valid_hex_color("rebeccapurple")); // CSS named color
    }

    /// `client.focused` command shape matches the R12-Q21 design
    /// lock: 5 color slots, text fixed at `#f4f4f4`, the other
    /// 4 take the tint.
    #[test]
    fn client_focused_command_matches_r12_q21_shape() {
        let cmd = client_focused_command("#42be65");
        assert_eq!(
            cmd,
            "client.focused #42be65 #42be65 #f4f4f4 #42be65 #42be65"
        );
        // Default-color round-trip.
        let cmd_default = client_focused_command(DEFAULT_FOCUSED_COLOR);
        assert_eq!(
            cmd_default,
            "client.focused #2b9af3 #2b9af3 #f4f4f4 #2b9af3 #2b9af3"
        );
    }

    /// Tag owns multiple workspaces → all resolve to the same
    /// color (locking the "tag color spans every owned workspace"
    /// contract).
    #[test]
    fn tag_color_spans_every_owned_workspace() {
        let mut store = TagStore::default();
        let t = Tag {
            name: "Dev".to_string(),
            flavor: TagFlavor::Manual,
            members: vec![
                TagMember::Workspace { num: 1 },
                TagMember::Workspace { num: 2 },
                TagMember::Workspace { num: 3 },
            ],
            group_color: Some("#33b1ff".to_string()),
            preferred_output: None,
            default_layout: None,
            autostart: Vec::new(),
        };
        store.add(t).unwrap();
        for ws in 1..=3 {
            assert_eq!(color_for_workspace(&store, ws), "#33b1ff");
        }
        // Untagged ws 4 → default.
        assert_eq!(color_for_workspace(&store, 4), DEFAULT_FOCUSED_COLOR);
    }
}
