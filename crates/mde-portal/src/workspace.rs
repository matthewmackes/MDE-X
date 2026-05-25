//! Portal-5 — swayipc-async workspace integration.
//!
//! Provides `WorkspaceInfo` (the trimmed-down data the Dock needs) and
//! `workspace_subscription()` (an Iced `Subscription` that emits
//! `Message::WorkspaceList` on startup and on every workspace change).
//!
//! Two swayipc connections are opened per watcher run:
//!   1. A command connection — used for `get_workspaces()` refreshes.
//!   2. An event connection — consumed by `subscribe()`, streams workspace events.
//!
//! If swayipc is unavailable ($SWAYSOCK unset, Sway not running), the
//! subscription retries every 3 s without panicking.  The Dock renders
//! an empty workspace segment until a connection succeeds.

use futures_util::StreamExt as _;
use iced::Subscription;
use swayipc_async::{Connection, EventType};

use crate::app::Message;

/// Adaptive-width floor for workspace cells (R4-Q64).
pub const WORKSPACE_CELL_MIN_PX: f32 = 24.0;

/// Maximum displayed characters before a workspace name is truncated.
const WS_NAME_MAX_CHARS: usize = 8;

/// Trimmed workspace data the Dock strip needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInfo {
    /// Workspace number (−1 for the scratch-pad / unnamed).
    pub num: i32,
    /// Human-readable name (may equal `num.to_string()` for numbered ws).
    pub name: String,
    /// This workspace has keyboard focus.
    pub focused: bool,
    /// This workspace is visible on some output.
    pub visible: bool,
    /// Output the workspace is assigned to (e.g. `"HDMI-A-1"`).
    pub output: String,
    /// At least one window has the urgent hint.
    pub urgent: bool,
}

impl From<swayipc_async::Workspace> for WorkspaceInfo {
    fn from(ws: swayipc_async::Workspace) -> Self {
        WorkspaceInfo {
            num: ws.num,
            name: ws.name,
            focused: ws.focused,
            visible: ws.visible,
            output: ws.output,
            urgent: ws.urgent,
        }
    }
}

impl WorkspaceInfo {
    /// Display label: raw number if `name == num.to_string()`, else
    /// truncate to 8 chars + `…`.
    pub fn display_label(&self) -> String {
        if self.name == self.num.to_string() {
            self.name.clone()
        } else if self.name.chars().count() > WS_NAME_MAX_CHARS {
            let prefix: String = self.name.chars().take(WS_NAME_MAX_CHARS).collect();
            format!("{prefix}…")
        } else {
            self.name.clone()
        }
    }
}

/// Iced `Subscription` that emits `Message::WorkspaceList` on startup and
/// on every workspace-change event from i3/sway.
///
/// Uses an async-stream generator so the event loop is ergonomic and the
/// stream is lazy (starts only when iced's runtime runs it).
pub fn workspace_subscription() -> Subscription<Message> {
    Subscription::run_with_id(
        "mde-portal-workspaces",
        async_stream::stream! {
            loop {
                // Open command connection (for get_workspaces refreshes).
                let cmd_conn = match Connection::new().await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::debug!(error = %e, "swayipc cmd connect failed; retrying in 3s");
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        continue;
                    }
                };
                let mut conn = cmd_conn;

                // Emit initial workspace list.
                if let Ok(wss) = conn.get_workspaces().await {
                    let infos: Vec<WorkspaceInfo> =
                        wss.into_iter().map(WorkspaceInfo::from).collect();
                    yield Message::WorkspaceList(infos);
                }

                // Open event connection (separate; subscribe() consumes self).
                let event_conn = match Connection::new().await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::debug!(error = %e, "swayipc event connect failed; retrying in 3s");
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        continue;
                    }
                };

                let mut events = match event_conn.subscribe([EventType::Workspace]).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::debug!(error = %e, "workspace subscribe failed; retrying in 3s");
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        continue;
                    }
                };

                // Forward workspace-change events as WorkspaceList updates.
                while let Some(event_result) = events.next().await {
                    if let Ok(swayipc_async::Event::Workspace(_)) = event_result {
                        if let Ok(wss) = conn.get_workspaces().await {
                            let infos: Vec<WorkspaceInfo> =
                                wss.into_iter().map(WorkspaceInfo::from).collect();
                            yield Message::WorkspaceList(infos);
                        }
                    }
                }

                // Event stream ended (sway disconnected) — loop retries immediately.
                tracing::debug!("swayipc event stream ended; reconnecting");
            }
        },
    )
}

/// Focus a workspace by name via a fresh swayipc connection.
///
/// The subscription's event loop will deliver an updated `WorkspaceList`
/// automatically once sway emits the workspace-change event.
pub async fn focus_workspace(name: String) {
    match Connection::new().await {
        Ok(mut conn) => {
            if let Err(e) = conn.run_command(&format!("workspace \"{}\"", name)).await {
                tracing::warn!(workspace = %name, error = %e, "focus_workspace command failed");
            }
        }
        Err(e) => tracing::warn!(error = %e, "focus_workspace: swayipc connect failed"),
    }
}

/// Switch to the lowest unused workspace number ≥ 1.
pub async fn new_workspace(taken_nums: Vec<i32>) {
    let next = (1i32..).find(|n| !taken_nums.contains(n)).unwrap_or(1);
    match Connection::new().await {
        Ok(mut conn) => {
            if let Err(e) = conn.run_command(&format!("workspace {next}")).await {
                tracing::warn!(error = %e, "new_workspace command failed");
            }
        }
        Err(e) => tracing::warn!(error = %e, "new_workspace: swayipc connect failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ws(num: i32, name: &str, focused: bool, visible: bool, output: &str, urgent: bool) -> WorkspaceInfo {
        WorkspaceInfo {
            num,
            name: name.to_string(),
            focused,
            visible,
            output: output.to_string(),
            urgent,
        }
    }

    #[test]
    fn workspace_cell_min_px_is_24() {
        assert!((WORKSPACE_CELL_MIN_PX - 24.0).abs() < f32::EPSILON);
    }

    #[test]
    fn display_label_numeric_workspace() {
        let ws = make_ws(1, "1", false, false, "HDMI-A-1", false);
        assert_eq!(ws.display_label(), "1");
    }

    #[test]
    fn display_label_short_named_workspace() {
        let ws = make_ws(2, "dev", false, false, "HDMI-A-1", false);
        assert_eq!(ws.display_label(), "dev");
    }

    #[test]
    fn display_label_long_named_workspace_truncates() {
        let ws = make_ws(3, "my-very-long-project-name", false, false, "HDMI-A-1", false);
        let label = ws.display_label();
        assert!(label.ends_with('…'), "long name should end with ellipsis: {label}");
        assert!(label.chars().count() <= WS_NAME_MAX_CHARS + 1, "truncated label too long: {label}");
    }

    #[test]
    fn display_label_exactly_max_chars_no_truncation() {
        let name = "abcdefgh"; // exactly WS_NAME_MAX_CHARS = 8
        let ws = make_ws(4, name, false, false, "HDMI-A-1", false);
        assert_eq!(ws.display_label(), name);
    }

    #[test]
    fn workspace_info_fields_preserved() {
        let ws = make_ws(5, "test", true, true, "eDP-1", true);
        assert_eq!(ws.num, 5);
        assert!(ws.focused);
        assert!(ws.visible);
        assert!(ws.urgent);
        assert_eq!(ws.output, "eDP-1");
    }

    #[test]
    fn new_workspace_finds_lowest_gap() {
        let taken = vec![1, 2];
        let next = (1i32..).find(|n| !taken.contains(n)).unwrap_or(1);
        assert_eq!(next, 3);
    }

    #[test]
    fn new_workspace_finds_gap_in_middle() {
        let taken = vec![1, 3, 5];
        let next = (1i32..).find(|n| !taken.contains(n)).unwrap_or(1);
        assert_eq!(next, 2);
    }

    #[test]
    fn new_workspace_empty_taken_starts_at_1() {
        let taken: Vec<i32> = vec![];
        let next = (1i32..).find(|n| !taken.contains(n)).unwrap_or(1);
        assert_eq!(next, 1);
    }
}
