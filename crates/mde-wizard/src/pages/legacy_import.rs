//! Legacy import — opt-in detection of XFCE/v1.x configs.
//!
//! On a v2.0.0 fresh install this is a no-op. On a v2.0.0 upgrade
//! from v1.x, `~/.config/mackes-shell/` + `~/.config/xfce4/`
//! exist and can be imported via `mde-migrate-from-1x` (Phase
//! 0.5, shipped). This page surfaces whether either path is
//! present + lets the user opt in.

use std::path::{Path, PathBuf};

/// Detection report for the page body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegacyDetection {
    pub mackes_shell_present: bool,
    pub xfce4_present: bool,
}

impl LegacyDetection {
    /// Probe the standard v1.x config locations.
    #[must_use]
    pub fn probe() -> Self {
        Self::probe_under(dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")))
    }

    /// Pure helper — probe with an explicit base dir. Used by
    /// tests + by callers that need to inspect a non-default
    /// XDG_CONFIG_HOME.
    #[must_use]
    pub fn probe_under(base: PathBuf) -> Self {
        Self {
            mackes_shell_present: base.join("mackes-shell").is_dir(),
            xfce4_present: base.join("xfce4").is_dir(),
        }
    }

    /// True when there's anything worth importing.
    #[must_use]
    pub fn has_anything(&self) -> bool {
        self.mackes_shell_present || self.xfce4_present
    }

    /// Description for the page body.
    #[must_use]
    pub fn summary(&self) -> String {
        match (self.mackes_shell_present, self.xfce4_present) {
            (false, false) => "No prior MDE / XFCE config detected — fresh install.".into(),
            (true, false) => "MDE 1.x config detected. Import will move it to ~/.config/mde/.".into(),
            (false, true) => "XFCE 4 config detected. Import will back it up to ~/.config/xfce4.v1x-backup.<ts>/.".into(),
            (true, true) => "Both MDE 1.x and XFCE 4 configs detected. Import will move + back up.".into(),
        }
    }
}

/// Resolve the `mde-migrate-from-1x` binary path; defaults to
/// `/usr/bin/mde-migrate-from-1x`.
#[must_use]
pub fn migrate_binary_path() -> &'static Path {
    Path::new("/usr/bin/mde-migrate-from-1x")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detection_finds_no_legacy_in_empty_dir() {
        let tmp = tempdir().unwrap();
        let d = LegacyDetection::probe_under(tmp.path().to_path_buf());
        assert!(!d.mackes_shell_present);
        assert!(!d.xfce4_present);
        assert!(!d.has_anything());
    }

    #[test]
    fn detection_finds_mackes_shell_dir() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("mackes-shell")).unwrap();
        let d = LegacyDetection::probe_under(tmp.path().to_path_buf());
        assert!(d.mackes_shell_present);
        assert!(d.has_anything());
    }

    #[test]
    fn detection_finds_xfce4_dir() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("xfce4")).unwrap();
        let d = LegacyDetection::probe_under(tmp.path().to_path_buf());
        assert!(d.xfce4_present);
    }

    #[test]
    fn detection_finds_both() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("mackes-shell")).unwrap();
        std::fs::create_dir(tmp.path().join("xfce4")).unwrap();
        let d = LegacyDetection::probe_under(tmp.path().to_path_buf());
        assert!(d.mackes_shell_present);
        assert!(d.xfce4_present);
        assert!(d.has_anything());
    }

    #[test]
    fn summary_distinguishes_every_combination() {
        let all_false = LegacyDetection::default();
        let only_mackes = LegacyDetection {
            mackes_shell_present: true,
            xfce4_present: false,
        };
        let only_xfce = LegacyDetection {
            mackes_shell_present: false,
            xfce4_present: true,
        };
        let both = LegacyDetection {
            mackes_shell_present: true,
            xfce4_present: true,
        };
        let summaries: std::collections::HashSet<_> = [
            all_false.summary(),
            only_mackes.summary(),
            only_xfce.summary(),
            both.summary(),
        ]
        .into_iter()
        .collect();
        assert_eq!(summaries.len(), 4);
    }

    #[test]
    fn migrate_binary_path_is_usr_bin() {
        let p = migrate_binary_path();
        assert_eq!(p, Path::new("/usr/bin/mde-migrate-from-1x"));
    }
}
