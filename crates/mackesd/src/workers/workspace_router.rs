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
use hyprland::dispatch::{Dispatch, DispatchType, MonitorIdentifier, WorkspaceIdentifier};
use hyprland::event_listener::{Event, EventStream};
use mackes_mesh_types::{Tag, TagMember, TagStore};

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
        // Reconnect loop. `EventStream::new()` is infallible; a
        // connect failure surfaces as the stream's first Err item
        // and routes into the backoff path below.
        loop {
            if shutdown.is_shutdown() {
                return Ok(());
            }
            let mut events = EventStream::new();
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.wait() => return Ok(()),
                    next = events.next() => {
                        match next {
                            // Hyprland's `WorkspaceAdded` (createworkspacev2)
                            // is the analog of sway's `WorkspaceChange::Init`.
                            Some(Ok(Event::WorkspaceAdded(data))) => {
                                handle_init(data.id).await;
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
            sleep_or_shutdown(RECONNECT_BACKOFF, &mut shutdown).await;
        }
    }
}

async fn sleep_or_shutdown(dur: Duration, shutdown: &mut ShutdownToken) {
    tokio::select! {
        _ = shutdown.wait() => {}
        _ = tokio::time::sleep(dur) => {}
    }
}

/// Handle a `WorkspaceAdded` event for workspace id `num`. Loads
/// the tag store fresh on each event so operator edits take effect
/// immediately without a daemon restart.
async fn handle_init(num: i32) {
    // Hyprland numeric workspace ids start at 1; negative ids are
    // special/named workspaces (scratchpad etc.) the router leaves
    // to Hyprland's own placement.
    if num <= 0 {
        return;
    }
    let store = match TagStore::load_default() {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(error = %e, "workspace_router tag-store load failed; skipping event");
            return;
        }
    };
    // HYP-8.5 — load the tag-manifest snapshot each event so per-tag
    // `output` can override the legacy TagStore `preferred_output`.
    // Manifest load is fail-soft: missing dir / unreadable file
    // falls through to TagStore. Cheap relative to the hyprctl
    // dispatch that follows.
    let manifests = crate::config::default_manifests_dir()
        .and_then(|d| crate::config::load_tag_manifests(&d).ok());
    let Some(output_name) =
        preferred_output_for_workspace_with_manifests(&store, num, manifests.as_deref())
    else {
        return;
    };
    match Dispatch::call_async(DispatchType::MoveWorkspaceToMonitor(
        WorkspaceIdentifier::Id(num),
        MonitorIdentifier::Name(&output_name),
    ))
    .await
    {
        Ok(()) => tracing::debug!(workspace = num, %output_name, "workspace_router moved workspace"),
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
    preferred_output_for_workspace_with_manifests(store, ws_num, None)
}

/// HYP-10.sway-bridge — same as [`preferred_output_for_workspace`]
/// but with an explicit tag-manifest snapshot. Resolution
/// precedence:
///
/// 1. **Tag manifest `output`** (HYP-8.5 source of truth) — when
///    the manifest matching the workspace's owning tag carries a
///    non-empty `output` field, that wins. Per the simplification
///    re-lock, the compositor-side output policy lives in the
///    manifest.
/// 2. **TagStore `preferred_output`** (Portal-18.a legacy) —
///    fallback so existing operator setups stay working until
///    they migrate.
/// 3. `None` — sway picks the natural output (no-op move command
///    issued).
#[must_use]
pub fn preferred_output_for_workspace_with_manifests(
    store: &TagStore,
    ws_num: i32,
    manifests: Option<&[crate::config::TagManifest]>,
) -> Option<String> {
    let owning_tag = find_owning_tag(store, ws_num)?;

    // Priority 1: tag-manifest output.
    if let Some(ms) = manifests {
        if let Some(m) = ms.iter().find(|m| m.name == owning_tag.name) {
            if let Some(o) = m.output.as_deref() {
                if !o.trim().is_empty() {
                    return Some(o.to_string());
                }
            }
        }
    }

    // Priority 2: TagStore preferred_output (legacy).
    owning_tag.preferred_output.clone()
}

// NOTE (HYP-10): the v6.0 `move_workspace_command` swayipc
// command-string builder + its output-name escaping is retired.
// Hyprland's `DispatchType::MoveWorkspaceToMonitor(Id(n), Name(out))`
// takes the id + monitor name directly; hyprland-rs owns socket-level
// escaping, so there's no command string to build here.

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

    // NOTE (HYP-10): the move_workspace_command escaping test is
    // retired with the function — the rename/move side-effect is now
    // a typed `DispatchType::MoveWorkspaceToMonitor`, and hyprland-rs
    // owns socket escaping. The output-resolution precedence tests
    // above remain the worker's testable surface.

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

    // ── HYP-10.sway-bridge — tag-manifest output overrides
    //    TagStore preferred_output ──────────────────────────────

    fn dev_manifest_with(output: Option<&str>) -> crate::config::TagManifest {
        crate::config::TagManifest {
            name: "Dev".to_string(),
            output: output.map(|s| s.to_string()),
            ..crate::config::TagManifest::default()
        }
    }

    /// Manifest output wins over TagStore preferred_output.
    #[test]
    fn manifest_output_overrides_tagstore_preferred() {
        let mut store = TagStore::default();
        store.add(dev_tag_on_hdmi(&[1])).unwrap(); // TagStore: HDMI-A-1
        let manifests = vec![dev_manifest_with(Some("DP-2"))];
        let out =
            preferred_output_for_workspace_with_manifests(&store, 1, Some(&manifests));
        assert_eq!(out.as_deref(), Some("DP-2"));
    }

    /// Manifest without output → fall through to TagStore.
    #[test]
    fn manifest_without_output_falls_through_to_tagstore() {
        let mut store = TagStore::default();
        store.add(dev_tag_on_hdmi(&[1])).unwrap();
        let manifests = vec![dev_manifest_with(None)];
        let out =
            preferred_output_for_workspace_with_manifests(&store, 1, Some(&manifests));
        assert_eq!(out.as_deref(), Some("HDMI-A-1"));
    }

    /// Manifest with empty / whitespace-only output → fall
    /// through (operator typed nothing meaningful).
    #[test]
    fn manifest_with_empty_output_falls_through() {
        let mut store = TagStore::default();
        store.add(dev_tag_on_hdmi(&[1])).unwrap();
        let manifests = vec![dev_manifest_with(Some("  "))];
        let out =
            preferred_output_for_workspace_with_manifests(&store, 1, Some(&manifests));
        assert_eq!(out.as_deref(), Some("HDMI-A-1"));
    }

    /// None snapshot → behaves exactly like the bare function.
    #[test]
    fn none_manifests_means_tagstore_only() {
        let mut store = TagStore::default();
        store.add(dev_tag_on_hdmi(&[1])).unwrap();
        let out = preferred_output_for_workspace_with_manifests(&store, 1, None);
        assert_eq!(out.as_deref(), Some("HDMI-A-1"));
    }

    /// Manifest for a different tag name → ignored.
    #[test]
    fn manifest_for_different_tag_is_ignored() {
        let mut store = TagStore::default();
        store.add(dev_tag_on_hdmi(&[1])).unwrap();
        let manifests = vec![crate::config::TagManifest {
            name: "Other".to_string(),
            output: Some("DP-2".to_string()),
            ..crate::config::TagManifest::default()
        }];
        let out =
            preferred_output_for_workspace_with_manifests(&store, 1, Some(&manifests));
        // Falls through to TagStore.
        assert_eq!(out.as_deref(), Some("HDMI-A-1"));
    }
}
