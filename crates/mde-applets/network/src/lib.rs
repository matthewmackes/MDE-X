//! NetworkManager status chip — top-bar-right applet.
//!
//! Phase E1.2.3: reads the connectivity column from `nmcli
//! -t -f STATE,CONNECTIVITY g` (general status) and renders
//! a one-line chip: `<glyph> <active-connection-name>` or
//! "Disconnected" when nothing is active.

#![forbid(unsafe_code)]

use mde_applet_api::{AppletId, AppletSlot, HostMessage};

#[must_use]
pub fn manifest() -> mde_applet_api::AppletManifest {
    mde_applet_api::AppletManifest {
        id: AppletId::from_static("network"),
        binary: "mde-applet-network".into(),
        slot: AppletSlot::TopBarRight,
        summary: "NetworkManager active-connection chip".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// One active connection row from `nmcli -t -f
/// NAME,TYPE,DEVICE,STATE connection show --active`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveConnection {
    pub name: String,
    pub kind: String,
}

/// Parse the first active wifi/ethernet connection out of
/// nmcli's colon-separated active-connection list.
/// Returns `None` when no connection is active.
#[must_use]
pub fn parse_active(raw: &str) -> Option<ActiveConnection> {
    for line in raw.lines() {
        let parts: Vec<&str> = line.splitn(4, ':').collect();
        if parts.len() < 4 {
            continue;
        }
        if parts[3] != "activated" {
            continue;
        }
        let kind = parts[1];
        if kind != "wifi" && kind != "802-3-ethernet" && kind != "ethernet" {
            continue;
        }
        return Some(ActiveConnection {
            name: parts[0].to_string(),
            kind: kind.to_string(),
        });
    }
    None
}

/// Glyph for a connection type. The host paints the actual
/// icon; the text is for fallback + accessibility.
#[must_use]
pub const fn type_glyph(kind: &str) -> &'static str {
    match kind.as_bytes() {
        b"wifi" => "\u{25EF}",                         // large circle = wifi-ish glyph
        b"802-3-ethernet" | b"ethernet" => "\u{2261}", // ≡ = ethernet
        _ => "?",
    }
}

/// Render the chip's display string. Disconnected →
/// "Disconnected".
#[must_use]
pub fn format_chip(conn: Option<&ActiveConnection>) -> String {
    match conn {
        Some(c) => format!("{} {}", type_glyph(&c.kind), c.name),
        None => "Disconnected".to_string(),
    }
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
        assert_eq!(m.id.as_str(), "network");
        assert_eq!(m.slot, AppletSlot::TopBarRight);
    }

    #[test]
    fn parse_active_extracts_first_wifi_row() {
        let raw = "home-wifi:wifi:wlan0:activated\nwired:ethernet:eno1:activated\n";
        let c = parse_active(raw).unwrap();
        assert_eq!(c.name, "home-wifi");
        assert_eq!(c.kind, "wifi");
    }

    #[test]
    fn parse_active_extracts_ethernet_when_no_wifi() {
        let raw = "wired:802-3-ethernet:eno1:activated\n";
        let c = parse_active(raw).unwrap();
        assert_eq!(c.kind, "802-3-ethernet");
    }

    #[test]
    fn parse_active_skips_inactive_rows() {
        let raw = "home-wifi:wifi:wlan0:deactivated\nwork-vpn:vpn:tun0:activated\n";
        // VPN doesn't count for the chip — both rows are
        // skipped.
        assert!(parse_active(raw).is_none());
    }

    #[test]
    fn parse_active_returns_none_on_empty() {
        assert!(parse_active("").is_none());
    }

    #[test]
    fn type_glyph_maps_wifi_and_ethernet() {
        assert_eq!(type_glyph("wifi"), "\u{25EF}");
        assert_eq!(type_glyph("ethernet"), "\u{2261}");
        assert_eq!(type_glyph("802-3-ethernet"), "\u{2261}");
        assert_eq!(type_glyph("vpn"), "?");
    }

    #[test]
    fn format_chip_disconnected_when_none() {
        assert_eq!(format_chip(None), "Disconnected");
    }

    #[test]
    fn format_chip_combines_glyph_and_name() {
        let c = ActiveConnection {
            name: "home-wifi".into(),
            kind: "wifi".into(),
        };
        let chip = format_chip(Some(&c));
        assert!(chip.contains("home-wifi"));
        assert!(chip.contains("\u{25EF}"));
    }

    #[test]
    fn handle_host_short_circuits_shutdown() {
        assert!(!handle_host(&HostMessage::Shutdown));
    }
}
