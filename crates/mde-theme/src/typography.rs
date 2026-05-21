//! Typography tokens. Locks: Q11/Q12 (Geologica display + body),
//! Q13 (IBM Plex Mono), Q14 (1.2 minor third scale),
//! Q15 (optical sizing — tight on display, default on body).

/// Display + body font family. Geologica is variable; the same
/// font face is used at every size, with the `opsz` axis driving
/// optical adjustments.
pub const FONT_DISPLAY_BODY: &str = "Geologica";

/// Monospace font family. IBM Plex Mono — paths, IDs, peer
/// addresses, code samples.
pub const FONT_MONO: &str = "IBM Plex Mono";

/// Type scale ratio. 1.2 minor third (Q14). Calm progression
/// matching Apple System Settings' rhythm.
pub const SCALE_RATIO: f32 = 1.2;

/// Sizes in scale points (sp), one tier per type role.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontSize {
    /// Caption / label — 12 sp.
    pub caption: f32,
    /// Body copy — 14 sp.
    pub body: f32,
    /// Subheading — 17 sp.
    pub subheading: f32,
    /// Heading — 20 sp.
    pub heading: f32,
    /// Section title — 24 sp.
    pub section: f32,
    /// Page / display title — 28 sp.
    pub display: f32,
    /// Monospace inline / code-fragment size — 13 sp.
    pub mono: f32,
}

impl FontSize {
    /// Token defaults — the 1.2 minor third scale per Q14.
    pub const fn defaults() -> Self {
        Self {
            caption:    12.0,
            body:       14.0,
            subheading: 17.0,
            heading:    20.0,
            section:    24.0,
            display:    28.0,
            mono:       13.0,
        }
    }
}

/// Letter-spacing adjustments per role. Q15: tight on display,
/// default on body. Values are in fractional em — apply via the
/// Iced widget's `letter-spacing` analogue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LetterSpacing {
    /// Tighten display titles ~1.5%.
    pub display: f32,
    /// Tighten section titles ~1%.
    pub section: f32,
    /// Tighten headings ~1%.
    pub heading: f32,
    /// Body / subheading / caption stay neutral.
    pub body: f32,
    /// Monospace stays neutral.
    pub mono: f32,
}

impl LetterSpacing {
    /// Defaults per Q15.
    pub const fn defaults() -> Self {
        Self {
            display: -0.015,
            section: -0.010,
            heading: -0.010,
            body:     0.000,
            mono:     0.000,
        }
    }
}

/// Font weights — Geologica's variable axis exposes 100..900;
/// the design system uses two: 400 (regular) and 500 (medium).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontWeight {
    /// 400 — body, caption.
    pub regular: u16,
    /// 500 — display, headings, section titles, button labels.
    pub medium: u16,
}

impl FontWeight {
    /// Defaults: 400 / 500.
    pub const fn defaults() -> Self {
        Self {
            regular: 400,
            medium:  500,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_is_1_2_minor_third() {
        assert!((SCALE_RATIO - 1.2).abs() < 0.001);
    }

    #[test]
    fn body_size_is_14sp() {
        assert_eq!(FontSize::defaults().body as i32, 14);
    }

    #[test]
    fn display_size_is_28sp() {
        assert_eq!(FontSize::defaults().display as i32, 28);
    }

    #[test]
    fn display_tracks_tighter_than_body() {
        let ls = LetterSpacing::defaults();
        assert!(ls.display < ls.body);
    }

    #[test]
    fn medium_weight_is_500() {
        assert_eq!(FontWeight::defaults().medium, 500);
    }

    #[test]
    fn font_family_is_geologica() {
        assert_eq!(FONT_DISPLAY_BODY, "Geologica");
    }

    #[test]
    fn mono_is_ibm_plex_mono() {
        assert_eq!(FONT_MONO, "IBM Plex Mono");
    }
}
