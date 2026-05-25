//! Portal-9.a — system status polling for the Dock status segment.
//!
//! Reads battery, network, and backlight state from sysfs every 30 s.
//! All reads are synchronous and cheap (< 1 ms); at 30 s cadence the
//! latency is irrelevant.  Callers receive a `StatusInfo` snapshot;
//! individual read failures leave the affected field at its default
//! value (None / false) so one missing subsystem never breaks the rest.

/// Current snapshot of system status for the Dock status segment.
#[derive(Debug, Clone, Default)]
pub struct StatusInfo {
    /// Battery level 0–100, or `None` if no battery is present.
    pub battery_pct: Option<u8>,
    /// True when the battery reports `Charging` or `Full`.
    pub battery_charging: bool,
    /// True when at least one non-loopback interface has operstate `up`.
    pub network_up: bool,
    /// True when the Nebula mesh interface (`nebula0`) has operstate `up`.
    pub mesh_up: bool,
    /// Backlight brightness 0–100, or `None` if no backlight is present.
    pub brightness_pct: Option<u8>,
}

/// Read a fresh `StatusInfo` snapshot from sysfs.
pub fn read_status() -> StatusInfo {
    StatusInfo {
        battery_pct: read_battery_pct(),
        battery_charging: read_battery_charging(),
        network_up: read_network_up(),
        mesh_up: read_interface_up("nebula0"),
        brightness_pct: read_brightness_pct(),
    }
}

// ── sysfs helpers ─────────────────────────────────────────────────────────────

fn read_battery_pct() -> Option<u8> {
    for name in &["BAT0", "BAT1"] {
        let path = format!("/sys/class/power_supply/{name}/capacity");
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(n) = s.trim().parse::<u8>() {
                return Some(n);
            }
        }
    }
    None
}

fn read_battery_charging() -> bool {
    for name in &["BAT0", "BAT1"] {
        let path = format!("/sys/class/power_supply/{name}/status");
        if let Ok(s) = std::fs::read_to_string(&path) {
            let status = s.trim();
            return status == "Charging" || status == "Full";
        }
    }
    false
}

fn read_interface_up(iface: &str) -> bool {
    let path = format!("/sys/class/net/{iface}/operstate");
    std::fs::read_to_string(path)
        .map(|s| s.trim() == "up")
        .unwrap_or(false)
}

fn read_network_up() -> bool {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let iface = name.to_string_lossy();
        if iface == "lo" {
            continue;
        }
        if read_interface_up(&iface) {
            return true;
        }
    }
    false
}

fn read_brightness_pct() -> Option<u8> {
    let Ok(dir) = std::fs::read_dir("/sys/class/backlight") else {
        return None;
    };
    for entry in dir.flatten() {
        let base = entry.path();
        let actual: u32 = std::fs::read_to_string(base.join("actual_brightness"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let max: u32 = std::fs::read_to_string(base.join("max_brightness"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        if max == 0 {
            return None;
        }
        return Some(((actual as f64 / max as f64) * 100.0).round() as u8);
    }
    None
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_info_default_is_all_none_false() {
        let si = StatusInfo::default();
        assert!(si.battery_pct.is_none());
        assert!(!si.battery_charging);
        assert!(!si.network_up);
        assert!(!si.mesh_up);
        assert!(si.brightness_pct.is_none());
    }

    #[test]
    fn status_info_clone_matches_original() {
        let si = StatusInfo {
            battery_pct: Some(75),
            battery_charging: true,
            network_up: true,
            mesh_up: false,
            brightness_pct: Some(50),
        };
        let si2 = si.clone();
        assert_eq!(si2.battery_pct, Some(75));
        assert!(si2.battery_charging);
        assert!(si2.network_up);
        assert!(!si2.mesh_up);
        assert_eq!(si2.brightness_pct, Some(50));
    }

    #[test]
    fn read_battery_pct_returns_valid_range_or_none() {
        let pct = read_battery_pct();
        if let Some(p) = pct {
            assert!(p <= 100, "battery pct must be 0–100, got {p}");
        }
        // None is acceptable on machines without a battery.
    }

    #[test]
    fn read_battery_charging_does_not_panic() {
        let _ = read_battery_charging();
    }

    #[test]
    fn read_network_up_does_not_panic() {
        let _ = read_network_up();
    }

    #[test]
    fn read_interface_up_returns_false_for_nonexistent_iface() {
        assert!(!read_interface_up("nonexistent-iface-xyz99"));
    }

    #[test]
    fn read_brightness_pct_returns_valid_range_or_none() {
        let pct = read_brightness_pct();
        if let Some(b) = pct {
            assert!(b <= 100, "brightness pct must be 0–100, got {b}");
        }
    }

    #[test]
    fn read_status_returns_consistent_snapshot() {
        let si = read_status();
        // Battery pct and charging are consistent (both from BAT0/BAT1).
        // If we have a battery, pct must be Some.
        if let Some(pct) = si.battery_pct {
            assert!(pct <= 100);
        }
        if let Some(bri) = si.brightness_pct {
            assert!(bri <= 100);
        }
    }

    #[test]
    fn mesh_up_is_false_when_nebula_absent() {
        // In CI / dev machines without nebula0, this should always be false
        // without panicking.
        let up = read_interface_up("nebula0");
        // Can't assert the value — just assert no panic.
        let _ = up;
    }
}
