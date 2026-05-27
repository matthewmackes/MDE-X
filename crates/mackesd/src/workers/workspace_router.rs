//! Portal-42 (v6.0, R12-Q2) — tag-driven workspace output assignment.
//!
//! Subscribes to sway's `EventType::Workspace`. On every
//! `WorkspaceChange::Init` event the worker looks up the owning
//! tag for the new workspace (a tag whose members include a
//! `TagMember::Workspace { num }` entry) + if that tag has a
//! `preferred_output` field set, fires swayipc
//! `move workspace <num> to output <name>` to relocate.
//!
//! Unset `preferred_output` (or no owning tag) is a no-op — sway's
//! natural placement wins.
//!
//! The tag store reloads from `<XDG_DATA_HOME>/mde/tags.json` on
//! every event so edits via the Portal-18.b modal take effect
//! immediately without a daemon restart. Reads are cheap (file is
//! small + JSON parse is fast) and only triggered by sway events,
//! so the polling overhead is bounded by user-initiated workspace
//! creations.

#![cfg(feature = "async-services")]

use std::time::Duration;

use futures_util::StreamExt as _;
use mackes_mesh_types::{Tag, TagMember, TagStore};
use swayipc_async::{Connection, EventType};

use super::{ShutdownToken, Worker};

const RECONNECT_BACKOFF: Duration = Duration::from_secs(3);

/// Empty-state worker — all state lives on the stack inside `run`.
pub struct WorkspaceRouterWorker;

impl WorkspaceRouterWorker {
    /// Construct a fresh worker. No configuration — the tag store
    /// path is resolved per-event from `TagStore::load_default`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for WorkspaceRouterWorker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Worker for WorkspaceRouterWorker {
    fn name(&self) -> &'static str {
        "workspace_router"
    }

    async fn run(&mut self, mut shutdown: ShutdownToken) -> anyhow::Result<()> {
        loop {
            if shutdown.is_shutdown() {
                return Ok(());
            }
            let mut cmd_conn = match Connection::new().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!(error = %e, "workspace_router cmd-conn connect failed; backing off");
                    sleep_or_shutdown(RECONNECT_BACKOFF, &mut shutdown).await;
                    continue;
                }
            };
            let event_conn = match Connection::new().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!(error = %e, "workspace_router event-conn connect failed; backing off");
                    sleep_or_shutdown(RECONNECT_BACKOFF, &mut shutdown).await;
                    continue;
                }
            };
            let mut events = match event_conn.subscribe([EventType::Workspace]).await {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::debug!(error = %e, "workspace_router subscribe failed; backing off");
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
                                if ws_ev.change == swayipc_async::WorkspaceChange::Init {
                                    if let Some(node) = ws_ev.current.as_ref() {
                                        handle_init(&mut cmd_conn, node).await;
                                    }
                                }
                            }
                            Some(Ok(_)) => {}
                            Some(Err(e)) => {
                                tracing::debug!(error = %e, "workspace_router event stream errored; reconnecting");
                                break;
                            }
                            None => {
                                tracing::debug!("workspace_router event stream ended; reconnecting");
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

/// Handle a `WorkspaceChange::Init` event. Loads the tag store
/// fresh on each event so operator edits take effect immediately
/// without a daemon restart.
async fn handle_init(conn: &mut Connection, node: &swayipc_async::Node) {
    let Some(num) = node.num else {
        // Workspace nodes without a num are sway-internal (e.g.
        // scratchpad meta-workspaces).
        return;
    };
    let store = match TagStore::load_default() {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(error = %e, "workspace_router tag-store load failed; skipping event");
            return;
        }
    };
    let Some(output_name) = preferred_output_for_workspace(&store, num) else {
        return;
    };
    let cmd = move_workspace_command(num, &output_name);
    match conn.run_command(&cmd).await {
        Ok(_) => tracing::debug!(workspace = num, %output_name, "workspace_router moved workspace"),
        Err(e) => tracing::warn!(workspace = num, %output_name, error = %e, "workspace_router move failed"),
    }
}

// ── Pure helpers (testable without a sway connection) ───────────────────

/// Find the tag that owns workspace number `ws_num` (one whose
/// `members` includes `TagMember::Workspace { num: ws_num }`).
/// Returns the first match — operators are expected to put each
/// workspace in at most one tag, but if multiples exist the first
/// in JSON order wins.
#[must_use]
pub fn find_owning_tag(store: &TagStore, ws_num: i32) -> Option<&Tag> {
    store.tags.iter().find(|t| {
        t.members
            .iter()
            .any(|m| matches!(m, TagMember::Workspace { num } if *num == ws_num))
    })
}

/// Resolve the `preferred_output` for workspace number `ws_num`.
/// Returns `None` when there's no owning tag, or when the owning
/// tag has no `preferred_output` set.
#[must_use]
pub fn preferred_output_for_workspace(store: &TagStore, ws_num: i32) -> Option<String> {
    find_owning_tag(store, ws_num)?.preferred_output.clone()
}

/// Build the swayipc command string that moves workspace `ws_num`
/// to output `output_name`. Embedded double-quotes in the output
/// name are backslash-escaped (sway output names like `HDMI-A-1`
/// don't contain quotes in practice, but the escape locks the
/// contract anyway).
#[must_use]
pub fn move_workspace_command(ws_num: i32, output_name: &str) -> String {
    let escaped = output_name.replace('\\', "\\\\").replace('"', "\\\"");
    format!("workspace number {ws_num}; move workspace to output \"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mackes_mesh_types::{Tag, TagFlavor, TagMember, TagStore};

    fn dev_tag_on_hdmi(ws_nums: &[i32]) -> Tag {
        let members = ws_nums
            .iter()
            .map(|&num| TagMember::Workspace { num })
            .collect();
        Tag {
            name: "Dev".to_string(),
            flavor: TagFlavor::Manual,
            members,
            group_color: None,
            preferred_output: Some("HDMI-A-1".to_string()),
            default_layout: None,
            autostart: Vec::new(),
        }
    }

    /// Empty store → no owning tag → no command. Locks the
    /// "sway natural placement wins" contract for the no-tags path.
    #[test]
    fn empty_store_returns_no_preferred_output() {
        let store = TagStore::default();
        assert!(preferred_output_for_workspace(&store, 1).is_none());
    }

    /// Tag exists but doesn't own ws 1 → no command for ws 1.
    #[test]
    fn untagged_workspace_returns_no_preferred_output() {
        let mut store = TagStore::default();
        store.add(dev_tag_on_hdmi(&[2, 3])).unwrap();
        assert!(preferred_output_for_workspace(&store, 1).is_none());
    }

    /// Tag owns ws 1 with no preferred_output → still no command.
    #[test]
    fn owning_tag_without_preferred_output_returns_none() {
        let mut store = TagStore::default();
        let mut t = dev_tag_on_hdmi(&[1]);
        t.preferred_output = None;
        store.add(t).unwrap();
        assert!(preferred_output_for_workspace(&store, 1).is_none());
    }

    /// Owning tag with preferred_output → returns the output name.
    /// Mirrors the bench acceptance "creating a workspace under
    /// tag `Dev` with `preferred_output: HDMI-A-1` lands on
    /// HDMI-A-1".
    #[test]
    fn owning_tag_with_preferred_output_returns_target() {
        let mut store = TagStore::default();
        store.add(dev_tag_on_hdmi(&[1, 2])).unwrap();
        assert_eq!(
            preferred_output_for_workspace(&store, 1).as_deref(),
            Some("HDMI-A-1")
        );
        assert_eq!(
            preferred_output_for_workspace(&store, 2).as_deref(),
            Some("HDMI-A-1")
        );
    }

    /// Multiple tags own the same workspace → first in JSON order
    /// wins. Locks the deterministic-tiebreaker contract.
    #[test]
    fn first_owning_tag_wins_on_conflict() {
        let mut store = TagStore::default();
        store.add(dev_tag_on_hdmi(&[1])).unwrap();
        let mut second = dev_tag_on_hdmi(&[1]);
        second.name = "Personal".to_string();
        second.preferred_output = Some("DP-2".to_string());
        store.add(second).unwrap();
        assert_eq!(
            preferred_output_for_workspace(&store, 1).as_deref(),
            Some("HDMI-A-1")
        );
    }

    /// `move_workspace_command` builds a two-command swayipc string
    /// that first focuses the target workspace (so sway has a
    /// concrete workspace to move) + then moves it. Quote-escaping
    /// rounds-trips quirky output names.
    #[test]
    fn move_workspace_command_escapes_quotes_and_backslashes() {
        assert_eq!(
            move_workspace_command(1, "HDMI-A-1"),
            r#"workspace number 1; move workspace to output "HDMI-A-1""#
        );
        assert_eq!(
            move_workspace_command(7, r#"weird"name"#),
            r#"workspace number 7; move workspace to output "weird\"name""#
        );
        assert_eq!(
            move_workspace_command(3, r"back\slash"),
            r#"workspace number 3; move workspace to output "back\\slash""#
        );
    }

    /// Non-workspace members (App / Peer / etc.) of an otherwise-
    /// matching tag must not cause the workspace to be claimed.
    #[test]
    fn non_workspace_members_dont_match() {
        let mut store = TagStore::default();
        store
            .add(Tag {
                name: "Apps".to_string(),
                flavor: TagFlavor::Manual,
                members: vec![
                    TagMember::App {
                        app_id: "firefox".to_string(),
                    },
                    TagMember::Peer {
                        hostname: "fedora".to_string(),
                    },
                ],
                group_color: None,
                preferred_output: Some("HDMI-A-1".to_string()),
                default_layout: None,
                autostart: Vec::new(),
            })
            .unwrap();
        assert!(preferred_output_for_workspace(&store, 1).is_none());
    }
}
