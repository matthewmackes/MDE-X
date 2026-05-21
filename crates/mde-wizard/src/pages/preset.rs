//! Preset picker — choose one of the 4 shipped presets.
//!
//! Default is `hashbang` per the Mackes Shell memory lock.
//! Each preset writes a JSON manifest into
//! `mackes/presets/*.json` that birthright steps consume.

/// The four shipped presets. Order matches the v1.x picker.
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "hashbang",
        display_name: "#! · Hashbang",
        accent_hex: "#FF6B00",
        blurb: "CrunchBang nod — minimalist orange-on-black; default.",
    },
    Preset {
        id: "mackes",
        display_name: "Mackes",
        accent_hex: "#2B9AF3",
        blurb: "IBM-blue Carbon-system feel; safe for shared machines.",
    },
    Preset {
        id: "daylight",
        display_name: "Daylight",
        accent_hex: "#42BE65",
        blurb: "Bright high-contrast green; pairs well with light wallpaper.",
    },
    Preset {
        id: "vanilla",
        display_name: "Vanilla",
        accent_hex: "#A8A8A8",
        blurb: "No accent. Stock fedora look with MDE chrome.",
    },
];

/// One preset description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub id: &'static str,
    pub display_name: &'static str,
    pub accent_hex: &'static str,
    pub blurb: &'static str,
}

/// Default preset id — `hashbang`.
pub const DEFAULT_PRESET: &str = "hashbang";

/// Resolve a preset by id; falls back to the default.
#[must_use]
pub fn by_id(id: &str) -> &'static Preset {
    PRESETS
        .iter()
        .find(|p| p.id == id)
        .unwrap_or_else(|| by_id(DEFAULT_PRESET))
}

/// Is `id` a known preset?
#[must_use]
pub fn is_valid(id: &str) -> bool {
    PRESETS.iter().any(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_presets_ship() {
        assert_eq!(PRESETS.len(), 4);
    }

    #[test]
    fn ids_are_distinct() {
        let ids: std::collections::HashSet<_> = PRESETS.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn hashbang_is_default() {
        assert_eq!(DEFAULT_PRESET, "hashbang");
        assert!(is_valid(DEFAULT_PRESET));
    }

    #[test]
    fn ids_match_memory_lock() {
        let names: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
        assert_eq!(names, vec!["hashbang", "mackes", "daylight", "vanilla"]);
    }

    #[test]
    fn by_id_returns_correct_preset() {
        assert_eq!(by_id("daylight").accent_hex, "#42BE65");
        assert_eq!(by_id("mackes").display_name, "Mackes");
    }

    #[test]
    fn by_id_falls_back_to_default_for_unknown() {
        assert_eq!(by_id("not-a-preset").id, DEFAULT_PRESET);
    }

    #[test]
    fn is_valid_rejects_unknown() {
        assert!(!is_valid(""));
        assert!(!is_valid("xyz"));
    }

    #[test]
    fn every_preset_has_hex_accent() {
        for p in PRESETS {
            assert!(p.accent_hex.starts_with('#'));
            assert!(p.accent_hex.len() == 7 || p.accent_hex.len() == 9);
        }
    }

    #[test]
    fn every_preset_has_non_empty_blurb() {
        for p in PRESETS {
            assert!(!p.blurb.is_empty());
        }
    }
}
