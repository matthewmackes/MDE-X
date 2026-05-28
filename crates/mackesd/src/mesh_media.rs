//! Mesh media-server discovery — Airsonic / Subsonic / Jellyfin.
//!
//! EPIC-SYNC-APP-CONFIG (Q26) — the native-Rust replacement for the
//! discovery half of the retired `mackes/mesh_media.py`. Per the
//! operator-delegated discovery decision (2026-05-28, "Make the
//! discovery call" → Option A), this ports the runtime behavior of
//! the Python module rather than the aspirational telemetry path its
//! `DeprecationWarning` gestured at (that replacement never landed).
//!
//! Discovery is **mesh-peer TCP-probe**: enumerate every peer's
//! Nebula overlay IP from the GFS-replicated `<qnm_root>/<peer>/
//! mackesd/nebula-bundle.json` files (the same source
//! [`crate::workers::gluster_worker::peer_probe_targets`] uses) and
//! probe the two well-known media ports on each. This is the
//! mesh-native core of the Python's discovery; it intentionally drops
//! the Python's supplementary mDNS `_subsonic._tcp`/`_jellyfin._tcp`
//! LAN browse — a media server reachable only over plain LAN mDNS has
//! no Nebula trust, so auto-configuring clients to point at it
//! conflicts with the §0 "Secure" mesh model. The mDNS-LAN path is
//! tracked as a deferred sub-task (EPIC-SYNC-APP-CONFIG.mdns-lan) for
//! a later session that wants the LAN-bleed behavior back.

use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

/// Minimal projection of `nebula-bundle.json` — we only need the
/// overlay IP to probe. Serde ignores the bundle's other fields
/// (cert PEMs, lighthouses, etc.), so this stays decoupled from
/// [`crate::ca::bundle::NebulaBundle`]'s full shape.
#[derive(Deserialize)]
struct BundleOverlayIp {
    overlay_ip: String,
}

/// Airsonic / Subsonic media-server kind tag.
pub const KIND_AIRSONIC: &str = "airsonic";
/// Jellyfin media-server kind tag.
pub const KIND_JELLYFIN: &str = "jellyfin";

/// Default Airsonic/Subsonic port (matches the retired Python).
pub const AIRSONIC_PORT: u16 = 4040;
/// Default Jellyfin port (matches the retired Python).
pub const JELLYFIN_PORT: u16 = 8096;

/// Per-port TCP connect timeout. Matches the Python's 0.25 s probe so
/// a full sweep over an 8-peer mesh stays well under a second.
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// One media server reachable on the mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaServer {
    /// [`KIND_AIRSONIC`] or [`KIND_JELLYFIN`].
    pub kind: String,
    /// Hostname (peer node-id) for display.
    pub host: String,
    /// Resolved overlay IP.
    pub ip: String,
    /// Service port.
    pub port: u16,
}

impl MediaServer {
    /// `http://<ip>:<port>` — mesh-internal; Nebula provides the
    /// trust layer, so plain HTTP over the overlay is intentional
    /// (mirrors the Python `MediaServer.url`).
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}:{}", self.ip, self.port)
    }
}

/// Enumerate every peer's `(node_id, overlay_ip)` from the
/// GFS-replicated nebula bundles under `qnm_root`. Includes the local
/// peer's own bundle — a media server may run on this host too, which
/// the Python's `nebula_peer_ips()` likewise included. Missing root
/// or unreadable/!malformed bundles are skipped (best-effort).
#[must_use]
pub fn peer_overlay_ips(qnm_root: &Path) -> Vec<(String, String)> {
    let entries = match std::fs::read_dir(qnm_root) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let Some(node_id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let bundle_path = entry.path().join("mackesd").join("nebula-bundle.json");
        let Ok(bytes) = std::fs::read(&bundle_path) else {
            continue;
        };
        let Ok(bundle) = serde_json::from_slice::<BundleOverlayIp>(&bytes) else {
            continue;
        };
        if !bundle.overlay_ip.is_empty() {
            out.push((node_id, bundle.overlay_ip));
        }
    }
    out.sort();
    out
}

/// One TCP connect with a short timeout. `true` if the port accepts.
fn probe_port(ip: &str, port: u16) -> bool {
    let Ok(addr) = format!("{ip}:{port}").parse::<std::net::SocketAddr>() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok()
}

/// Build a [`MediaServer`] from a probe hit. Pure helper (no I/O) so
/// the kind→port mapping is unit-testable.
#[must_use]
pub fn server_from_probe(kind: &str, host: &str, ip: &str, port: u16) -> MediaServer {
    MediaServer {
        kind: kind.to_owned(),
        host: host.to_owned(),
        ip: ip.to_owned(),
        port,
    }
}

/// Probe every `(host, ip)` peer for the two media ports, returning a
/// [`MediaServer`] per open port. The injected `probe` closure is the
/// seam tests use to avoid real sockets; [`discover`] passes the live
/// [`probe_port`].
fn scan_probe<F>(peers: &[(String, String)], mut probe: F) -> Vec<MediaServer>
where
    F: FnMut(&str, u16) -> bool,
{
    let mut found = Vec::new();
    for (host, ip) in peers {
        if probe(ip, AIRSONIC_PORT) {
            found.push(server_from_probe(KIND_AIRSONIC, host, ip, AIRSONIC_PORT));
        }
        if probe(ip, JELLYFIN_PORT) {
            found.push(server_from_probe(KIND_JELLYFIN, host, ip, JELLYFIN_PORT));
        }
    }
    found
}

/// Dedupe on `(kind, ip, port)`, preserving first-seen order. Pure
/// helper so the dedup contract is unit-testable.
#[must_use]
pub fn dedupe(servers: Vec<MediaServer>) -> Vec<MediaServer> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for s in servers {
        let key = (s.kind.clone(), s.ip.clone(), s.port);
        if seen.insert(key) {
            out.push(s);
        }
    }
    out
}

/// Discover every mesh media server: enumerate peer overlay IPs from
/// `qnm_root` and TCP-probe each for the Airsonic + Jellyfin ports.
/// Best-effort — an empty/missing `qnm_root` yields an empty list
/// (clients keep their existing config). Result is deduped on
/// `(kind, ip, port)`.
#[must_use]
pub fn discover(qnm_root: &Path) -> Vec<MediaServer> {
    let peers = peer_overlay_ips(qnm_root);
    dedupe(scan_probe(&peers, probe_port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_is_http_over_overlay() {
        let s = server_from_probe(KIND_AIRSONIC, "peer-a", "10.42.0.5", AIRSONIC_PORT);
        assert_eq!(s.url(), "http://10.42.0.5:4040");
    }

    #[test]
    fn scan_probe_emits_one_server_per_open_port() {
        let peers = vec![
            ("peer-a".to_string(), "10.42.0.5".to_string()),
            ("peer-b".to_string(), "10.42.0.6".to_string()),
        ];
        // peer-a runs Airsonic; peer-b runs Jellyfin.
        let probe = |ip: &str, port: u16| match (ip, port) {
            ("10.42.0.5", AIRSONIC_PORT) => true,
            ("10.42.0.6", JELLYFIN_PORT) => true,
            _ => false,
        };
        let found = scan_probe(&peers, probe);
        assert_eq!(found.len(), 2);
        assert!(found.contains(&server_from_probe(
            KIND_AIRSONIC,
            "peer-a",
            "10.42.0.5",
            AIRSONIC_PORT
        )));
        assert!(found.contains(&server_from_probe(
            KIND_JELLYFIN,
            "peer-b",
            "10.42.0.6",
            JELLYFIN_PORT
        )));
    }

    #[test]
    fn scan_probe_emits_both_kinds_for_a_dual_host() {
        let peers = vec![("multi".to_string(), "10.42.0.9".to_string())];
        let found = scan_probe(&peers, |_ip, _port| true);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].kind, KIND_AIRSONIC);
        assert_eq!(found[1].kind, KIND_JELLYFIN);
    }

    #[test]
    fn scan_probe_skips_closed_ports() {
        let peers = vec![("dark".to_string(), "10.42.0.1".to_string())];
        let found = scan_probe(&peers, |_ip, _port| false);
        assert!(found.is_empty());
    }

    #[test]
    fn dedupe_collapses_same_kind_ip_port() {
        let servers = vec![
            server_from_probe(KIND_AIRSONIC, "a", "10.42.0.5", 4040),
            server_from_probe(KIND_AIRSONIC, "a-dup", "10.42.0.5", 4040),
            server_from_probe(KIND_JELLYFIN, "a", "10.42.0.5", 8096),
        ];
        let out = dedupe(servers);
        assert_eq!(out.len(), 2);
        // First-seen wins (keeps host "a", drops "a-dup").
        assert_eq!(out[0].host, "a");
    }

    #[test]
    fn peer_overlay_ips_empty_for_missing_root() {
        let out = peer_overlay_ips(Path::new("/nonexistent/qnm/root/xyz"));
        assert!(out.is_empty());
    }

    #[test]
    fn peer_overlay_ips_reads_bundles() {
        let tmp = std::env::temp_dir().join(format!("mde-mediatest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        for (peer, ip) in [("peer-a", "10.42.0.5"), ("peer-b", "10.42.0.6")] {
            let dir = tmp.join(peer).join("mackesd");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("nebula-bundle.json"),
                format!(r#"{{"overlay_ip":"{ip}","node_id":"{peer}"}}"#),
            )
            .unwrap();
        }
        let out = peer_overlay_ips(&tmp);
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(
            out,
            vec![
                ("peer-a".to_string(), "10.42.0.5".to_string()),
                ("peer-b".to_string(), "10.42.0.6".to_string()),
            ]
        );
    }
}
