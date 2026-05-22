//! KDC2-2 discovery — UDP-broadcast announcements + mesh-shunt
//! synthetic-mDNS injection point.
//!
//! Stock KDE Connect uses UDP/1716 broadcasts on the local LAN
//! to announce a peer's identity. KDC2 keeps that exact behavior
//! for wire compatibility — phones discover MDE peers through
//! the upstream protocol — but layers a [`SyntheticAnnounce`]
//! injection point on top so peer A can tell peer B "phone X
//! exists, here's its identity envelope" through the MDE mesh
//! router, making X reachable from B without re-pairing.
//!
//! Networking + actual broadcast send/receive live in
//! `mde-kdc::discovery` (host integration, KDC2-3). This file
//! ships the **announce data model** + the synthetic-injection
//! seam.

use serde::{Deserialize, Serialize};

/// Identity announcement broadcast on UDP/1716 (or injected
/// through the mesh-shunt). Stays wire-compatible with the
/// upstream KDC identity packet's `body` shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Announce {
    /// Stable per-device identifier (KDE Connect UUID).
    pub device_id: String,
    /// Display name. MDE peers append `[mde]` (see
    /// [`crate::MDE_DEVICE_NAME_SUFFIX`]).
    pub device_name: String,
    /// Coarse device type — drives the row icon glyph in the
    /// receiver's UI.
    pub device_type: DeviceType,
    /// Protocol version this peer speaks. Stock KDC currently
    /// emits `7`; KDC2 matches.
    pub protocol_version: u32,
    /// Plugin types this peer accepts (`kdeconnect.clipboard`,
    /// `kdeconnect.notification`, etc.). Upstream calls this
    /// `incomingCapabilities`.
    pub incoming_capabilities: Vec<String>,
    /// Plugin types this peer emits. Upstream calls this
    /// `outgoingCapabilities`.
    pub outgoing_capabilities: Vec<String>,
}

/// KDC's coarse device-type enumeration. Stays in lock-step with
/// the legacy v13.0 `mackes-kdc::DeviceKind` for serde token
/// compatibility (`phone`, `tablet`, `desktop`, `unknown`) — the
/// v2.1 KDC2 lock keeps these tokens stable so paired phones
/// don't re-classify on the v2.0 → v2.1 upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    /// Android handset.
    Phone,
    /// Tablet (Android / iOS).
    Tablet,
    /// Linux desktop (MDE peer OR a stock-KDC desktop client).
    Desktop,
    /// Anything else.
    Unknown,
}

/// Mesh-shunt: peer A injects "I see phone X" so peer B finds X
/// without a direct broadcast from X. The injection point is the
/// seam where KDC2-4 wires the mesh router into the discovery
/// layer.
///
/// KDC2-2.1 ships the data model + signature placeholder; the
/// actual SyntheticAnnounce verification + drop-if-stale logic
/// lands with the KDC2-4 mesh-shunt work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntheticAnnounce {
    /// The relayed identity announcement (verbatim from the
    /// originating peer's broadcast).
    pub announce: Announce,
    /// Identity of the MDE peer that's relaying. Receivers use
    /// this to filter (e.g. discard relays from a peer we don't
    /// trust).
    pub relayed_by: String,
    /// Monotonic timestamp of the relay (ms since Unix epoch).
    /// Used to drop stale announces — a peer that hasn't been
    /// re-announced in N minutes is treated as gone.
    pub relayed_at_ms: i64,
}

impl SyntheticAnnounce {
    /// True when this synthetic announce is recent enough to act
    /// on. KDC2-4 sets the staleness window from a config knob;
    /// this default (90 s) matches upstream KDC's own broadcast
    /// cadence.
    #[must_use]
    pub fn is_fresh(&self, now_ms: i64) -> bool {
        now_ms.saturating_sub(self.relayed_at_ms) <= STALE_WINDOW_MS
    }
}

/// Staleness window (ms). Announce records older than this are
/// dropped from the registry on every `prune_stale` call.
/// Matches upstream KDE Connect's broadcast cadence — phones
/// re-announce every ~60 s, so a 90 s window covers the
/// expected jitter without holding ghosts.
pub const STALE_WINDOW_MS: i64 = 90_000;

/// KDC2-2.11 — in-memory registry the host's discovery layer
/// polls for unified real + synthetic announces.
///
/// The host's UDP/mDNS listener (KDC2-2.9/2.10) feeds real
/// announces via [`DiscoveryRegistry::inject_real`]; the mesh-
/// shunt worker (KDC2-4.3) feeds synthetic announces (relayed
/// from neighbors' `phones.json`) via [`inject_synthetic`].
/// Downstream consumers (`KdcHost::open` for outbound pairing
/// + the `mde-workbench` peer list) drain via
/// [`take_fresh`] on each tick.
///
/// Receivers can't distinguish real from synthetic — both
/// surface as `Announce` records — and shouldn't care: the
/// trust model (cert fingerprint pinning) is the same either
/// way.
#[derive(Debug, Default)]
pub struct DiscoveryRegistry {
    /// (relayer_id, relayed_at_ms, announce) — relayer_id is
    /// `"self"` for real broadcasts; mesh-shunt records carry
    /// the actual neighbor peer-id. Tuple instead of struct so
    /// the Vec stays cheap to drain.
    entries: Vec<RegistryEntry>,
}

/// Internal entry shape — kept small + non-public.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistryEntry {
    announce: Announce,
    relayer_id: String,
    received_at_ms: i64,
}

impl DiscoveryRegistry {
    /// Empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// How many announce records the registry is currently
    /// holding (including stale ones until the next prune).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no announces are queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Inject a real UDP/mDNS announce. `received_at_ms` is the
    /// wall-clock timestamp the listener observed the packet.
    pub fn inject_real(&mut self, announce: Announce, received_at_ms: i64) {
        self.upsert("self", announce, received_at_ms);
    }

    /// Inject a synthetic (mesh-shunted) announce. The mesh-
    /// shunt worker (KDC2-4.3) calls this for each phone in a
    /// neighbor's `phones.json`. `relayer_id` is the neighbor
    /// peer-id (so downstream can show "via peer-A" in the UI
    /// + audit log).
    pub fn inject_synthetic(&mut self, synthetic: SyntheticAnnounce) {
        self.upsert(
            &synthetic.relayed_by,
            synthetic.announce,
            synthetic.relayed_at_ms,
        );
    }

    fn upsert(&mut self, relayer_id: &str, announce: Announce, received_at_ms: i64) {
        // Replace any existing entry with the same device_id —
        // keeps the registry at one entry per device.
        self.entries.retain(|e| e.announce.device_id != announce.device_id);
        self.entries.push(RegistryEntry {
            announce,
            relayer_id: relayer_id.to_string(),
            received_at_ms,
        });
    }

    /// Drop entries older than `STALE_WINDOW_MS`. Returns how
    /// many were dropped. Cheap to call on every tick.
    pub fn prune_stale(&mut self, now_ms: i64) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|e| now_ms.saturating_sub(e.received_at_ms) <= STALE_WINDOW_MS);
        before - self.entries.len()
    }

    /// Return every fresh (non-stale) announce. Does NOT mutate
    /// the registry — the host calls `prune_stale` separately
    /// when it's safe to drop entries.
    #[must_use]
    pub fn take_fresh(&self, now_ms: i64) -> Vec<Announce> {
        self.entries
            .iter()
            .filter(|e| now_ms.saturating_sub(e.received_at_ms) <= STALE_WINDOW_MS)
            .map(|e| e.announce.clone())
            .collect()
    }

    /// Look up the relayer for a given device-id. `Some("self")`
    /// for real broadcasts; `Some(<neighbor-peer-id>)` for
    /// synthetic. `None` when the device-id isn't in the
    /// registry.
    #[must_use]
    pub fn relayer_for(&self, device_id: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.announce.device_id == device_id)
            .map(|e| e.relayer_id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announce_serializes_with_kdc_field_names() {
        // `deviceId`, `deviceName`, `incomingCapabilities`, etc. —
        // the upstream KDC identity packet uses camelCase. Our
        // serde rename_all is the wire lock.
        let a = Announce {
            device_id: "abc".to_string(),
            device_name: "lab-01 [mde]".to_string(),
            device_type: DeviceType::Desktop,
            protocol_version: 7,
            incoming_capabilities: vec!["kdeconnect.clipboard".into()],
            outgoing_capabilities: vec!["kdeconnect.notification".into()],
        };
        let s = serde_json::to_string(&a).unwrap();
        assert!(s.contains(r#""deviceId":"abc""#));
        assert!(s.contains(r#""deviceName":"lab-01 [mde]""#));
        assert!(s.contains(r#""incomingCapabilities""#));
        assert!(s.contains(r#""outgoingCapabilities""#));
    }

    #[test]
    fn device_type_serializes_snake_case() {
        // Matches legacy `mackes-kdc::DeviceKind` for token
        // stability across the v2.0 → v2.1 upgrade.
        assert_eq!(serde_json::to_string(&DeviceType::Phone).unwrap(), r#""phone""#);
        assert_eq!(serde_json::to_string(&DeviceType::Tablet).unwrap(), r#""tablet""#);
        assert_eq!(
            serde_json::to_string(&DeviceType::Desktop).unwrap(),
            r#""desktop""#,
        );
        assert_eq!(
            serde_json::to_string(&DeviceType::Unknown).unwrap(),
            r#""unknown""#,
        );
    }

    #[test]
    fn synthetic_announce_is_fresh_within_90s_window() {
        let s = SyntheticAnnounce {
            announce: Announce {
                device_id: "abc".to_string(),
                device_name: "phone".to_string(),
                device_type: DeviceType::Phone,
                protocol_version: 7,
                incoming_capabilities: vec![],
                outgoing_capabilities: vec![],
            },
            relayed_by: "peer-A".to_string(),
            relayed_at_ms: 1_000_000,
        };
        // 50s after relay — fresh.
        assert!(s.is_fresh(1_050_000));
        // 90s after relay — still fresh (boundary inclusive).
        assert!(s.is_fresh(1_090_000));
        // 91s after relay — stale.
        assert!(!s.is_fresh(1_091_000));
        // 200s after relay — definitely stale.
        assert!(!s.is_fresh(1_200_000));
    }

    #[test]
    fn synthetic_announce_round_trips_through_json() {
        let s = SyntheticAnnounce {
            announce: Announce {
                device_id: "abc".to_string(),
                device_name: "phone".to_string(),
                device_type: DeviceType::Phone,
                protocol_version: 7,
                incoming_capabilities: vec!["kdeconnect.clipboard".into()],
                outgoing_capabilities: vec!["kdeconnect.notification".into()],
            },
            relayed_by: "peer-A".to_string(),
            relayed_at_ms: 1_700_000_000_000,
        };
        let raw = serde_json::to_string(&s).unwrap();
        let back: SyntheticAnnounce = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, s);
    }

    // ─────────────────────────────────────────────────────────
    // KDC2-2.11 — DiscoveryRegistry
    // ─────────────────────────────────────────────────────────

    fn sample_announce(device_id: &str) -> Announce {
        Announce {
            device_id: device_id.to_string(),
            device_name: device_id.to_string(),
            device_type: DeviceType::Phone,
            protocol_version: 7,
            incoming_capabilities: vec![],
            outgoing_capabilities: vec![],
        }
    }

    #[test]
    fn registry_starts_empty() {
        let r = DiscoveryRegistry::new();
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn inject_real_marks_relayer_as_self() {
        let mut r = DiscoveryRegistry::new();
        r.inject_real(sample_announce("phone-A"), 1000);
        assert_eq!(r.relayer_for("phone-A"), Some("self"));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn inject_synthetic_records_neighbor_relayer() {
        let mut r = DiscoveryRegistry::new();
        let synth = SyntheticAnnounce {
            announce: sample_announce("phone-X"),
            relayed_by: "peer-A".to_string(),
            relayed_at_ms: 1000,
        };
        r.inject_synthetic(synth);
        assert_eq!(r.relayer_for("phone-X"), Some("peer-A"));
    }

    #[test]
    fn inject_replaces_existing_entry_for_same_device() {
        // Re-announce of the same device updates rather than
        // duplicates — keeps the registry at one entry per
        // device.
        let mut r = DiscoveryRegistry::new();
        r.inject_real(sample_announce("phone-A"), 1000);
        r.inject_real(sample_announce("phone-A"), 2000);
        assert_eq!(r.len(), 1, "second inject must replace, not duplicate");
    }

    #[test]
    fn take_fresh_filters_stale_entries() {
        let mut r = DiscoveryRegistry::new();
        // Fresh entry at t=1000.
        r.inject_real(sample_announce("phone-A"), 1000);
        // Stale entry at t=10 (now is 1000 + STALE_WINDOW_MS + 1).
        r.inject_real(sample_announce("phone-B"), 10);
        let now = 10 + STALE_WINDOW_MS + 1;
        let fresh = r.take_fresh(now);
        // phone-B's received_at (10) is older than the window
        // → filtered. phone-A's received_at (1000) is at the
        // edge of the window (now - 1000 = STALE + 1 - 990 =
        // STALE - 989, fresh).
        let ids: Vec<&str> = fresh.iter().map(|a| a.device_id.as_str()).collect();
        assert!(ids.contains(&"phone-A"));
        assert!(!ids.contains(&"phone-B"));
    }

    #[test]
    fn prune_stale_drops_old_entries() {
        let mut r = DiscoveryRegistry::new();
        r.inject_real(sample_announce("phone-A"), 1000);
        r.inject_real(sample_announce("phone-B"), 10);
        let now = 10 + STALE_WINDOW_MS + 1;
        let dropped = r.prune_stale(now);
        assert_eq!(dropped, 1);
        // phone-B is gone; phone-A remains.
        assert_eq!(r.len(), 1);
        assert_eq!(r.relayer_for("phone-A"), Some("self"));
        assert!(r.relayer_for("phone-B").is_none());
    }

    #[test]
    fn synthetic_replaces_prior_real_announce_for_same_device() {
        // Edge case: phone goes off-LAN; the mesh-shunt now
        // relays it from a neighbor. The registry must reflect
        // the new relayer (neighbor instead of "self").
        let mut r = DiscoveryRegistry::new();
        r.inject_real(sample_announce("phone-A"), 1000);
        assert_eq!(r.relayer_for("phone-A"), Some("self"));
        r.inject_synthetic(SyntheticAnnounce {
            announce: sample_announce("phone-A"),
            relayed_by: "peer-B".to_string(),
            relayed_at_ms: 2000,
        });
        assert_eq!(r.relayer_for("phone-A"), Some("peer-B"));
        assert_eq!(r.len(), 1);
    }
}
