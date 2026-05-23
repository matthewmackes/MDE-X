//! Clock + date pill applet — top-bar-center slot.
//!
//! Phase E1.2.1: smallest of the Phase E1 applets.
//! v4.0.1 BUG-14: switched from a single-line `YYYY-MM-DD HH:MM`
//! string to the Win10 two-line stack — `H:MM AM/PM` on top,
//! `M/D/YYYY` on bottom, joined by `\n`. The panel renders these as
//! a two-line column in the clock zone.

#![forbid(unsafe_code)]

use mde_applet_api::{AppletId, AppletSlot, HostMessage};

/// Build the canonical manifest the panel host picks up
/// from `/usr/share/mde/applets/clock.json`.
#[must_use]
pub fn manifest() -> mde_applet_api::AppletManifest {
    mde_applet_api::AppletManifest {
        id: AppletId::from_static("clock"),
        binary: "mde-applet-clock".into(),
        slot: AppletSlot::TopBarCenter,
        summary: "Clock + date pill".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// Format a Unix-epoch-seconds timestamp as a Win10-style two-line
/// stack — `"H:MM AM/PM\nM/D/YYYY"` (local-time-as-UTC, no TZ
/// adjustment in this pure helper — the host process applies any TZ
/// shift before calling). Returns `"--:--"` for non-positive
/// timestamps; used by the applet's loading state.
///
/// v4.0.1 BUG-14: replaces the prior single-line
/// `YYYY-MM-DD HH:MM` format. Panel renderers split on `\n` to draw
/// the two stacked text lines.
#[must_use]
pub fn format_clock(secs: i64) -> String {
    if secs <= 0 {
        return "--:--".to_string();
    }
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let h24 = rem / 3600;
    let m = (rem % 3600) / 60;
    let (y, mo, d) = days_to_ymd(days);
    let (h12, ampm) = to_12h(h24);
    format!("{h12}:{m:02} {ampm}\n{mo}/{d}/{y:04}")
}

/// Convert a 24-hour hour (`0..=23`) to (12-hour-hour, "AM"/"PM").
/// Midnight (0) → (12, AM); noon (12) → (12, PM); 13 → (1, PM); etc.
#[must_use]
pub fn to_12h(h24: i64) -> (i64, &'static str) {
    let h = h24.rem_euclid(24);
    let ampm = if h < 12 { "AM" } else { "PM" };
    let h12 = match h % 12 {
        0 => 12,
        n => n,
    };
    (h12, ampm)
}

/// Howard Hinnant civil-from-days. Shared with the run-
/// history + mesh-history panels in mde-workbench; copied
/// here so this applet stays a standalone binary with
/// minimal deps.
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if mo <= 2 { y + 1 } else { y };
    (year as i32, mo as u32, d as u32)
}

/// Decide whether a host-pushed message means the applet
/// should re-render its view. Accent changes + visibility
/// flips trigger a render; Shutdown short-circuits to false
/// (the caller is expected to flush + exit).
#[must_use]
pub fn handle_host(msg: &HostMessage) -> bool {
    !matches!(msg, HostMessage::Shutdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_lands_in_top_bar_center_slot() {
        let m = manifest();
        assert_eq!(m.id.as_str(), "clock");
        assert_eq!(m.slot, AppletSlot::TopBarCenter);
        assert_eq!(m.binary, "mde-applet-clock");
        assert!(!m.summary.is_empty());
    }

    #[test]
    fn format_clock_renders_known_timestamps() {
        // 1_715_000_000 secs since epoch -> 2024-05-06 12:53 UTC.
        // v4.0.1 BUG-14 — Win10 two-line layout: time on top,
        // date M/D/YYYY on bottom.
        let s = format_clock(1_715_000_000);
        assert_eq!(s, "12:53 PM\n5/6/2024", "got: {s}");
    }

    #[test]
    fn format_clock_dashes_non_positive_timestamps() {
        assert_eq!(format_clock(0), "--:--");
        assert_eq!(format_clock(-1), "--:--");
    }

    #[test]
    fn to_12h_midnight_noon_anchors() {
        assert_eq!(to_12h(0), (12, "AM"));
        assert_eq!(to_12h(11), (11, "AM"));
        assert_eq!(to_12h(12), (12, "PM"));
        assert_eq!(to_12h(13), (1, "PM"));
        assert_eq!(to_12h(23), (11, "PM"));
    }

    #[test]
    fn handle_host_short_circuits_shutdown() {
        assert!(!handle_host(&HostMessage::Shutdown));
        assert!(handle_host(&HostMessage::Accent {
            color: "#000".into()
        }));
        assert!(handle_host(&HostMessage::Visibility { active: true }));
    }

    #[test]
    fn days_to_ymd_anchor_dates() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        assert_eq!(days_to_ymd(19_723), (2024, 1, 1));
    }
}
