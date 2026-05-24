//! Mesh-first backend — talks to mackesd's live `dev.mackes.MDE.
//! Nebula.Status` + `dev.mackes.MDE.Gluster.Status` D-Bus surfaces
//! and merges the two snapshots into the shapes mde-files renders
//! (peer roster, mesh volume status, mount state, heal queue).
//!
//! Why this exists. The previous `DBusBackend` talked to
//! `dev.mackes.MDE.Fleet.Files`, which returns `[]` for
//! `ListPeer(_)` because mackesd doesn't maintain a per-peer file
//! index (and won't — the v5.0.0 GlusterFS lock makes the
//! mesh-home volume the shared file plane, not a per-peer
//! inventory). For a mesh-first manager the right sources of
//! truth are:
//!
//!   1. **Nebula.Status** — peer reachability + overlay IPs +
//!      handshake age. Every peer in the mesh shows up here as
//!      soon as the lighthouse signs its cert. Live as of
//!      NF-Bundle-0 (v2.5).
//!   2. **Gluster.Status** — volume size + heal queue + per-peer
//!      brick free-space + mount status. Live as of GF-2.2.a
//!      (v5.0.0, shipped 2026-05-24).
//!
//! This module reads both, merges peer rows keyed on overlay IP
//! (when known) or hostname, and exposes a single mesh-flavoured
//! API the UI binds against. No new D-Bus surface is added —
//! everything here is a thin client over what mackesd already
//! serves.

#![cfg(feature = "dbus")]

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;
use tokio::runtime::Runtime;
use zbus::{Connection, Proxy};

/// Well-known bus name mackesd registers.
pub const BUS_NAME: &str = "org.mackes.mackesd";

/// Object path for `dev.mackes.MDE.Nebula.Status` (mirror of
/// `mackesd_core::ipc::nebula::NEBULA_STATUS_OBJECT_PATH`).
pub const NEBULA_STATUS_OBJECT_PATH: &str = "/dev/mackes/MDE/Nebula/Status";

/// Interface name for `dev.mackes.MDE.Nebula.Status`.
pub const NEBULA_STATUS_INTERFACE: &str = "dev.mackes.MDE.Nebula.Status";

/// Object path for `dev.mackes.MDE.Gluster.Status` (mirror of
/// `mackesd_core::ipc::gluster::GLUSTER_STATUS_OBJECT_PATH`).
pub const GLUSTER_STATUS_OBJECT_PATH: &str = "/dev/mackes/MDE/Gluster/Status";

/// Interface name for `dev.mackes.MDE.Gluster.Status`.
pub const GLUSTER_STATUS_INTERFACE: &str = "dev.mackes.MDE.Gluster.Status";

/// Errors a mesh-backend call can surface. `Unavailable` is the
/// common case (mackesd not running); `Decode` only fires when
/// the daemon's JSON shape drifts past what this client parses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshError {
    /// mackesd isn't on the session bus, or the call timed out
    /// before a reply arrived.
    Unavailable(String),
    /// Daemon replied but the JSON didn't deserialize.
    Decode(String),
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(s) => write!(f, "mesh unavailable: {s}"),
            Self::Decode(s) => write!(f, "mesh decode failed: {s}"),
        }
    }
}

impl std::error::Error for MeshError {}

// ----- wire types (mirror mackesd's IPC structs) ----------------

/// Nebula `Status()` snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NebulaStatus {
    pub is_lighthouse: bool,
    pub ca_epoch: i64,
    pub peer_count: usize,
    pub mesh_id: String,
    pub active_transport: String,
}

/// Nebula `ListPeers()` row.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NebulaPeer {
    pub node_id: String,
    pub name: String,
    pub overlay_ip: String,
    pub fingerprint: String,
    pub cert_epoch: i64,
    pub cert_expires_at: i64,
    /// "online" / "idle" / "offline"
    pub reachable: String,
}

/// Nebula `SelfNode()` snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NebulaSelfNode {
    pub node_id: String,
    pub host: String,
    pub role: String,
    pub cert_epoch: i64,
    pub cert_expires_at: i64,
    pub overlay_ip: String,
    pub mesh_id: String,
}

/// Gluster `Status()` snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GlusterStatus {
    pub volume_name: String,
    pub peers_count: usize,
    pub bricks_count: usize,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub heal_pending_count: usize,
    pub conflict_count: usize,
    pub volume_online: bool,
}

/// Gluster `ListPeers()` row.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GlusterPeer {
    pub uuid: String,
    pub host: String,
    pub state: String,
    pub is_self: bool,
    pub brick_free_bytes: u64,
}

/// Gluster `MountStatus()` snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GlusterMount {
    pub is_mounted: bool,
    pub mount_point: String,
    pub since_unix_s: u64,
}

/// Gluster `HealStatus()` snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GlusterHeal {
    pub pending_count: usize,
    pub in_progress_count: usize,
    pub split_brain_count: usize,
}

// ----- merged shape the UI binds against -----------------------

/// One row in the mesh-first peer list — merges Nebula's
/// reachability snapshot with Gluster's brick free-space + peer
/// state when both surfaces know about the peer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MeshPeer {
    /// Stable peer node-id (Nebula's `node_id`).
    pub node_id: String,
    /// Display name (Nebula's `name`, fallback hostname).
    pub host: String,
    /// 10.42.x.x — Nebula overlay IP.
    pub overlay_ip: String,
    /// "online" / "idle" / "offline" — from Nebula.
    pub reachable: String,
    /// Last cert epoch the lighthouse signed for this peer.
    pub cert_epoch: i64,
    /// Gluster's connection-state string. Empty when this peer
    /// isn't in the trusted storage pool yet.
    pub gluster_state: String,
    /// Free bytes on the peer's brick. 0 when unknown.
    pub brick_free_bytes: u64,
}

// ----- backend client ------------------------------------------

/// Cheap-to-construct mesh-backend client. Connection + tokio
/// runtime are opened once at start-up; each call blocks the
/// caller until the daemon replies (with a per-call timeout so
/// the UI thread never freezes for the dbus-defaults 25 s).
pub struct MeshBackend {
    rt: Runtime,
    connection: Connection,
    /// Per-call timeout — keeps the GUI thread snappy when
    /// mackesd is busy.
    call_timeout: Duration,
}

impl MeshBackend {
    /// Default connect timeout.
    pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_millis(800);
    /// Default per-method call timeout.
    pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_millis(750);

    /// Connect to the session bus + verify mackesd is reachable.
    /// Identical handshake to `DBusBackend::connect_with_timeout`
    /// so the failure mode matches what existing callers expect.
    pub fn connect_with_timeout(timeout: Duration) -> Result<Self, MeshError> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| MeshError::Unavailable(format!("tokio runtime: {e}")))?;
        let connection = rt.block_on(async {
            tokio::time::timeout(timeout, Connection::session())
                .await
                .map_err(|_| MeshError::Unavailable("session bus: timeout".into()))?
                .map_err(|e| MeshError::Unavailable(format!("session bus: {e}")))
        })?;
        let alive: bool = rt.block_on(async {
            let dbus_proxy = Proxy::new(
                &connection,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
            )
            .await
            .map_err(|e| MeshError::Unavailable(format!("dbus proxy: {e}")))?;
            let res: zbus::Result<bool> = tokio::time::timeout(timeout, async {
                dbus_proxy
                    .call_method("NameHasOwner", &(BUS_NAME,))
                    .await?
                    .body()
                    .deserialize::<bool>()
            })
            .await
            .map_err(|_| zbus::Error::Failure("NameHasOwner timeout".into()))
            .and_then(|x| x);
            res.map_err(|e| MeshError::Unavailable(format!("NameHasOwner: {e}")))
        })?;
        if !alive {
            return Err(MeshError::Unavailable(format!(
                "{BUS_NAME} not on the session bus"
            )));
        }
        Ok(Self {
            rt,
            connection,
            call_timeout: Self::DEFAULT_CALL_TIMEOUT,
        })
    }

    /// Convenience — connect with the default timeout.
    pub fn connect() -> Result<Self, MeshError> {
        Self::connect_with_timeout(Self::DEFAULT_CONNECT_TIMEOUT)
    }

    /// Override the per-call timeout. Tests use this to keep the
    /// suite fast.
    #[must_use]
    pub fn with_call_timeout(mut self, t: Duration) -> Self {
        self.call_timeout = t;
        self
    }

    /// Live Nebula overlay status.
    pub fn nebula_status(&self) -> Result<NebulaStatus, MeshError> {
        let raw = self.call_string(
            NEBULA_STATUS_OBJECT_PATH,
            NEBULA_STATUS_INTERFACE,
            "Status",
        )?;
        parse_nebula_status(&raw)
            .ok_or_else(|| MeshError::Decode(format!("nebula_status: {raw}")))
    }

    /// Live Nebula peer roster.
    pub fn nebula_peers(&self) -> Result<Vec<NebulaPeer>, MeshError> {
        let raw = self.call_string(
            NEBULA_STATUS_OBJECT_PATH,
            NEBULA_STATUS_INTERFACE,
            "ListPeers",
        )?;
        parse_nebula_peers(&raw)
            .ok_or_else(|| MeshError::Decode(format!("nebula_peers: {raw}")))
    }

    /// Live Nebula self-node snapshot.
    pub fn nebula_self_node(&self) -> Result<NebulaSelfNode, MeshError> {
        let raw = self.call_string(
            NEBULA_STATUS_OBJECT_PATH,
            NEBULA_STATUS_INTERFACE,
            "SelfNode",
        )?;
        parse_nebula_self_node(&raw)
            .ok_or_else(|| MeshError::Decode(format!("nebula_self_node: {raw}")))
    }

    /// Live Gluster volume snapshot.
    pub fn gluster_status(&self) -> Result<GlusterStatus, MeshError> {
        let raw = self.call_string(
            GLUSTER_STATUS_OBJECT_PATH,
            GLUSTER_STATUS_INTERFACE,
            "Status",
        )?;
        parse_gluster_status(&raw)
            .ok_or_else(|| MeshError::Decode(format!("gluster_status: {raw}")))
    }

    /// Live Gluster peer list.
    pub fn gluster_peers(&self) -> Result<Vec<GlusterPeer>, MeshError> {
        let raw = self.call_string(
            GLUSTER_STATUS_OBJECT_PATH,
            GLUSTER_STATUS_INTERFACE,
            "ListPeers",
        )?;
        parse_gluster_peers(&raw)
            .ok_or_else(|| MeshError::Decode(format!("gluster_peers: {raw}")))
    }

    /// Live Gluster mount status.
    pub fn gluster_mount_status(&self) -> Result<GlusterMount, MeshError> {
        let raw = self.call_string(
            GLUSTER_STATUS_OBJECT_PATH,
            GLUSTER_STATUS_INTERFACE,
            "MountStatus",
        )?;
        parse_gluster_mount(&raw)
            .ok_or_else(|| MeshError::Decode(format!("gluster_mount: {raw}")))
    }

    /// Live Gluster heal status.
    pub fn gluster_heal_status(&self) -> Result<GlusterHeal, MeshError> {
        let raw = self.call_string(
            GLUSTER_STATUS_OBJECT_PATH,
            GLUSTER_STATUS_INTERFACE,
            "HealStatus",
        )?;
        parse_gluster_heal(&raw)
            .ok_or_else(|| MeshError::Decode(format!("gluster_heal: {raw}")))
    }

    /// Composite call — fetches Nebula + Gluster peer rows + merges
    /// them keyed on (case-insensitive) hostname OR overlay IP. The
    /// resulting list is what the sidebar renders one row per. The
    /// UI never has to know whether a peer is "Nebula-only" (signed
    /// but not yet in the trusted pool) or fully joined; the merge
    /// surfaces both cases as one row with empty gluster_state /
    /// brick_free_bytes when only the Nebula side knows about it.
    pub fn mesh_peers(&self) -> Result<Vec<MeshPeer>, MeshError> {
        let nebula = self.nebula_peers().unwrap_or_default();
        let gluster = self.gluster_peers().unwrap_or_default();
        Ok(merge_peers(nebula, gluster))
    }

    fn call_string(
        &self,
        path: &str,
        iface: &str,
        method: &str,
    ) -> Result<String, MeshError> {
        self.rt.block_on(async {
            let proxy = Proxy::new(&self.connection, BUS_NAME, path, iface)
                .await
                .map_err(|e| MeshError::Unavailable(format!("proxy {iface}: {e}")))?;
            let res: zbus::Result<String> = tokio::time::timeout(self.call_timeout, async {
                proxy
                    .call_method(method, &())
                    .await?
                    .body()
                    .deserialize::<String>()
            })
            .await
            .map_err(|_| zbus::Error::Failure(format!("{iface}.{method} timeout")))
            .and_then(|x| x);
            res.map_err(|e| MeshError::Unavailable(format!("{iface}.{method}: {e}")))
        })
    }
}

// ----- pure helpers --------------------------------------------

/// Parse the JSON returned by `dev.mackes.MDE.Nebula.Status::Status`.
#[must_use]
pub fn parse_nebula_status(raw: &str) -> Option<NebulaStatus> {
    serde_json::from_str(raw).ok()
}

/// Parse the JSON returned by `dev.mackes.MDE.Nebula.Status::ListPeers`.
#[must_use]
pub fn parse_nebula_peers(raw: &str) -> Option<Vec<NebulaPeer>> {
    serde_json::from_str(raw).ok()
}

/// Parse the JSON returned by `dev.mackes.MDE.Nebula.Status::SelfNode`.
#[must_use]
pub fn parse_nebula_self_node(raw: &str) -> Option<NebulaSelfNode> {
    serde_json::from_str(raw).ok()
}

/// Parse the JSON returned by `dev.mackes.MDE.Gluster.Status::Status`.
#[must_use]
pub fn parse_gluster_status(raw: &str) -> Option<GlusterStatus> {
    serde_json::from_str(raw).ok()
}

/// Parse the JSON returned by `dev.mackes.MDE.Gluster.Status::ListPeers`.
#[must_use]
pub fn parse_gluster_peers(raw: &str) -> Option<Vec<GlusterPeer>> {
    serde_json::from_str(raw).ok()
}

/// Parse the JSON returned by `dev.mackes.MDE.Gluster.Status::MountStatus`.
#[must_use]
pub fn parse_gluster_mount(raw: &str) -> Option<GlusterMount> {
    serde_json::from_str(raw).ok()
}

/// Parse the JSON returned by `dev.mackes.MDE.Gluster.Status::HealStatus`.
#[must_use]
pub fn parse_gluster_heal(raw: &str) -> Option<GlusterHeal> {
    serde_json::from_str(raw).ok()
}

/// Merge Nebula peer rows with Gluster peer rows into the
/// UI-facing `MeshPeer` shape. Matching is overlay-IP first
/// (when both sides know it); falls back to case-insensitive
/// hostname. Gluster-only peers (no Nebula signature) still
/// land in the output so the operator sees them.
#[must_use]
pub fn merge_peers(nebula: Vec<NebulaPeer>, gluster: Vec<GlusterPeer>) -> Vec<MeshPeer> {
    let mut by_ip: BTreeMap<String, &GlusterPeer> = BTreeMap::new();
    let mut by_host: BTreeMap<String, &GlusterPeer> = BTreeMap::new();
    for g in &gluster {
        // Gluster's host string is whatever was supplied to
        // `peer probe` — overlay IP on a NF-3.4-managed mesh,
        // hostname on hand-rolled setups. Index both ways.
        by_ip.insert(g.host.clone(), g);
        by_host.insert(g.host.to_ascii_lowercase(), g);
    }
    let mut consumed: std::collections::BTreeSet<String> = Default::default();
    let mut out: Vec<MeshPeer> = Vec::new();
    for n in nebula {
        let mut gluster_state = String::new();
        let mut brick_free_bytes = 0u64;
        let mut matched_key: Option<String> = None;
        if let Some(g) = by_ip.get(&n.overlay_ip) {
            gluster_state = g.state.clone();
            brick_free_bytes = g.brick_free_bytes;
            matched_key = Some(g.host.clone());
        } else if let Some(g) = by_host.get(&n.name.to_ascii_lowercase()) {
            gluster_state = g.state.clone();
            brick_free_bytes = g.brick_free_bytes;
            matched_key = Some(g.host.clone());
        }
        if let Some(k) = matched_key {
            consumed.insert(k);
        }
        out.push(MeshPeer {
            node_id: n.node_id,
            host: n.name,
            overlay_ip: n.overlay_ip,
            reachable: n.reachable,
            cert_epoch: n.cert_epoch,
            gluster_state,
            brick_free_bytes,
        });
    }
    // Gluster peers nobody signed yet (e.g., probed by IP but
    // not enrolled into Nebula). Surface them so the operator
    // sees the partial state.
    for g in gluster {
        if consumed.contains(&g.host) {
            continue;
        }
        if g.is_self {
            continue;
        }
        out.push(MeshPeer {
            node_id: String::new(),
            host: g.host.clone(),
            overlay_ip: String::new(),
            reachable: "offline".into(),
            cert_epoch: 0,
            gluster_state: g.state.clone(),
            brick_free_bytes: g.brick_free_bytes,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_nebula_status_decodes_real_shape() {
        let raw = r#"{
            "is_lighthouse": true,
            "ca_epoch": 3,
            "peer_count": 5,
            "mesh_id": "mesh-abc",
            "active_transport": "nebula_direct"
        }"#;
        let s = parse_nebula_status(raw).expect("parse");
        assert!(s.is_lighthouse);
        assert_eq!(s.ca_epoch, 3);
        assert_eq!(s.peer_count, 5);
        assert_eq!(s.mesh_id, "mesh-abc");
    }

    #[test]
    fn parse_nebula_status_returns_none_on_garbage() {
        assert!(parse_nebula_status("not json").is_none());
    }

    #[test]
    fn parse_nebula_peers_decodes_array() {
        let raw = r#"[
            {"node_id":"peer:pine","name":"pine","overlay_ip":"10.42.0.5","fingerprint":"abc","cert_epoch":3,"cert_expires_at":0,"reachable":"online"},
            {"node_id":"peer:birch","name":"birch","overlay_ip":"10.42.0.6","fingerprint":"def","cert_epoch":3,"cert_expires_at":0,"reachable":"offline"}
        ]"#;
        let rows = parse_nebula_peers(raw).expect("parse");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].overlay_ip, "10.42.0.5");
        assert_eq!(rows[1].reachable, "offline");
    }

    #[test]
    fn parse_nebula_self_node_decodes() {
        let raw = r#"{
            "node_id":"peer:anvil","host":"anvil","role":"host",
            "cert_epoch":3,"cert_expires_at":1234567890,
            "overlay_ip":"10.42.0.1","mesh_id":"mesh-abc"
        }"#;
        let s = parse_nebula_self_node(raw).expect("parse");
        assert_eq!(s.overlay_ip, "10.42.0.1");
        assert_eq!(s.role, "host");
    }

    #[test]
    fn parse_gluster_status_decodes_real_shape() {
        let raw = r#"{
            "volume_name":"mesh-home","peers_count":3,"bricks_count":3,
            "total_bytes":1000000,"used_bytes":400000,"free_bytes":600000,
            "heal_pending_count":2,"conflict_count":0,"volume_online":true
        }"#;
        let s = parse_gluster_status(raw).expect("parse");
        assert_eq!(s.volume_name, "mesh-home");
        assert_eq!(s.peers_count, 3);
        assert_eq!(s.heal_pending_count, 2);
        assert!(s.volume_online);
    }

    #[test]
    fn parse_gluster_peers_decodes_array() {
        let raw = r#"[
            {"uuid":"aaa","host":"10.42.0.5","state":"Peer in Cluster","is_self":false,"brick_free_bytes":500000},
            {"uuid":"bbb","host":"10.42.0.6","state":"Disconnected","is_self":false,"brick_free_bytes":0}
        ]"#;
        let rows = parse_gluster_peers(raw).expect("parse");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].brick_free_bytes, 500_000);
        assert_eq!(rows[1].state, "Disconnected");
    }

    #[test]
    fn parse_gluster_mount_decodes() {
        let raw = r#"{
            "is_mounted":true,
            "mount_point":"/home/op",
            "since_unix_s":1715000000
        }"#;
        let m = parse_gluster_mount(raw).expect("parse");
        assert!(m.is_mounted);
        assert_eq!(m.mount_point, "/home/op");
    }

    #[test]
    fn parse_gluster_heal_decodes() {
        let raw = r#"{
            "pending_count":4,
            "in_progress_count":1,
            "split_brain_count":0
        }"#;
        let h = parse_gluster_heal(raw).expect("parse");
        assert_eq!(h.pending_count, 4);
        assert_eq!(h.split_brain_count, 0);
    }

    #[test]
    fn merge_peers_matches_on_overlay_ip() {
        let n = vec![NebulaPeer {
            node_id: "peer:pine".into(),
            name: "pine".into(),
            overlay_ip: "10.42.0.5".into(),
            fingerprint: "f".into(),
            cert_epoch: 3,
            cert_expires_at: 0,
            reachable: "online".into(),
        }];
        let g = vec![GlusterPeer {
            uuid: "aaa".into(),
            host: "10.42.0.5".into(),
            state: "Peer in Cluster".into(),
            is_self: false,
            brick_free_bytes: 500_000,
        }];
        let merged = merge_peers(n, g);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].host, "pine");
        assert_eq!(merged[0].overlay_ip, "10.42.0.5");
        assert_eq!(merged[0].gluster_state, "Peer in Cluster");
        assert_eq!(merged[0].brick_free_bytes, 500_000);
    }

    #[test]
    fn merge_peers_matches_on_hostname_when_overlay_ip_misses() {
        let n = vec![NebulaPeer {
            node_id: "peer:Birch".into(),
            name: "Birch".into(),
            overlay_ip: "10.42.0.6".into(),
            fingerprint: "f".into(),
            cert_epoch: 3,
            cert_expires_at: 0,
            reachable: "idle".into(),
        }];
        // Gluster knows it by hostname (different from overlay IP).
        let g = vec![GlusterPeer {
            uuid: "bbb".into(),
            host: "birch".into(),
            state: "Disconnected".into(),
            is_self: false,
            brick_free_bytes: 0,
        }];
        let merged = merge_peers(n, g);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].gluster_state, "Disconnected");
    }

    #[test]
    fn merge_peers_surfaces_gluster_only_peers() {
        let n: Vec<NebulaPeer> = Vec::new();
        let g = vec![GlusterPeer {
            uuid: "ccc".into(),
            host: "10.42.0.7".into(),
            state: "Peer in Cluster".into(),
            is_self: false,
            brick_free_bytes: 100,
        }];
        let merged = merge_peers(n, g);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].host, "10.42.0.7");
        // No Nebula signature → reachable shows offline so the
        // operator sees the partial-enrollment state.
        assert_eq!(merged[0].reachable, "offline");
        assert!(merged[0].node_id.is_empty());
    }

    #[test]
    fn merge_peers_drops_self_from_gluster_only_path() {
        let n: Vec<NebulaPeer> = Vec::new();
        let g = vec![GlusterPeer {
            uuid: "self".into(),
            host: "localhost".into(),
            state: "Connected".into(),
            is_self: true,
            brick_free_bytes: 9999,
        }];
        let merged = merge_peers(n, g);
        // Self never duplicates the sidebar's "you" row.
        assert!(merged.is_empty());
    }

    #[test]
    fn merge_peers_handles_both_empty() {
        let merged = merge_peers(Vec::new(), Vec::new());
        assert!(merged.is_empty());
    }

    #[test]
    fn merge_peers_preserves_nebula_only_peer_with_empty_gluster_state() {
        let n = vec![NebulaPeer {
            node_id: "peer:oak".into(),
            name: "oak".into(),
            overlay_ip: "10.42.0.8".into(),
            fingerprint: "f".into(),
            cert_epoch: 3,
            cert_expires_at: 0,
            reachable: "online".into(),
        }];
        let g: Vec<GlusterPeer> = Vec::new();
        let merged = merge_peers(n, g);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].host, "oak");
        assert_eq!(merged[0].overlay_ip, "10.42.0.8");
        assert!(merged[0].gluster_state.is_empty());
        assert_eq!(merged[0].brick_free_bytes, 0);
    }

    #[test]
    fn mesh_error_display_carries_context() {
        let e = MeshError::Unavailable("session bus closed".into());
        assert!(format!("{e}").contains("session bus closed"));
        let e = MeshError::Decode("bad JSON".into());
        assert!(format!("{e}").contains("bad JSON"));
    }

    #[test]
    fn const_paths_mirror_mackesd() {
        assert_eq!(NEBULA_STATUS_INTERFACE, "dev.mackes.MDE.Nebula.Status");
        assert_eq!(NEBULA_STATUS_OBJECT_PATH, "/dev/mackes/MDE/Nebula/Status");
        assert_eq!(GLUSTER_STATUS_INTERFACE, "dev.mackes.MDE.Gluster.Status");
        assert_eq!(GLUSTER_STATUS_OBJECT_PATH, "/dev/mackes/MDE/Gluster/Status");
        assert_eq!(BUS_NAME, "org.mackes.mackesd");
    }
}
