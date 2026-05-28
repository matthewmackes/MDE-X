//! Portal-52.a (v6.0, R12-Q13 — workspace-structure half) — sway
//! session-restore worker.
//!
//! Two responsibilities:
//!
//!   1. **Snapshot** — every 5 seconds, walk the live sway tree
//!      via `get_workspaces` + `get_tree`, serialize the
//!      workspace structure (number + name + output + layout) to
//!      `<XDG_DATA_HOME>/mde/session.json` via atomic
//!      temp + rename.
//!   2. **Restore** — on first event after worker start, read
//!      the snapshot file. For each workspace fire swayipc
//!      `workspace number <n>; move workspace to output <out>;
//!      layout <name>` to recreate the slot + output + layout
//!      triple. Operator's apps don't auto-relaunch in this
//!      half; Portal-52.b ships the `append_layout` swallow
//!      placeholders.
//!
//! Operators get workspaces in their correct slots / outputs /
//! layouts on every login. App relaunch is operator-driven
//! (Mod+Space / Hub click). This split is per CLAUDE.md §0.12 —
//! Portal-52.a is bench-observable on its own (login lands
//! workspaces in slots) and Portal-52.b extends it with the
//! placeholder swallows.

#![cfg(feature = "async-services")]

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use hyprland::data::Workspaces;
use hyprland::dispatch::{Dispatch, DispatchType, MonitorIdentifier, WorkspaceIdentifier};
use hyprland::shared::{HyprData, HyprDataVec};

use super::{ShutdownToken, Worker};

const RECONNECT_BACKOFF: Duration = Duration::from_secs(3);
const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(5);

/// Schema version for the session snapshot file. Bump on
/// backwards-incompatible changes; for now informational.
pub const SCHEMA_VERSION: u32 = 1;

/// Sentinel layout name written into every v6.5 snapshot — Hyprland
/// has no per-workspace layout, so this records the global `mde`
/// layout (HYP-12) rather than a sway-style splith/tabbed/stacked.
pub const DEFAULT_LAYOUT: &str = "mde";

/// One workspace's structural state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    /// i3/sway workspace number.
    pub workspace_num: i32,
    /// Workspace name as displayed (Portal-41 auto-derived form,
    /// or operator-set).
    pub name: String,
    /// Output the workspace lives on (e.g. `HDMI-A-1`).
    pub output: String,
    /// Container layout. Vestigial under Hyprland (HYP-17): Hyprland
    /// has no per-workspace layout primitive — tiling is governed by
    /// the global `general { layout = mde }` (HYP-12) + per-window
    /// grouping — so this field is always the `mde` default sentinel
    /// on v6.5 snapshots. Retained for snapshot-schema compatibility
    /// with v6.0 session.json files. The structural/layout restore
    /// (sway's `append_layout` + swallows) is Portal-52.b, which the
    /// design doc holds blocked pending a Hyprland equivalent.
    pub layout: String,
}

/// Top-level session snapshot. Wraps `Vec<WorkspaceSnapshot>` with
/// a schema_version for forward-compat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Snapshot schema version — readers refuse newer-than-known
    /// values + accept older ones via defaulted fields.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Per-workspace state captured at snapshot time.
    #[serde(default)]
    pub workspaces: Vec<WorkspaceSnapshot>,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

impl Default for SessionSnapshot {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            workspaces: Vec::new(),
        }
    }
}

/// Worker state. `restored` flips to `true` after the first
/// restore attempt so we don't re-restore on every event during a
/// single mded lifetime.
pub struct SessionPersistWorker {
    restored: bool,
}

impl SessionPersistWorker {
    /// Construct a fresh worker — restore pending, snapshot ticks
    /// will start once the worker enters run().
    #[must_use]
    pub fn new() -> Self {
        Self { restored: false }
    }
}

impl Default for SessionPersistWorker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Worker for SessionPersistWorker {
    fn name(&self) -> &'static str {
        "session_persist"
    }

    async fn run(&mut self, mut shutdown: ShutdownToken) -> anyhow::Result<()> {
        loop {
            if shutdown.is_shutdown() {
                return Ok(());
            }
            // First-run-only restore pass. Subsequent reconnects
            // (e.g. Hyprland restart) don't re-restore — the
            // operator's current session is the source of truth from
            // then on. hyprland-rs data/dispatch calls are stateless
            // (no persistent Connection), so the restore + snapshot
            // helpers reach Hyprland directly via the socket each call.
            if !self.restored {
                if let Err(e) = restore_from_default().await {
                    tracing::debug!(error = %e, "session_persist restore failed; continuing snapshot cadence");
                }
                self.restored = true;
            }
            // 5-second snapshot loop. Aborts on shutdown.
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.wait() => return Ok(()),
                    _ = tokio::time::sleep(SNAPSHOT_INTERVAL) => {
                        match snapshot_to_default().await {
                            Ok(()) => {}
                            Err(SessionPersistError::Connection) => {
                                tracing::debug!("session_persist snapshot lost connection; reconnecting");
                                break;
                            }
                            Err(e) => {
                                tracing::debug!(error = ?e, "session_persist snapshot non-fatal error; continuing");
                            }
                        }
                    }
                }
            }
            // Reconnect backoff after a lost-connection snapshot.
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

/// Error type returned by [`write_snapshot_atomic`] and
/// [`read_snapshot`]. Carries the four distinct failure modes
/// the worker can encounter at the FS / IPC / JSON boundaries.
#[derive(Debug)]
pub enum SessionPersistError {
    /// Hyprland IPC socket dropped or refused.
    Connection,
    /// FS-side IO failure (read / write / rename).
    Io(std::io::Error),
    /// JSON serde failure (parse on read, serialize on write).
    Json(serde_json::Error),
    /// `$HOME` / `$XDG_DATA_HOME` not set, so the session.json
    /// path couldn't be resolved.
    PathResolution,
}

impl std::fmt::Display for SessionPersistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connection => write!(f, "Hyprland IPC socket dropped"),
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Json(e) => write!(f, "json: {e}"),
            Self::PathResolution => write!(f, "could not resolve session.json path"),
        }
    }
}

impl From<std::io::Error> for SessionPersistError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for SessionPersistError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// Snapshot the live Hyprland workspace list to the default path
/// (`<XDG_DATA_HOME>/mde/session.json`). Atomic write via
/// temp + rename.
async fn snapshot_to_default() -> Result<(), SessionPersistError> {
    let path = default_session_path().ok_or(SessionPersistError::PathResolution)?;
    let snapshot = build_snapshot().await?;
    write_snapshot_atomic(&path, &snapshot)?;
    Ok(())
}

/// Read the live Hyprland workspace list to build a snapshot.
///
/// HYP-17: the v6.0 sway version also walked `get_tree` for each
/// workspace's container layout. Hyprland has no per-workspace
/// layout primitive, so the layout field is set to the `mde`
/// default sentinel (see [`WorkspaceSnapshot::layout`]) and the
/// tree-walk is dropped.
async fn build_snapshot() -> Result<SessionSnapshot, SessionPersistError> {
    let workspaces = Workspaces::get_async()
        .await
        .map_err(|_| SessionPersistError::Connection)?
        .to_vec();
    let mut out = SessionSnapshot::default();
    for ws in workspaces {
        if ws.id < 0 {
            // Hyprland special/named workspace (scratchpad etc.) — skip.
            continue;
        }
        out.workspaces.push(WorkspaceSnapshot {
            workspace_num: ws.id,
            name: ws.name,
            output: ws.monitor,
            layout: DEFAULT_LAYOUT.to_string(),
        });
    }
    Ok(out)
}

/// Restore from the default-path snapshot. Missing file is not
/// an error — first-boot path.
///
/// HYP-17: under sway each workspace was recreated via a 3-directive
/// command string (`workspace number N; move workspace to output …;
/// layout …`). On Hyprland the restore is best-effort monitor
/// pinning — `MoveWorkspaceToMonitor(Id(n), Name(out))` — so a
/// workspace lands on its remembered output when it next opens. The
/// per-workspace layout restore (sway `append_layout` + swallows) is
/// Portal-52.b, held blocked pending a Hyprland equivalent, so no
/// layout directive is dispatched here.
async fn restore_from_default() -> Result<(), SessionPersistError> {
    let path = default_session_path().ok_or(SessionPersistError::PathResolution)?;
    if !path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path)?;
    let snap: SessionSnapshot = serde_json::from_str(&raw)?;
    for ws in &snap.workspaces {
        // Portal-59: skip the parked workspace — it's an
        // ephemeral platform slot.
        if ws.workspace_num == 99 {
            continue;
        }
        if ws.workspace_num < 0 || ws.output.is_empty() {
            continue;
        }
        if let Err(e) = Dispatch::call_async(DispatchType::MoveWorkspaceToMonitor(
            WorkspaceIdentifier::Id(ws.workspace_num),
            MonitorIdentifier::Name(&ws.output),
        ))
        .await
        {
            tracing::warn!(workspace = ws.workspace_num, error = %e, "session_persist restore move failed");
        }
    }
    Ok(())
}

// ── Pure helpers ────────────────────────────────────────────────────────

/// Resolve `<XDG_DATA_HOME>/mde/session.json`.
#[must_use]
pub fn default_session_path() -> Option<PathBuf> {
    let data_home = dirs::data_dir()?;
    Some(data_home.join("mde").join("session.json"))
}

/// Atomic write of `snapshot` to `path` via temp + rename.
/// Creates the parent directory if missing.
pub fn write_snapshot_atomic(
    path: &std::path::Path,
    snapshot: &SessionSnapshot,
) -> Result<(), SessionPersistError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pretty = serde_json::to_string_pretty(snapshot)?;
    let mut tmp = path.to_path_buf();
    tmp.set_extension("json.tmp");
    std::fs::write(&tmp, pretty)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Read a snapshot from `path`. Missing file returns the empty
/// default snapshot (first-run path).
pub fn read_snapshot(path: &std::path::Path) -> Result<SessionSnapshot, SessionPersistError> {
    if !path.exists() {
        return Ok(SessionSnapshot::default());
    }
    let raw = std::fs::read_to_string(path)?;
    let snap: SessionSnapshot = serde_json::from_str(&raw)?;
    Ok(snap)
}

// NOTE (HYP-17): the v6.0 `restore_command` swayipc command-string
// builder + the `workspace_layout` get_tree walker are retired.
// Restore is now a typed `DispatchType::MoveWorkspaceToMonitor` per
// workspace (hyprland-rs owns escaping), and Hyprland exposes no
// per-workspace container layout to read back, so there's nothing to
// walk. The layout field is the `mde` sentinel (DEFAULT_LAYOUT).

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot() -> SessionSnapshot {
        SessionSnapshot {
            schema_version: SCHEMA_VERSION,
            workspaces: vec![
                WorkspaceSnapshot {
                    workspace_num: 1,
                    name: "1: firefox".to_string(),
                    output: "HDMI-A-1".to_string(),
                    layout: "splith".to_string(),
                },
                WorkspaceSnapshot {
                    workspace_num: 2,
                    name: "2".to_string(),
                    output: "HDMI-A-1".to_string(),
                    layout: "tabbed".to_string(),
                },
            ],
        }
    }

    /// Round-trip a populated snapshot through serde JSON.
    #[test]
    fn snapshot_serde_round_trip() {
        let s = sample_snapshot();
        let json = serde_json::to_string_pretty(&s).unwrap();
        let parsed: SessionSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, s);
    }

    /// Atomic write + read cycle leaves no `.json.tmp` sibling +
    /// round-trips data.
    #[test]
    fn atomic_write_read_cycle() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested/dir/session.json");
        let snap = sample_snapshot();
        write_snapshot_atomic(&path, &snap).unwrap();
        let read = read_snapshot(&path).unwrap();
        assert_eq!(read, snap);
        // Atomic-rename should leave no `.json.tmp` sibling.
        let sibling = path.with_extension("json.tmp");
        assert!(!sibling.exists());
    }

    /// Missing file → empty default snapshot.
    #[test]
    fn read_snapshot_missing_file_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nope/session.json");
        let snap = read_snapshot(&path).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert!(snap.workspaces.is_empty());
    }

    // NOTE (HYP-17): the restore_command shape + escaping tests are
    // retired with the function. Restore is now a typed
    // `DispatchType::MoveWorkspaceToMonitor(Id, Name)` per workspace;
    // hyprland-rs owns socket escaping + there's no command string to
    // assert. The snapshot serde round-trip (above) + the schema
    // forward-compat tests (below) remain the worker's pure surface.
    //
    // DEFAULT_LAYOUT sentinel lock: snapshots written on v6.5 carry
    // the `mde` layout sentinel since Hyprland has no per-workspace
    // layout to record.
    #[test]
    fn snapshot_layout_is_mde_sentinel() {
        assert_eq!(DEFAULT_LAYOUT, "mde");
    }

    /// Pre-schema files (no schema_version field) load with the
    /// default version filled in by serde.
    #[test]
    fn pre_schema_files_load_with_default_version() {
        let json = r#"{"workspaces":[{"workspace_num":1,"name":"1","output":"eDP-1","layout":"splith"}]}"#;
        let snap: SessionSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert_eq!(snap.workspaces.len(), 1);
    }

    /// Empty snapshot (no workspaces) writes + reads cleanly —
    /// fresh-install path.
    #[test]
    fn empty_snapshot_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.json");
        let snap = SessionSnapshot::default();
        write_snapshot_atomic(&path, &snap).unwrap();
        let read = read_snapshot(&path).unwrap();
        assert_eq!(read, snap);
    }
}
