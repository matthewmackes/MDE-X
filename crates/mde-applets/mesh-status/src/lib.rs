//! Mesh-status chip — top-bar-right applet that surfaces
//! peer-count + aggregate health.
//!
//! Phase E1.2.4 (original): polled `mded healthz` JSON for
//! aggregate state + peer count.
//!
//! NF-10.1 (v2.5, 2026-05-23): also consumes
//! `dev.mackes.MDE.Nebula.Status` for active-transport +
//! lighthouse-role data. The healthz path remains as a
//! back-compat fallback when the Nebula surface is
//! unreachable. Output:
//!   * glyph color follows active transport
//!     (green = nebula_direct, amber = nebula_lighthouse_relay,
//!      red = nebula_https443, grey = offline / unknown)
//!   * inset "lighthouse" pictogram when this peer is acting
//!     as a lighthouse (NF-10.4).
//! Click → opens the Mesh workbench panel pre-focused on
//! topology (binary main.rs handles the spawn).

#![forbid(unsafe_code)]

use mde_applet_api::{AppletId, AppletSlot, HostMessage};
use serde::Deserialize;

/// Minimal HealthReport shape the chip needs. Mirrors
/// `mackesd_core::health::HealthReport`'s JSON-line output.
#[derive(Debug, Clone, Deserialize)]
pub struct HealthReport {
    /// Aggregate state — one of `healthy` / `degraded` /
    /// `unreachable` / `unknown`. Defaults to `unknown`
    /// on missing-field so a fresh boot doesn't claim a
    /// state it doesn't have evidence for.
    #[serde(default = "default_unknown")]
    pub state: String,
    /// Number of peers contributing to the aggregate. `0`
    /// on a standalone box that hasn't enrolled yet.
    #[serde(default)]
    pub peer_count: u32,
}

fn default_unknown() -> String {
    "unknown".to_string()
}

impl Default for HealthReport {
    fn default() -> Self {
        Self {
            state: default_unknown(),
            peer_count: 0,
        }
    }
}

/// Build the static applet manifest the host registers at
/// startup. Slot = TopBarRight alongside the other status chips
/// (audio, network, clock).
#[must_use]
pub fn manifest() -> mde_applet_api::AppletManifest {
    mde_applet_api::AppletManifest {
        id: AppletId::from_static("mesh-status"),
        binary: "mde-applet-mesh-status".into(),
        slot: AppletSlot::TopBarRight,
        summary: "Mesh peer-count + aggregate health chip".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// Parse the JSON line `mded healthz` emits. Returns a
/// default `unknown` / 0 report on any failure so the chip
/// shows the unknown glyph rather than crashing.
#[must_use]
pub fn parse_healthz(raw: &str) -> HealthReport {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Glyph for a health state. Matches the inventory panel's
/// `health_glyph` mapping.
#[must_use]
pub const fn health_glyph(state: &str) -> &'static str {
    match state.as_bytes() {
        b"healthy" => "\u{25CF}",
        b"degraded" => "\u{25D0}",
        b"unreachable" => "\u{25CB}",
        _ => "?",
    }
}

/// Format the chip text — `<peer_count>`.
///
/// v4.0.1 BUG-13.a: leading Unicode glyph (`health_glyph(state)`,
/// e.g. `●` / `◐` / `○` / `?`) dropped from the chip text — the
/// panel composes a Material Symbols SVG icon (`PanelIcon::Mesh`)
/// before this text instead. `health_glyph` is kept exported for
/// tooltip / accessibility-text consumers. State-based color tinting
/// at the render side now lives on the SVG, not the unicode glyph.
#[must_use]
pub fn format_chip(report: &HealthReport) -> String {
    report.peer_count.to_string()
}

/// NF-10.1 (v2.5) — JSON shape of `mded.Nebula.Status`'s
/// `Status()` reply. Mirrors `mackesd_core::ipc::nebula::
/// StatusSnapshot`; defined inline so the applet doesn't
/// take a dep on mackesd-core.
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct NebulaStatusSnapshot {
    /// True when the local peer is acting as a lighthouse.
    /// Renders the lighthouse pictogram badge (NF-10.4).
    #[serde(default)]
    pub is_lighthouse: bool,
    /// Active CA epoch (informational only at the chip
    /// level; the workbench panel uses it).
    #[serde(default)]
    pub ca_epoch: i64,
    /// Peer count excluding self.
    #[serde(default)]
    pub peer_count: usize,
    /// Mesh-id.
    #[serde(default)]
    pub mesh_id: String,
    /// Active transport name. One of
    /// `"nebula_direct"` / `"nebula_lighthouse_relay"` /
    /// `"nebula_https443"` / `"kdc_tls"` / `"offline"`.
    #[serde(default)]
    pub active_transport: String,
}

/// NF-10.1 — parse the JSON `Status()` reply. Returns
/// `NebulaStatusSnapshot::default()` on garbage so the chip
/// shows the offline glyph rather than crashing.
#[must_use]
pub fn parse_nebula_status(raw: &str) -> NebulaStatusSnapshot {
    serde_json::from_str(raw).unwrap_or_default()
}

/// NF-10.1 — colour key for the chip glyph, keyed on the
/// Nebula active transport. Locked per the design doc:
/// green = direct UDP healthy, amber = lighthouse relay,
/// red = TCP/443 fallback, grey = offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NebulaTransportColor {
    /// Healthy direct UDP.
    Green,
    /// Routing via the lighthouse relay.
    Amber,
    /// Covert TCP/443 fallback.
    Red,
    /// Offline / unknown / not yet enrolled.
    Grey,
}

impl NebulaTransportColor {
    /// Map an `active_transport` string to a colour.
    #[must_use]
    pub fn from_transport(active: &str) -> Self {
        match active {
            "nebula_direct" => Self::Green,
            "nebula_lighthouse_relay" => Self::Amber,
            "nebula_https443" => Self::Red,
            // kdc_tls is a non-mesh path; treat as healthy
            // green from a fabric-status POV.
            "kdc_tls" => Self::Green,
            _ => Self::Grey,
        }
    }

    /// Hex string for the panel renderer. Material palette
    /// `--mde-status-*` aligned.
    #[must_use]
    pub const fn hex(self) -> &'static str {
        match self {
            Self::Green => "#1ac782",
            Self::Amber => "#f1c21b",
            Self::Red => "#da1e28",
            Self::Grey => "#8d8d8d",
        }
    }
}

/// NF-10.1 — hover-tooltip body. Peer count + active
/// transport name + lighthouse role when applicable.
#[must_use]
pub fn format_tooltip(snap: &NebulaStatusSnapshot) -> String {
    let role = if snap.is_lighthouse {
        " · lighthouse"
    } else {
        ""
    };
    let mesh = if snap.mesh_id.is_empty() {
        "no mesh".to_string()
    } else {
        format!("mesh {}", snap.mesh_id)
    };
    let transport = if snap.active_transport.is_empty() {
        "offline".to_string()
    } else {
        snap.active_transport.clone()
    };
    format!(
        "{mesh} · {peers} peers · {transport}{role}",
        peers = snap.peer_count,
    )
}

/// NF-10.4 — true when the chip should render the
/// lighthouse pictogram inset over the base health glyph.
#[must_use]
pub const fn show_lighthouse_badge(snap: &NebulaStatusSnapshot) -> bool {
    snap.is_lighthouse
}

/// Process a host control message and return `true` when the
/// applet should keep running. Only [`HostMessage::Shutdown`]
/// stops the event loop; every other variant is a host-side
/// hint the renderer reacts to elsewhere.
#[must_use]
pub fn handle_host(msg: &HostMessage) -> bool {
    !matches!(msg, HostMessage::Shutdown)
}

// ─────────────────────────────────────────────────────────
// MESHFS-12.1 — LizardFS `mesh-storage` status surface.
//
// Sourced from `mackesd meshfs-status --json` (the report
// `meshfs_worker::meshfs_status_report` emits, shipped in
// MESHFS-13.1). The applet renders the at-a-glance master +
// fleet-health indicator; the detailed per-peer table lives
// in the Workbench "Mesh Storage" panel (MESHFS-13.1).
// ─────────────────────────────────────────────────────────

/// One chunkserver row from the meshfs status report.
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct MeshFsPeer {
    /// Overlay address of the chunkserver.
    #[serde(default)]
    pub addr: String,
    /// Bytes stored on this chunkserver.
    #[serde(default)]
    pub used_bytes: u64,
    /// Bytes still free on this chunkserver.
    #[serde(default)]
    pub avail_bytes: u64,
}

/// JSON shape of `mackesd meshfs-status --json`. Mirrors
/// `meshfs_worker::MeshFsStatusReport`; defined inline so the
/// applet keeps its zero-dep-on-mackesd-core stance.
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct MeshFsStatus {
    /// Whether the active LizardFS master answered on the VIP.
    #[serde(default)]
    pub master_reachable: bool,
    /// Online chunkservers the master currently lists.
    #[serde(default)]
    pub peers: Vec<MeshFsPeer>,
    /// Replication goal (= converged peer count).
    #[serde(default)]
    pub goal: usize,
    /// Hard quota cap in bytes (absent when no CS data).
    #[serde(default)]
    pub quota_cap_bytes: Option<u64>,
    /// Overlay addr of the chunkserver with the least free space.
    #[serde(default)]
    pub limiting_peer_addr: Option<String>,
}

/// Parse `mackesd meshfs-status --json`. Returns a default
/// (master-down, no peers) status on any failure so the chip
/// shows the offline indicator rather than crashing.
#[must_use]
pub fn parse_meshfs_status(raw: &str) -> MeshFsStatus {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Fleet-level meshfs health, derived from the status report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshFsHealth {
    /// Master reachable + every replica present (`peers >= goal`).
    InSync,
    /// Master reachable but under-replicated (`peers < goal`) —
    /// chunks are re-replicating to restore the goal.
    Healing,
    /// The active master is unreachable on the VIP — the mesh
    /// FS is read-only (offline writes stage locally).
    MasterDown,
}

/// Classify the fleet meshfs health. `goal == 0` (no report /
/// pre-enrollment) with a reachable master counts as in-sync
/// (nothing to heal yet).
#[must_use]
pub fn classify_meshfs(status: &MeshFsStatus) -> MeshFsHealth {
    if !status.master_reachable {
        return MeshFsHealth::MasterDown;
    }
    if status.goal > 0 && status.peers.len() < status.goal {
        return MeshFsHealth::Healing;
    }
    MeshFsHealth::InSync
}

/// Colour for the meshfs health indicator, reusing the locked
/// status palette: green = in-sync, amber = healing, red =
/// master-down.
#[must_use]
pub const fn meshfs_color(health: MeshFsHealth) -> NebulaTransportColor {
    match health {
        MeshFsHealth::InSync => NebulaTransportColor::Green,
        MeshFsHealth::Healing => NebulaTransportColor::Amber,
        MeshFsHealth::MasterDown => NebulaTransportColor::Red,
    }
}

/// Human bytes (powers of 1024, one decimal past KiB).
#[must_use]
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

/// Multi-line meshfs summary for the applet tooltip / `--meshfs`
/// output: an active-master indicator + fleet health header,
/// then one `online` line per chunkserver with its usage.
#[must_use]
pub fn format_meshfs_summary(status: &MeshFsStatus) -> String {
    let health = classify_meshfs(status);
    let header = match health {
        MeshFsHealth::MasterDown => "mesh-storage: master DOWN (read-only)".to_string(),
        MeshFsHealth::Healing => format!(
            "mesh-storage: master up · healing ({}/{} replicas)",
            status.peers.len(),
            status.goal,
        ),
        MeshFsHealth::InSync => format!(
            "mesh-storage: master up · in-sync ({} peers)",
            status.peers.len(),
        ),
    };
    let mut out = header;
    for p in &status.peers {
        out.push_str(&format!(
            "\n  {}  ·  {} used / {} free  ·  online",
            p.addr,
            human_bytes(p.used_bytes),
            human_bytes(p.avail_bytes),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_lands_in_top_bar_right_slot() {
        let m = manifest();
        assert_eq!(m.id.as_str(), "mesh-status");
        assert_eq!(m.slot, AppletSlot::TopBarRight);
    }

    #[test]
    fn parse_healthz_extracts_state_and_peer_count() {
        let raw = r#"{"state": "healthy", "peer_count": 5}"#;
        let r = parse_healthz(raw);
        assert_eq!(r.state, "healthy");
        assert_eq!(r.peer_count, 5);
    }

    #[test]
    fn parse_healthz_defaults_to_unknown_on_garbage() {
        let r = parse_healthz("not json");
        assert_eq!(r.state, "unknown");
        assert_eq!(r.peer_count, 0);
    }

    #[test]
    fn parse_healthz_defaults_to_unknown_when_state_missing() {
        let r = parse_healthz(r#"{"peer_count": 3}"#);
        assert_eq!(r.state, "unknown");
        assert_eq!(r.peer_count, 3);
    }

    #[test]
    fn health_glyph_maps_canonical_states() {
        assert_eq!(health_glyph("healthy"), "\u{25CF}");
        assert_eq!(health_glyph("degraded"), "\u{25D0}");
        assert_eq!(health_glyph("unreachable"), "\u{25CB}");
        assert_eq!(health_glyph("unknown"), "?");
        assert_eq!(health_glyph("anything-else"), "?");
    }

    #[test]
    fn format_chip_renders_count_only() {
        // v4.0.1 BUG-13.a — leading Unicode glyph dropped.
        let r = HealthReport {
            state: "healthy".into(),
            peer_count: 7,
        };
        let chip = format_chip(&r);
        assert_eq!(chip, "7");
        assert!(!chip.contains("\u{25CF}"));
    }

    #[test]
    fn handle_host_short_circuits_shutdown() {
        assert!(!handle_host(&HostMessage::Shutdown));
        assert!(handle_host(&HostMessage::Visibility { active: true }));
    }

    // ─────────────────────────────────────────────────────
    // NF-10.1 / NF-10.4 — Nebula status surface
    // ─────────────────────────────────────────────────────

    #[test]
    fn parse_nebula_status_extracts_every_field() {
        let raw = r#"{"is_lighthouse": true, "ca_epoch": 3,
            "peer_count": 4, "mesh_id": "m1",
            "active_transport": "nebula_direct"}"#;
        let s = parse_nebula_status(raw);
        assert!(s.is_lighthouse);
        assert_eq!(s.ca_epoch, 3);
        assert_eq!(s.peer_count, 4);
        assert_eq!(s.mesh_id, "m1");
        assert_eq!(s.active_transport, "nebula_direct");
    }

    #[test]
    fn parse_nebula_status_defaults_on_garbage() {
        let s = parse_nebula_status("not json");
        assert_eq!(s, NebulaStatusSnapshot::default());
    }

    #[test]
    fn parse_nebula_status_tolerates_missing_fields() {
        let s = parse_nebula_status(r#"{"peer_count": 2}"#);
        assert_eq!(s.peer_count, 2);
        assert!(!s.is_lighthouse);
        assert_eq!(s.mesh_id, "");
    }

    #[test]
    fn transport_color_maps_locked_palette() {
        // Locked per the v2.5 design doc + NF-10.1 spec.
        assert_eq!(
            NebulaTransportColor::from_transport("nebula_direct"),
            NebulaTransportColor::Green,
        );
        assert_eq!(
            NebulaTransportColor::from_transport("nebula_lighthouse_relay"),
            NebulaTransportColor::Amber,
        );
        assert_eq!(
            NebulaTransportColor::from_transport("nebula_https443"),
            NebulaTransportColor::Red,
        );
        assert_eq!(
            NebulaTransportColor::from_transport("kdc_tls"),
            NebulaTransportColor::Green,
        );
        assert_eq!(
            NebulaTransportColor::from_transport("offline"),
            NebulaTransportColor::Grey,
        );
        assert_eq!(
            NebulaTransportColor::from_transport(""),
            NebulaTransportColor::Grey,
        );
    }

    #[test]
    fn transport_color_hex_is_material_status_palette() {
        // Hex codes must match the Material --mde-status-* CSS
        // tokens so the SVG renderer at the panel side
        // doesn't need a parallel mapping table.
        assert_eq!(NebulaTransportColor::Green.hex(), "#1ac782");
        assert_eq!(NebulaTransportColor::Amber.hex(), "#f1c21b");
        assert_eq!(NebulaTransportColor::Red.hex(), "#da1e28");
        assert_eq!(NebulaTransportColor::Grey.hex(), "#8d8d8d");
    }

    #[test]
    fn format_tooltip_shows_mesh_peer_transport_and_role() {
        let s = NebulaStatusSnapshot {
            is_lighthouse: true,
            ca_epoch: 1,
            peer_count: 7,
            mesh_id: "office".into(),
            active_transport: "nebula_direct".into(),
        };
        let t = format_tooltip(&s);
        assert!(t.contains("mesh office"));
        assert!(t.contains("7 peers"));
        assert!(t.contains("nebula_direct"));
        assert!(t.contains("lighthouse"));
    }

    #[test]
    fn format_tooltip_omits_lighthouse_when_peer_role() {
        let s = NebulaStatusSnapshot {
            is_lighthouse: false,
            peer_count: 3,
            mesh_id: "office".into(),
            active_transport: "nebula_direct".into(),
            ..Default::default()
        };
        let t = format_tooltip(&s);
        assert!(!t.contains("lighthouse"));
    }

    #[test]
    fn format_tooltip_handles_offline_no_mesh() {
        let s = NebulaStatusSnapshot::default();
        let t = format_tooltip(&s);
        assert!(t.contains("no mesh"));
        assert!(t.contains("offline"));
    }

    #[test]
    fn show_lighthouse_badge_only_when_role_host() {
        let host = NebulaStatusSnapshot {
            is_lighthouse: true,
            ..Default::default()
        };
        assert!(show_lighthouse_badge(&host));
        let peer = NebulaStatusSnapshot::default();
        assert!(!show_lighthouse_badge(&peer));
    }

    // ─────────────────────────────────────────────────────
    // MESHFS-12.1 — mesh-storage status surface
    // ─────────────────────────────────────────────────────

    #[test]
    fn parse_meshfs_status_extracts_report() {
        let raw = r#"{"master_reachable": true,
            "peers": [{"addr": "10.42.0.7", "used_bytes": 1024, "avail_bytes": 2048}],
            "goal": 1, "quota_cap_bytes": 4096, "limiting_peer_addr": "10.42.0.7"}"#;
        let s = parse_meshfs_status(raw);
        assert!(s.master_reachable);
        assert_eq!(s.peers.len(), 1);
        assert_eq!(s.peers[0].addr, "10.42.0.7");
        assert_eq!(s.goal, 1);
        assert_eq!(s.quota_cap_bytes, Some(4096));
    }

    #[test]
    fn parse_meshfs_status_defaults_to_master_down_on_garbage() {
        let s = parse_meshfs_status("not json");
        assert!(!s.master_reachable);
        assert!(s.peers.is_empty());
    }

    #[test]
    fn classify_meshfs_covers_three_states() {
        // Master down → MasterDown regardless of peers.
        let down = MeshFsStatus { master_reachable: false, goal: 3, ..Default::default() };
        assert_eq!(classify_meshfs(&down), MeshFsHealth::MasterDown);
        // Up + under-goal → Healing.
        let healing = MeshFsStatus {
            master_reachable: true,
            goal: 3,
            peers: vec![MeshFsPeer::default(), MeshFsPeer::default()],
            ..Default::default()
        };
        assert_eq!(classify_meshfs(&healing), MeshFsHealth::Healing);
        // Up + at-goal → InSync.
        let synced = MeshFsStatus {
            master_reachable: true,
            goal: 2,
            peers: vec![MeshFsPeer::default(), MeshFsPeer::default()],
            ..Default::default()
        };
        assert_eq!(classify_meshfs(&synced), MeshFsHealth::InSync);
        // goal=0 (pre-enrollment) + master up → InSync (nothing to heal).
        let fresh = MeshFsStatus { master_reachable: true, ..Default::default() };
        assert_eq!(classify_meshfs(&fresh), MeshFsHealth::InSync);
    }

    #[test]
    fn meshfs_color_maps_health_to_status_palette() {
        assert_eq!(meshfs_color(MeshFsHealth::InSync), NebulaTransportColor::Green);
        assert_eq!(meshfs_color(MeshFsHealth::Healing), NebulaTransportColor::Amber);
        assert_eq!(meshfs_color(MeshFsHealth::MasterDown), NebulaTransportColor::Red);
    }

    #[test]
    fn format_meshfs_summary_shows_master_indicator_and_peers() {
        let s = MeshFsStatus {
            master_reachable: true,
            goal: 1,
            peers: vec![MeshFsPeer {
                addr: "10.42.0.7".into(),
                used_bytes: 1024,
                avail_bytes: 2048,
            }],
            ..Default::default()
        };
        let out = format_meshfs_summary(&s);
        assert!(out.contains("master up"));
        assert!(out.contains("in-sync"));
        assert!(out.contains("10.42.0.7"));
        assert!(out.contains("online"));
        assert!(out.contains("1.0 KiB used"));
    }

    #[test]
    fn format_meshfs_summary_master_down_is_read_only() {
        let s = MeshFsStatus::default();
        let out = format_meshfs_summary(&s);
        assert!(out.contains("master DOWN"));
        assert!(out.contains("read-only"));
    }
}
