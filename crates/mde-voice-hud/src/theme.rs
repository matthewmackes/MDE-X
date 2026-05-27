//! Color tokens + spacing constants for `mde-voice-hud`.
//!
//! The HUD is a Portal-full overlay surface and per
//! `docs/design/v6.0-pjsip-presence-and-hud.md` §2.1 gets its own
//! focused Material-3-dark sub-palette inside the platform's
//! Classic-ChromeOS lock (Q1+Q2 of 25-Q). The values below mirror
//! the design bundle's `styles.css` `:root` tokens, adapted to
//! Iced's `iced::Color` (RGB f32 channels, 0.0–1.0).
//!
//! Per the design-tokens lint snapshot allowlist, hex literals
//! outside `data/css/tokens.css` get caught — this module is one
//! of the canonical token sites for the voice-HUD surface and is
//! the only place that should carry these constants.

use iced::Color;

/// Helper: convert an 8-bit RGB hex into an Iced `Color`.
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}
const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
}

// ---------- Surface palette (Material 3 dark, neutral default) ----------

/// Base surface (`--md-surf` in the bundle).
pub const SURF: Color = rgb(0x13, 0x13, 0x16);
/// Dim variant for the inner SIP-log + activity panels.
pub const SURF_DIM: Color = rgb(0x0e, 0x0e, 0x10);
/// Low-elevation container — used for topbar / call bar.
pub const SURF_C_LOW: Color = rgb(0x18, 0x18, 0x1b);
/// Mid-elevation container — used for keypad keys / hop pills.
pub const SURF_C: Color = rgb(0x1d, 0x1d, 0x20);
/// High-elevation hover state.
pub const SURF_C_HI: Color = rgb(0x28, 0x28, 0x2c);
/// Hierarchical accent surface — used for selected tab / avatar bg.
pub const SURF_C_HIER: Color = rgb(0x33, 0x33, 0x39);
/// Outline / divider line.
pub const OUTLINE: Color = rgb(0x4a, 0x4a, 0x52);
/// Outline variant (lighter divider).
pub const OUTLINE_VAR: Color = rgb(0x34, 0x34, 0x3b);

// ---------- Foreground palette ----------

pub const ON_SURF: Color = rgb(0xec, 0xec, 0xf2);
pub const ON_SURF_VAR: Color = rgb(0xc7, 0xc7, 0xd1);
pub const ON_SURF_MUTED: Color = rgb(0x8a, 0x8a, 0x96);

// ---------- Status colors ----------

pub const SUCCESS: Color = rgb(0x6f, 0xdc, 0x8c);
pub const WARNING: Color = rgb(0xf1, 0xc2, 0x1b);
pub const ERROR: Color = rgb(0xff, 0xb4, 0xab);
pub const INFO: Color = rgb(0x78, 0xa9, 0xff);

// ---------- Primary (Mackes orange default preset) ----------

pub const PRIMARY: Color = rgb(0xff, 0xb6, 0x8a);
pub const ON_PRIMARY: Color = rgb(0x4c, 0x1e, 0x00);
pub const PRIMARY_C: Color = rgb(0x6b, 0x2e, 0x00);
pub const ON_PRIMARY_C: Color = rgb(0xff, 0xdc, 0xc4);
pub const PRIMARY_FIXED: Color = rgb(0xf1, 0x85, 0x3d);

// ---------- Accept / decline FAB ----------

/// Background for the Call FAB (green-700-ish).
pub const ACCEPT_C: Color = rgb(0x00, 0x6e, 0x2c);
/// Foreground glyph color on the Call FAB.
pub const ACCEPT_FG: Color = rgb(0x7d, 0xf0, 0xa3);
/// Background for the Hangup FAB (red-900-ish).
pub const ERROR_C: Color = rgb(0x93, 0x00, 0x0a);
pub const ON_ERROR_C: Color = rgb(0xff, 0xda, 0xd6);

// ---------- Presence pip colors ----------

pub const PRESENCE_AVAILABLE: Color = SUCCESS;
pub const PRESENCE_ON_CALL: Color = PRIMARY_FIXED;
pub const PRESENCE_AWAY: Color = WARNING;
pub const PRESENCE_DND: Color = ERROR;
pub const PRESENCE_OFFLINE: Color = OUTLINE;

// ---------- Translucent overlays ----------

pub const SCRIM_55: Color = rgba(0x00, 0x00, 0x00, 0.55);
pub const HOVER_TINT_8: Color = rgba(0xff, 0xff, 0xff, 0.04);

// ---------- HUD dimensions ----------

/// Cozy default width (px).
pub const HUD_W: f32 = 420.0;
/// Cozy default height (px).
pub const HUD_H: f32 = 720.0;
/// Bottom margin above the dock (px).
pub const HUD_MARGIN_BOTTOM: i32 = 56;
/// Right margin from the screen edge (px).
pub const HUD_MARGIN_RIGHT: i32 = 16;
/// Row height for keypad keys + peer rows (cozy density, px).
pub const ROW_H: f32 = 64.0;

// ---------- Border radii ----------

pub const R_XS: f32 = 8.0;
pub const R_S: f32 = 12.0;
pub const R_M: f32 = 16.0;
pub const R_L: f32 = 20.0;
pub const R_XL: f32 = 28.0;
pub const R_FULL: f32 = 999.0;
