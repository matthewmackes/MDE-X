//! Portal-44 (v6.0, R12-Q4) — per-tag default_layout enforcement.
//!
//! Subscribes to sway's `EventType::Window`. On every
//! `WindowChange::New` event, the worker checks:
//!
//!   1. Is the new window's workspace owned by a tag?
//!   2. Does the owning tag have a `default_layout` set?
//!   3. Is this the only window currently on that workspace?
//!
//! If all three are true AND the workspace's current container
//! layout differs from the tag default, fires swayipc
//! `[con_id=<n>] focus; layout <name>` to flip the layout. The
//! second condition (single window) is the design lock —
//! flipping layout on every subsequent window would override
//! operator choice mid-session. Only the FIRST window in a tag-
//! owned workspace triggers the rule.
//!
//! Hub right-click 'Layout Chooser' UI (R3-Q62) is deferred to
//! Portal-18.b — operators set `default_layout` by hand-editing
//! `~/.local/share/mde/tags.json` until the modal lands.

#![cfg(feature = "async-services")]

use std::time::Duration;

use futures_util::StreamExt as _;
use mackes_mesh_types::TagStore;
use swayipc_async::{Connection, EventType};

use super::{ShutdownToken, Worker};
use super::workspace_router::find_owning_tag;

const RECONNECT_BACKOFF: Duration = Duration::from_secs(3);

/// Empty-state worker — tag store reloads per-event.
pub struct TagLayoutWorker;

impl TagLayoutWorker {
    /// Construct a fresh worker.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for TagLayoutWorker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Worker for TagLayoutWorker {
    fn name(&self) -> &'static str {
        "tag_layout"
    }

    async fn run(&mut self, mut shutdown: ShutdownToken) -> anyhow::Result<()> {
        loop {
            if shutdown.is_shutdown() {
                return Ok(());
            }
            let mut cmd_conn = match Connection::new().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!(error = %e, "tag_layout cmd-conn connect failed; backing off");
                    sleep_or_shutdown(RECONNECT_BACKOFF, &mut shutdown).await;
                    continue;
                }
            };
            let event_conn = match Connection::new().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!(error = %e, "tag_layout event-conn connect failed; backing off");
                    sleep_or_shutdown(RECONNECT_BACKOFF, &mut shutdown).await;
                    continue;
                }
            };
            let mut events = match event_conn.subscribe([EventType::Window]).await {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::debug!(error = %e, "tag_layout subscribe failed; backing off");
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
                            Some(Ok(swayipc_async::Event::Window(win_ev))) => {
                                if win_ev.change == swayipc_async::WindowChange::New {
                                    handle_new_window(&mut cmd_conn, &win_ev.container).await;
                                }
                            }
                            Some(Ok(_)) => {}
                            Some(Err(e)) => {
                                tracing::debug!(error = %e, "tag_layout event stream errored; reconnecting");
                                break;
                            }
                            None => {
                                tracing::debug!("tag_layout event stream ended; reconnecting");
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

async fn handle_new_window(conn: &mut Connection, container: &swayipc_async::Node) {
    let con_id = container.id;
    // Re-fetch the tree to find the workspace this window landed
    // on + count siblings (the event's container alone doesn't
    // tell us workspace context).
    let tree = match conn.get_tree().await {
        Ok(t) => t,
        Err(e) => {
            tracing::debug!(error = %e, "tag_layout get_tree failed; skipping event");
            return;
        }
    };
    let Some(ws_num) = workspace_num_for_con_id(&tree, con_id) else {
        return;
    };
    let store = match TagStore::load_default() {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(error = %e, "tag_layout tag-store load failed; skipping");
            return;
        }
    };
    let Some(owning) = find_owning_tag(&store, ws_num) else {
        return;
    };
    let Some(desired) = owning.default_layout.as_deref() else {
        return;
    };
    if !is_recognised_layout(desired) {
        tracing::debug!(workspace = ws_num, %desired, "tag_layout default unrecognised; skipping");
        return;
    }
    // Only apply on the very first window in the workspace —
    // subsequent windows let the operator drive layout choice.
    let Some(window_count) = window_count_on_workspace(&tree, ws_num) else {
        return;
    };
    if window_count != 1 {
        return;
    }
    let current = current_layout(&tree, ws_num).unwrap_or_default();
    if current == desired {
        return;
    }
    let cmd = layout_command(desired);
    match conn.run_command(&cmd).await {
        Ok(_) => tracing::debug!(workspace = ws_num, %desired, "tag_layout applied"),
        Err(e) => tracing::warn!(workspace = ws_num, %desired, error = %e, "tag_layout command failed"),
    }
}

// ── Pure helpers ────────────────────────────────────────────────────────

/// True if `name` is one of the four sway layout names the design
/// lock recognises. Anything else is silently skipped to avoid
/// passing garbage through to swayipc.
#[must_use]
pub fn is_recognised_layout(name: &str) -> bool {
    matches!(name, "splith" | "splitv" | "tabbed" | "stacked")
}

/// Build the swayipc command string for `layout <name>`.
#[must_use]
pub fn layout_command(name: &str) -> String {
    format!("layout {name}")
}

/// Walk the sway tree to find the workspace number that contains
/// `con_id`. Returns `None` if the container isn't in any
/// workspace (e.g. scratchpad or floating before workspace
/// assignment).
fn workspace_num_for_con_id(node: &swayipc_async::Node, con_id: i64) -> Option<i32> {
    if node.node_type == swayipc_async::NodeType::Workspace {
        if walk_finds_con_id(node, con_id) {
            return node.num;
        }
    }
    for child in &node.nodes {
        if let Some(found) = workspace_num_for_con_id(child, con_id) {
            return Some(found);
        }
    }
    None
}

fn walk_finds_con_id(node: &swayipc_async::Node, target: i64) -> bool {
    if node.id == target {
        return true;
    }
    node.nodes.iter().any(|n| walk_finds_con_id(n, target))
    || node.floating_nodes.iter().any(|n| walk_finds_con_id(n, target))
}

/// Count leaf `Con` windows on workspace `ws_num`. Returns `None`
/// if the workspace isn't found in the tree.
fn window_count_on_workspace(node: &swayipc_async::Node, ws_num: i32) -> Option<usize> {
    let ws_node = find_workspace_node(node, ws_num)?;
    Some(count_leaves(ws_node))
}

fn find_workspace_node<'a>(
    node: &'a swayipc_async::Node,
    ws_num: i32,
) -> Option<&'a swayipc_async::Node> {
    if node.node_type == swayipc_async::NodeType::Workspace && node.num == Some(ws_num) {
        return Some(node);
    }
    for child in &node.nodes {
        if let Some(found) = find_workspace_node(child, ws_num) {
            return Some(found);
        }
    }
    None
}

fn count_leaves(node: &swayipc_async::Node) -> usize {
    if node.node_type == swayipc_async::NodeType::Con && node.nodes.is_empty() {
        return 1;
    }
    node.nodes.iter().map(count_leaves).sum()
}

/// Read the workspace's current container layout (`splith` /
/// `splitv` / `tabbed` / `stacked`) from the tree. Returns
/// `None` if the workspace isn't found.
fn current_layout(node: &swayipc_async::Node, ws_num: i32) -> Option<String> {
    let ws_node = find_workspace_node(node, ws_num)?;
    Some(format!("{:?}", ws_node.layout).to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognised_layouts_lock_the_four_names() {
        assert!(is_recognised_layout("splith"));
        assert!(is_recognised_layout("splitv"));
        assert!(is_recognised_layout("tabbed"));
        assert!(is_recognised_layout("stacked"));
        assert!(!is_recognised_layout("Splith"));
        assert!(!is_recognised_layout("splith "));
        assert!(!is_recognised_layout(""));
        assert!(!is_recognised_layout("output"));
    }

    #[test]
    fn layout_command_round_trips() {
        assert_eq!(layout_command("splith"), "layout splith");
        assert_eq!(layout_command("tabbed"), "layout tabbed");
        // Garbage in / garbage out — the caller is expected to
        // gate via `is_recognised_layout`. Lock the contract so
        // an upstream bug surfaces in tests.
        assert_eq!(layout_command("garbage"), "layout garbage");
    }
}
