//! Install profiles (locked matrix, AI_GOVERNANCE §11.1 / INST epic).
//!
//! A clean install builds **up** from a minimal Fedora Server CLI:
//! `lighthouse` and `headless` stay headless; `full` layers the sway
//! desktop on top (operator directive 2026-05-29).

use std::fmt;
use std::str::FromStr;

/// One of the three install profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Routing-only mesh node: nebula + mackesd + GlusterFS client
    /// (read-only mount, no brick). VPS-friendly.
    Lighthouse,
    /// Headless mesh peer: lighthouse + a GlusterFS brick + fleet
    /// ansible-pull + monitoring. No desktop.
    Headless,
    /// Full workstation: headless + the sway/Iced desktop.
    Full,
}

impl Profile {
    /// Lowercase wire name passed to `mackes.birthright --profile=`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lighthouse => "lighthouse",
            Self::Headless => "headless",
            Self::Full => "full",
        }
    }

    /// One-line description for the interactive picker.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Lighthouse => {
                "routing-only mesh node (nebula + mackesd + read-only mesh-home); no desktop"
            }
            Self::Headless => {
                "headless peer (mesh-home brick + fleet + monitoring); no desktop"
            }
            Self::Full => "full workstation (everything above + the sway desktop)",
        }
    }

    /// Whether reaching this profile requires the `mde-desktop` RPM.
    #[must_use]
    pub const fn needs_desktop_rpm(self) -> bool {
        matches!(self, Self::Full)
    }

    /// All profiles in picker-menu order.
    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::Lighthouse, Self::Headless, Self::Full]
    }

    /// Capability rank: each profile is a strict superset of the one
    /// below it (`lighthouse` ⊂ `headless` ⊂ `full`). Used to tell an
    /// upgrade (rank rises, nothing lost) from a lossy downgrade (rank
    /// falls, the brick and/or desktop get torn down).
    #[must_use]
    const fn rank(self) -> u8 {
        match self {
            Self::Lighthouse => 0,
            Self::Headless => 1,
            Self::Full => 2,
        }
    }
}

/// Whether moving from `prev` to `new` drops capabilities the operator
/// currently has (the brick on `headless`/`full`, the desktop on
/// `full`). A lossy downgrade gets INST-6's extra typed-`<prev>` confirm
/// so muscle-memory `NUKE` doesn't silently demote a workstation to a
/// routing-only lighthouse. Same-profile reinstalls and upgrades are not
/// lossy.
#[must_use]
pub fn is_lossy_downgrade(prev: Profile, new: Profile) -> bool {
    new.rank() < prev.rank()
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when a profile name doesn't match a known profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseProfileError(pub String);

impl fmt::Display for ParseProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown profile: {} (choose lighthouse|headless|full)",
            self.0
        )
    }
}

impl std::error::Error for ParseProfileError {}

impl FromStr for Profile {
    type Err = ParseProfileError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "lighthouse" => Ok(Self::Lighthouse),
            "headless" => Ok(Self::Headless),
            "full" => Ok(Self::Full),
            other => Err(ParseProfileError(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_str() {
        for p in Profile::all() {
            assert_eq!(p.as_str().parse::<Profile>(), Ok(p));
        }
    }

    #[test]
    fn parse_is_case_insensitive_and_trims() {
        assert_eq!("  FULL ".parse::<Profile>(), Ok(Profile::Full));
    }

    #[test]
    fn unknown_profile_errors() {
        assert!("server".parse::<Profile>().is_err());
    }

    #[test]
    fn only_full_needs_desktop_rpm() {
        assert!(Profile::Full.needs_desktop_rpm());
        assert!(!Profile::Headless.needs_desktop_rpm());
        assert!(!Profile::Lighthouse.needs_desktop_rpm());
    }

    #[test]
    fn downgrades_are_lossy() {
        use Profile::{Full, Headless, Lighthouse};
        assert!(is_lossy_downgrade(Full, Headless));
        assert!(is_lossy_downgrade(Full, Lighthouse));
        assert!(is_lossy_downgrade(Headless, Lighthouse));
    }

    #[test]
    fn upgrades_and_same_profile_are_not_lossy() {
        use Profile::{Full, Headless, Lighthouse};
        // Upgrades.
        assert!(!is_lossy_downgrade(Lighthouse, Headless));
        assert!(!is_lossy_downgrade(Lighthouse, Full));
        assert!(!is_lossy_downgrade(Headless, Full));
        // Same-profile reinstalls.
        for p in Profile::all() {
            assert!(!is_lossy_downgrade(p, p));
        }
    }
}
