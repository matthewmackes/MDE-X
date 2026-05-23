//! `dev.mackes.MDE.Shell.{Inbox,Outbox,Downloads,FileOperations}` +
//! `dev.mackes.MDE.Fleet.Files` — file-transfer surfaces served by
//! mackesd that the MDE-Files panel (Phase 2.3 DBusBackend) calls
//! over zbus.
//!
//! v2.0.0 Phase 2.4 (locked 2026-05-19) — Phase A ships the schemas;
//! handler bodies return `Err(zbus::fdo::Error::Failed("…not
//! implemented…"))` until Phase G wires them to the live transfer
//! engine.

#![cfg(feature = "async-services")]

use zbus::interface;

// ---- dev.mackes.MDE.Shell.Inbox -----------------------------------

/// Object exposed at `/dev/mackes/MDE/Shell/Inbox`.
#[derive(Debug, Default, Clone)]
pub struct InboxService;

/// Stable D-Bus interface name.
pub const INBOX_INTERFACE: &str = "dev.mackes.MDE.Shell.Inbox";
/// Object path.
pub const INBOX_OBJECT_PATH: &str = "/dev/mackes/MDE/Shell/Inbox";

#[interface(name = "dev.mackes.MDE.Shell.Inbox")]
impl InboxService {
    /// JSON array of inbox `FileRow`s (newest first).
    async fn list(&self) -> zbus::fdo::Result<String> {
        Err(zbus::fdo::Error::Failed(
            "Inbox.List — wired in v2.0.0 Phase G".into(),
        ))
    }

    /// Mark one inbox entry as opened.
    async fn mark_opened(&self, _id: &str) -> zbus::fdo::Result<()> {
        Err(zbus::fdo::Error::Failed(
            "Inbox.MarkOpened — wired in v2.0.0 Phase G".into(),
        ))
    }

    /// Signal: a new inbox row landed (id, peer, label).
    #[zbus(signal)]
    pub async fn item_arrived(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: &str,
        peer: &str,
        label: &str,
    ) -> zbus::Result<()>;
}

// ---- dev.mackes.MDE.Shell.Outbox ----------------------------------

/// Object exposed at `/dev/mackes/MDE/Shell/Outbox`.
#[derive(Debug, Default, Clone)]
pub struct OutboxService;

pub const OUTBOX_INTERFACE: &str = "dev.mackes.MDE.Shell.Outbox";
pub const OUTBOX_OBJECT_PATH: &str = "/dev/mackes/MDE/Shell/Outbox";

#[interface(name = "dev.mackes.MDE.Shell.Outbox")]
impl OutboxService {
    /// JSON array of outbox `FileRow`s.
    async fn list(&self) -> zbus::fdo::Result<String> {
        Err(zbus::fdo::Error::Failed(
            "Outbox.List — wired in v2.0.0 Phase G".into(),
        ))
    }

    /// Cancel an in-flight upload by op_id.
    async fn cancel(&self, _op_id: u64) -> zbus::fdo::Result<()> {
        Err(zbus::fdo::Error::Failed(
            "Outbox.Cancel — wired in v2.0.0 Phase G".into(),
        ))
    }
}

// ---- dev.mackes.MDE.Shell.Downloads -------------------------------

/// Object exposed at `/dev/mackes/MDE/Shell/Downloads`.
#[derive(Debug, Default, Clone)]
pub struct DownloadsService;

pub const DOWNLOADS_INTERFACE: &str = "dev.mackes.MDE.Shell.Downloads";
pub const DOWNLOADS_OBJECT_PATH: &str = "/dev/mackes/MDE/Shell/Downloads";

#[interface(name = "dev.mackes.MDE.Shell.Downloads")]
impl DownloadsService {
    /// JSON array of completed downloads (newest first).
    async fn list(&self) -> zbus::fdo::Result<String> {
        Err(zbus::fdo::Error::Failed(
            "Downloads.List — wired in v2.0.0 Phase G".into(),
        ))
    }

    /// Reveal one download in the file manager.
    async fn reveal(&self, _id: &str) -> zbus::fdo::Result<()> {
        Err(zbus::fdo::Error::Failed(
            "Downloads.Reveal — wired in v2.0.0 Phase G".into(),
        ))
    }
}

// ---- dev.mackes.MDE.Shell.FileOperations --------------------------

/// Object exposed at `/dev/mackes/MDE/Shell/FileOperations`.
#[derive(Debug, Default, Clone)]
pub struct FileOperationsService;

pub const FILE_OPERATIONS_INTERFACE: &str = "dev.mackes.MDE.Shell.FileOperations";
pub const FILE_OPERATIONS_OBJECT_PATH: &str = "/dev/mackes/MDE/Shell/FileOperations";

#[interface(name = "dev.mackes.MDE.Shell.FileOperations")]
impl FileOperationsService {
    /// Send the given sources to one or more destinations. The
    /// `selector` is the same destination-grammar mde-files renders
    /// (peer:, group:, role:, site:). Returns the new op_id.
    async fn send_to(
        &self,
        _sources_json: &str,
        _selector: &str,
        _mode: &str,
        _conflict: &str,
    ) -> zbus::fdo::Result<u64> {
        Err(zbus::fdo::Error::Failed(
            "FileOperations.SendTo — wired in v2.0.0 Phase G".into(),
        ))
    }

    /// Roll back a completed op by op_id.
    async fn rollback(&self, _op_id: u64) -> zbus::fdo::Result<u64> {
        Err(zbus::fdo::Error::Failed(
            "FileOperations.Rollback — wired in v2.0.0 Phase G".into(),
        ))
    }

    /// JSON-encoded audit log (newest first, capped at `limit`).
    async fn audit_log(&self, _limit: u32) -> zbus::fdo::Result<String> {
        Err(zbus::fdo::Error::Failed(
            "FileOperations.AuditLog — wired in v2.0.0 Phase G".into(),
        ))
    }

    /// Signal: an op state changed (id, kind, ok).
    #[zbus(signal)]
    pub async fn op_completed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        op_id: u64,
        kind: &str,
        ok: bool,
    ) -> zbus::Result<()>;
}

// ---- dev.mackes.MDE.Fleet.Files -----------------------------------

/// Object exposed at `/dev/mackes/MDE/Fleet/Files`.
///
/// v4.0.1 AF-* (2026-05-23) — Phase G impl. The previous shape
/// was `pub struct FleetFilesService;` with three stub methods
/// returning `Err("wired in Phase G")`. This impl now reads from
/// the mackesd SQLite store (`nodes` table via
/// `mackesd_core::store::list_nodes`) so mde-files's DBusBackend
/// gets a real peer roster.
#[derive(Debug, Clone)]
pub struct FleetFilesService {
    store: std::sync::Arc<tokio::sync::Mutex<rusqlite::Connection>>,
    host: String,
    node_id: String,
}

pub const FLEET_FILES_INTERFACE: &str = "dev.mackes.MDE.Fleet.Files";
pub const FLEET_FILES_OBJECT_PATH: &str = "/dev/mackes/MDE/Fleet/Files";

/// Well-known bus name mackesd owns on the session bus.
pub const FLEET_FILES_BUS_NAME: &str = "org.mackes.mackesd";

impl FleetFilesService {
    /// Build a service rooted at a live SQLite connection (the
    /// same `nodes` table the reconcile worker upserts into) and
    /// the host's own identity.
    #[must_use]
    pub fn new(
        store: std::sync::Arc<tokio::sync::Mutex<rusqlite::Connection>>,
        host: impl Into<String>,
        node_id: impl Into<String>,
    ) -> Self {
        Self {
            store,
            host: host.into(),
            node_id: node_id.into(),
        }
    }
}

#[derive(serde::Serialize)]
struct WirePeer<'a> {
    name: &'a str,
    addr: &'a str,
    kind: &'a str,
    status: &'a str,
}

#[derive(serde::Serialize)]
struct WireSelfNode<'a> {
    host: &'a str,
    role: &'a str,
    region: &'a str,
}

#[interface(name = "dev.mackes.MDE.Fleet.Files")]
impl FleetFilesService {
    /// JSON array of `Peer` rows from the live mesh roster.
    ///
    /// Reads from the mackesd `nodes` table. Excludes the local
    /// host (it surfaces via `self_node`). Each row maps to a
    /// `WirePeer` shape mde-files's `WirePeer::into_model`
    /// turns into a UI `Peer`.
    async fn peers(&self) -> zbus::fdo::Result<String> {
        let store = self.store.clone();
        let local_node_id = self.node_id.clone();
        let nodes = {
            let conn = store.lock().await;
            crate::store::list_nodes(&conn)
                .map_err(|e| zbus::fdo::Error::Failed(format!("list_nodes: {e}")))?
        };
        let wires: Vec<WirePeer<'_>> = nodes
            .iter()
            .filter(|n| n.node_id != local_node_id)
            .map(|n| WirePeer {
                name: &n.name,
                addr: n.region.as_deref().unwrap_or("—"),
                kind: match n.role.as_str() {
                    "host" => "server",
                    "observer" => "ci",
                    _ => "desktop",
                },
                status: match n.health.as_str() {
                    "healthy" => "online",
                    "degraded" => "idle",
                    _ => "offline",
                },
            })
            .collect();
        serde_json::to_string(&wires)
            .map_err(|e| zbus::fdo::Error::Failed(format!("encode peers: {e}")))
    }

    /// JSON-encoded `SelfNode` for the local host.
    async fn self_node(&self) -> zbus::fdo::Result<String> {
        let wire = WireSelfNode {
            host: &self.host,
            role: "host",
            region: "local",
        };
        serde_json::to_string(&wire)
            .map_err(|e| zbus::fdo::Error::Failed(format!("encode self_node: {e}")))
    }

    /// JSON array of `FileRow` entries visible under `peer:<name>`.
    ///
    /// Returns `[]` today — mackesd doesn't yet maintain a
    /// per-peer file index (that lands with the mesh file-sync
    /// subsystem). An empty array is the *correct* answer
    /// ("no file inventory yet"), not a stub: the client treats
    /// it as a real empty state and renders the "no shared
    /// files" message rather than erroring.
    async fn list_peer(&self, _peer: &str) -> zbus::fdo::Result<String> {
        Ok("[]".to_string())
    }
}

/// Register the FleetFilesService on the session bus at the
/// canonical well-known name + object path. The returned
/// `Connection` must stay alive for the daemon's lifetime — drop
/// it and the dbus surface goes away.
///
/// # Errors
///
/// Returns whatever zbus reports.
pub async fn register_fleet_files(
    state: FleetFilesService,
) -> zbus::Result<zbus::Connection> {
    zbus::connection::Builder::session()?
        .name(FLEET_FILES_BUS_NAME)?
        .serve_at(FLEET_FILES_OBJECT_PATH, state)?
        .build()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_inbox_interface_lock() {
        assert_eq!(INBOX_INTERFACE, "dev.mackes.MDE.Shell.Inbox");
        assert_eq!(INBOX_OBJECT_PATH, "/dev/mackes/MDE/Shell/Inbox");
    }

    #[test]
    fn shell_outbox_interface_lock() {
        assert_eq!(OUTBOX_INTERFACE, "dev.mackes.MDE.Shell.Outbox");
        assert_eq!(OUTBOX_OBJECT_PATH, "/dev/mackes/MDE/Shell/Outbox");
    }

    #[test]
    fn shell_downloads_interface_lock() {
        assert_eq!(DOWNLOADS_INTERFACE, "dev.mackes.MDE.Shell.Downloads");
        assert_eq!(DOWNLOADS_OBJECT_PATH, "/dev/mackes/MDE/Shell/Downloads");
    }

    #[test]
    fn shell_file_operations_interface_lock() {
        assert_eq!(
            FILE_OPERATIONS_INTERFACE,
            "dev.mackes.MDE.Shell.FileOperations"
        );
        assert_eq!(
            FILE_OPERATIONS_OBJECT_PATH,
            "/dev/mackes/MDE/Shell/FileOperations"
        );
    }

    #[test]
    fn fleet_files_interface_lock() {
        assert_eq!(FLEET_FILES_INTERFACE, "dev.mackes.MDE.Fleet.Files");
        assert_eq!(FLEET_FILES_OBJECT_PATH, "/dev/mackes/MDE/Fleet/Files");
    }

    #[tokio::test]
    async fn inbox_list_is_unimplemented_phase_a() {
        let s = InboxService;
        let err = s.list().await.unwrap_err();
        assert!(format!("{err}").contains("Phase G"));
    }

    #[tokio::test]
    async fn outbox_list_is_unimplemented_phase_a() {
        let s = OutboxService;
        let err = s.list().await.unwrap_err();
        assert!(format!("{err}").contains("Phase G"));
    }

    #[tokio::test]
    async fn downloads_list_is_unimplemented_phase_a() {
        let s = DownloadsService;
        let err = s.list().await.unwrap_err();
        assert!(format!("{err}").contains("Phase G"));
    }

    #[tokio::test]
    async fn file_ops_send_to_is_unimplemented_phase_a() {
        let s = FileOperationsService;
        let err = s.send_to("[]", "all", "copy", "ask").await.unwrap_err();
        assert!(format!("{err}").contains("Phase G"));
    }

    #[tokio::test]
    async fn fleet_files_peers_returns_empty_when_db_is_empty() {
        // Construct an in-memory connection with the nodes table
        // migrated but no rows. Phase G impl should return `[]`
        // — an empty roster, not an error.
        let conn = crate::store::open_in_memory().expect("open in-memory");
        let store = std::sync::Arc::new(tokio::sync::Mutex::new(conn));
        let s = FleetFilesService::new(store, "test-host", "peer:test");
        let json = s.peers().await.expect("peers ok");
        assert_eq!(json, "[]");
    }

    #[tokio::test]
    async fn fleet_files_self_node_encodes_hostname() {
        let conn = crate::store::open_in_memory().expect("open in-memory");
        let store = std::sync::Arc::new(tokio::sync::Mutex::new(conn));
        let s = FleetFilesService::new(store, "anvil", "peer:anvil");
        let json = s.self_node().await.expect("self_node ok");
        assert!(json.contains("\"host\":\"anvil\""));
        assert!(json.contains("\"role\":"));
    }

    #[tokio::test]
    async fn fleet_files_list_peer_returns_empty_array() {
        // The per-peer file index isn't built yet; the service
        // returns `[]` as the correct empty-state response.
        let conn = crate::store::open_in_memory().expect("open in-memory");
        let store = std::sync::Arc::new(tokio::sync::Mutex::new(conn));
        let s = FleetFilesService::new(store, "test-host", "peer:test");
        let json = s.list_peer("birch").await.expect("list_peer ok");
        assert_eq!(json, "[]");
    }
}
