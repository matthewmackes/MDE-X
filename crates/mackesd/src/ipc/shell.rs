//! `dev.mackes.MDE.Shell` — top-level shell control (health, version,
//! worker pool status).
//!
//! v2.0.0 Phase 0.4 rebrand — interface name moved from
//! `org.mackes.Shell` to `dev.mackes.MDE.Shell`. Backward-compat
//! alias .service file ships under the old name for one release; see
//! `data/dbus-1/services/`.
//!
//! v4.1 (2026-05-24) — Healthz + Workers wired up. `register_shell_on`
//! attaches the service to the existing daemon D-Bus connection
//! alongside Fleet.Files and Nebula.Status. The service carries a
//! `ShellState` with the daemon's db_path + an Arc<Vec<String>> of
//! live worker names that `run_serve` populates as it spawns each
//! supervisor child. Healthz returns the same `HealthReport`
//! envelope as the `mackesd healthz` CLI so panel reads stay in
//! parity with the command-line surface; the live-probe
//! enhancement (read store + heartbeat files) is a follow-up to
//! the existing `mackesd healthz` improvement, NOT a Shell-only
//! concern.

#![cfg(feature = "async-services")]

use std::path::PathBuf;
use std::sync::Arc;

use zbus::interface;

/// Object exposed at `/dev/mackes/MDE/Shell`.
#[derive(Debug, Clone)]
pub struct ShellService {
    state: Arc<ShellState>,
}

impl Default for ShellService {
    fn default() -> Self {
        Self {
            state: Arc::new(ShellState::default()),
        }
    }
}

/// Live state the daemon binds at registration time. Cheap to
/// share via `Arc` so the service handle stays `Clone` for
/// zbus's interface registration.
///
/// `worker_names` is a shared `Mutex<Vec<String>>` because
/// some workers spawn AFTER ShellService registers (KDC host,
/// reconcile). `run_serve` pushes to the shared vec at each
/// spawn site so Workers() always reflects what's currently
/// running, not a snapshot frozen at registration time.
#[derive(Debug, Default, Clone)]
pub struct ShellState {
    /// Sqlite store path. Healthz reads via this on every call so
    /// the report reflects current store contents, not a snapshot
    /// taken at registration time. Empty when the daemon is
    /// running without a store (only the `version` method works
    /// in that case).
    pub db_path: PathBuf,
    /// Shared roster of spawned worker names, in spawn order.
    /// The daemon writes (push) during each `sup.spawn()` call
    /// site; Workers() reads (lock + clone) on each D-Bus call.
    /// Held across awaits in the zbus method but only via
    /// momentary lock acquisition (clone-then-drop pattern), so
    /// there's no deadlock risk with the tokio scheduler.
    pub worker_names: Arc<std::sync::Mutex<Vec<String>>>,
}

impl ShellService {
    /// Construct against a live `ShellState`. Used by
    /// `register_shell_on` in `run_serve`.
    #[must_use]
    pub fn new(state: ShellState) -> Self {
        Self {
            state: Arc::new(state),
        }
    }
}

/// Stable D-Bus name used by Phase 0.4-onward callers. The legacy
/// `org.mackes.Shell` alias ships through one v2.x line for
/// backward-compat.
pub const SERVICE_NAME: &str = "dev.mackes.MDE.Shell";

/// Object-path under [`SERVICE_NAME`]. Matches the
/// reverse-slash convention zbus picks by default.
pub const OBJECT_PATH: &str = "/dev/mackes/MDE/Shell";

#[interface(name = "dev.mackes.MDE.Shell")]
impl ShellService {
    /// Compiled crate version (`CARGO_PKG_VERSION`).
    async fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// JSON-encoded [`crate::health::HealthReport`]. v4.1 — returns
    /// the same `HealthReport::empty()` shape the `mackesd healthz`
    /// CLI subcommand emits today. When the CLI's healthz grows a
    /// live-probe path (read store + heartbeat files), both
    /// surfaces inherit the improvement automatically.
    async fn healthz(&self) -> zbus::fdo::Result<String> {
        let report = crate::health::HealthReport::empty();
        report
            .to_json_line()
            .map_err(|e| zbus::fdo::Error::Failed(format!("healthz encode: {e}")))
    }

    /// List currently-spawned worker names. v4.1 — sourced from
    /// the `ShellState::worker_names` shared roster the daemon
    /// pushes to during each `sup.spawn()` call. Stable order
    /// matches the supervisor's spawn sequence in `run_serve`.
    /// The lock acquisition is brief (clone-then-drop) so it
    /// can't deadlock with the tokio scheduler.
    async fn workers(&self) -> zbus::fdo::Result<Vec<String>> {
        let guard = self
            .state
            .worker_names
            .lock()
            .map_err(|e| zbus::fdo::Error::Failed(format!("worker_names lock: {e}")))?;
        Ok(guard.clone())
    }
}

/// v4.1 — register the ShellService on an EXISTING zbus
/// `Connection`. Matches the pattern in `ipc/nebula.rs`'s
/// `register_nebula_status_on`: the daemon shares one bus name
/// (`org.mackes.mackesd`) across every IPC surface so callers
/// don't have to discover a separate connection per object.
///
/// # Errors
///
/// Returns whatever zbus reports — usually an object-path
/// collision when the same daemon process registers twice.
pub async fn register_shell_on(
    conn: &zbus::Connection,
    state: ShellState,
) -> zbus::Result<()> {
    conn.object_server()
        .at(OBJECT_PATH, ShellService::new(state))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn version_matches_crate() {
        let svc = ShellService::default();
        assert_eq!(svc.version().await, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn service_name_carries_mde_namespace() {
        assert_eq!(SERVICE_NAME, "dev.mackes.MDE.Shell");
        assert!(SERVICE_NAME.starts_with("dev.mackes.MDE."));
    }

    #[test]
    fn object_path_mirrors_service_name_segments() {
        assert_eq!(OBJECT_PATH, "/dev/mackes/MDE/Shell");
    }

    #[tokio::test]
    async fn healthz_returns_health_report_json() {
        let svc = ShellService::default();
        let json = svc.healthz().await.expect("healthz");
        // Round-trips to HealthReport with the current schema.
        let parsed: crate::health::HealthReport =
            serde_json::from_str(&json).expect("decode");
        assert_eq!(parsed.schema, crate::health::HealthReport::CURRENT_SCHEMA);
        assert_eq!(parsed.version, env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn workers_returns_empty_for_default_state() {
        let svc = ShellService::default();
        let names = svc.workers().await.expect("workers");
        assert!(names.is_empty());
    }

    #[tokio::test]
    async fn workers_reflects_state_snapshot_in_spawn_order() {
        let names_shared = Arc::new(std::sync::Mutex::new(vec![
            "clipboard".to_string(),
            "mdns".into(),
            "fs_sync".into(),
            "heartbeat".into(),
            "mesh_router".into(),
        ]));
        let state = ShellState {
            db_path: PathBuf::from("/tmp/test.sqlite"),
            worker_names: Arc::clone(&names_shared),
        };
        let svc = ShellService::new(state);
        let names = svc.workers().await.expect("workers");
        assert_eq!(
            names,
            vec![
                "clipboard".to_string(),
                "mdns".into(),
                "fs_sync".into(),
                "heartbeat".into(),
                "mesh_router".into(),
            ]
        );
    }

    #[tokio::test]
    async fn workers_reflects_post_registration_appends() {
        // The daemon registers ShellService BEFORE spawning every
        // worker (KDC + reconcile spawn after IPC registration).
        // The shared Mutex must pick up post-registration pushes.
        let names_shared = Arc::new(std::sync::Mutex::new(vec!["clipboard".to_string()]));
        let state = ShellState {
            db_path: PathBuf::new(),
            worker_names: Arc::clone(&names_shared),
        };
        let svc = ShellService::new(state);
        // After registration: another push lands.
        names_shared.lock().unwrap().push("kdc_host".into());
        let names = svc.workers().await.expect("workers");
        assert_eq!(names, vec!["clipboard".to_string(), "kdc_host".into()]);
    }

    #[test]
    fn shell_state_default_carries_empty_paths_and_workers() {
        let s = ShellState::default();
        assert_eq!(s.db_path, PathBuf::new());
        assert!(s.worker_names.lock().unwrap().is_empty());
    }
}
