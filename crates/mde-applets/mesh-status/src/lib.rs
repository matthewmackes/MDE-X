//! Mesh-status chip — top-bar-right applet that surfaces
//! peer-count + aggregate health from `mded healthz`.
//!
//! Phase E1.2.4: lightweight chip with a colour-coded
//! glyph (●=healthy / ◐=degraded / ○=unreachable / ?=
//! unknown) + the active peer count.

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
/// panel composes a Carbon SVG icon (`PanelIcon::Mesh`) before this
/// text instead. `health_glyph` is kept exported for tooltip /
/// accessibility-text consumers. State-based color tinting at the
/// render side now lives on the SVG, not the unicode glyph.
#[must_use]
pub fn format_chip(report: &HealthReport) -> String {
    report.peer_count.to_string()
}

#[must_use]
pub fn handle_host(msg: &HostMessage) -> bool {
    !matches!(msg, HostMessage::Shutdown)
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
}
