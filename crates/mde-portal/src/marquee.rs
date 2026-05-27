//! Portal-14.c (v6.0, R4-Q60) — marquee scroll for long labels.
//!
//! When a breadcrumb segment carries a label longer than the
//! viewport, the marquee primitive returns a substring of the
//! label that scrolls horizontally over time. Click + hover
//! semantics (pause-on-hover, click-jump-to-end) live in the
//! consuming view; this module is pure-fn so the scroll position
//! is computable from `(started_at, now, chars_per_sec)` alone.
//!
//! ## Design choices
//!
//! - **Char-based, not pixel-based.** R4-Q60 specifies 50 px/sec
//!   but font metrics aren't available to a pure helper. At the
//!   Dock's 11 px font size, monospaced glyphs run ~6.5 px wide,
//!   so 50 px/sec ≈ 7.7 chars/sec. The `DEFAULT_CHARS_PER_SEC`
//!   constant pins the char-rate equivalent of the design lock;
//!   downstream sites pass an override if they pick a different
//!   font.
//!
//! - **Loops with a trailing gap.** After scrolling past the end,
//!   the substring re-enters from the right edge of the viewport.
//!   A trailing whitespace gap (`MARQUEE_GAP_CHARS`) prevents the
//!   loop from appearing as one continuous tape.
//!
//! - **Short labels return unchanged.** When `label.chars()` is
//!   ≤ `viewport_chars`, no scrolling is needed — the function
//!   returns the original label. Saves the consumer a render-
//!   pressure check.
//!
//! Pure functions only — no I/O, no allocation surprises (one
//! `String` per call, sized at most `viewport_chars`).

use chrono::{DateTime, Local};

/// Char-rate equivalent of R4-Q60's "50 px/sec" lock. At Dock
/// font metrics (11 px Iced text, ~6.5 px monospaced glyph)
/// 50 px/sec rounds to ~7.7 chars/sec; we round down to 7 so
/// labels stay readable rather than nervous.
pub const DEFAULT_CHARS_PER_SEC: f32 = 7.0;

/// Trailing gap (in chars) between the label's tail and its
/// re-entry from the right. Three spaces reads as a clear
/// "the same label, repeating" beat without burning visual
/// space.
pub const MARQUEE_GAP_CHARS: usize = 3;

/// 33 ms tick interval — same cadence as the typewriter +
/// breath-line primitives so the Dock can reuse one render
/// tick when all three are active.
pub const TICK_INTERVAL_MS: u64 = 33;

/// Return the current viewport substring for a marquee-scrolled
/// label.
///
/// - When `label.chars() <= viewport_chars`, returns the label
///   unchanged (no scrolling needed).
/// - When `viewport_chars` is 0, returns an empty string.
/// - When elapsed is negative (clock skew) or zero, returns the
///   first `viewport_chars` of the label (the initial frame).
/// - Otherwise: builds a virtual `label + gap` track, computes
///   the current offset modulo track length, and slices a
///   `viewport_chars`-wide window starting at that offset. When
///   the window straddles the loop boundary, wraps to the start
///   of the track.
///
/// The output is always `viewport_chars` Unicode scalar values
/// wide (except when the label fits, in which case it's the
/// label's char count).
#[must_use]
pub fn marquee_visible_window(
    label: &str,
    viewport_chars: usize,
    started_at: DateTime<Local>,
    now: DateTime<Local>,
    chars_per_sec: f32,
) -> String {
    if viewport_chars == 0 {
        return String::new();
    }
    let chars: Vec<char> = label.chars().collect();
    if chars.len() <= viewport_chars {
        return label.to_string();
    }
    if chars_per_sec <= 0.0 {
        // No motion — pin to the head of the label.
        return chars[..viewport_chars].iter().collect();
    }
    let elapsed_ms = now.signed_duration_since(started_at).num_milliseconds();
    if elapsed_ms <= 0 {
        // First frame: show the head of the label.
        return chars[..viewport_chars].iter().collect();
    }
    let elapsed_secs = (elapsed_ms as f32) / 1000.0;
    let raw_offset = (elapsed_secs * chars_per_sec) as usize;
    // Track = label + gap of spaces. Scroll is modulo track len.
    let track_len = chars.len() + MARQUEE_GAP_CHARS;
    let offset = raw_offset % track_len;
    let mut out = String::with_capacity(viewport_chars * 4);
    for i in 0..viewport_chars {
        let pos = (offset + i) % track_len;
        if pos < chars.len() {
            out.push(chars[pos]);
        } else {
            out.push(' ');
        }
    }
    out
}

/// Return true when the marquee should currently be scrolling.
/// Consumers use this to decide whether to subscribe to render
/// ticks for the underlying label.
#[must_use]
pub fn marquee_active(label: &str, viewport_chars: usize) -> bool {
    label.chars().count() > viewport_chars && viewport_chars > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms_after: i64, base: DateTime<Local>) -> DateTime<Local> {
        base + chrono::Duration::milliseconds(ms_after)
    }

    /// Short label returns unchanged regardless of viewport size.
    #[test]
    fn short_label_returns_unchanged() {
        let base = Local::now();
        let now = at(5000, base);
        let r = marquee_visible_window("hi", 10, base, now, 7.0);
        assert_eq!(r, "hi");
    }

    /// Exactly-viewport-width label returns unchanged.
    #[test]
    fn exact_width_label_returns_unchanged() {
        let base = Local::now();
        let now = at(5000, base);
        let r = marquee_visible_window("abcdefghij", 10, base, now, 7.0);
        assert_eq!(r, "abcdefghij");
    }

    /// Zero viewport returns empty string.
    #[test]
    fn zero_viewport_returns_empty() {
        let base = Local::now();
        let r = marquee_visible_window("anything", 0, base, base, 7.0);
        assert_eq!(r, "");
    }

    /// First frame (t=0) shows the head of the label.
    #[test]
    fn first_frame_shows_label_head() {
        let base = Local::now();
        let r = marquee_visible_window("abcdefghijklmnop", 5, base, base, 7.0);
        assert_eq!(r, "abcde");
    }

    /// After 1 second at 7 chars/sec, the window has advanced by 7.
    #[test]
    fn one_second_advances_by_chars_per_sec() {
        let base = Local::now();
        let now = at(1000, base);
        let r = marquee_visible_window("abcdefghijklmnop", 5, base, now, 7.0);
        // Offset = 7; window = chars[7..12] = "hijkl"
        assert_eq!(r, "hijkl");
    }

    /// Half-second at 7 chars/sec advances by 3 (rounded down).
    #[test]
    fn half_second_advances_by_three() {
        let base = Local::now();
        let now = at(500, base);
        let r = marquee_visible_window("abcdefghijklmnop", 5, base, now, 7.0);
        // Offset = floor(0.5 * 7) = 3; window = chars[3..8] = "defgh"
        assert_eq!(r, "defgh");
    }

    /// At the end-of-track boundary, the window wraps with gap
    /// spaces followed by the head re-entering.
    #[test]
    fn wraps_with_trailing_gap() {
        let base = Local::now();
        // Label is 6 chars; viewport 4; track = 6 + 3 = 9.
        // After offset 6, window straddles into the gap: chars[6..6]
        // is empty (we're at end), so 3 spaces then chars[0..1].
        // raw_offset = elapsed_secs * 7 → at 6/7 sec we get offset 6.
        // 857 ms → 5.999 ≈ 5; use 1000 ms = offset 7.
        // offset 7: positions 7, 8, 0, 1 → " " + " " + "a" + "b"
        let now = at(1000, base);
        let r = marquee_visible_window("abcdef", 4, base, now, 7.0);
        assert_eq!(r, "  ab");
    }

    /// Negative elapsed (clock skew) returns the head of the label.
    #[test]
    fn negative_elapsed_returns_head() {
        let base = at(1000, Local::now());
        let now = at(500, Local::now());
        let r = marquee_visible_window("abcdefghij", 4, base, now, 7.0);
        assert_eq!(r, "abcd");
    }

    /// Zero chars_per_sec pins to the head of the label.
    #[test]
    fn zero_rate_pins_to_head() {
        let base = Local::now();
        let now = at(5000, base);
        let r = marquee_visible_window("abcdefghij", 4, base, now, 0.0);
        assert_eq!(r, "abcd");
    }

    /// One full loop returns the window to the head.
    #[test]
    fn full_loop_returns_to_head() {
        let base = Local::now();
        // Label 6 + gap 3 = track 9. At 7 chars/sec, one full loop
        // is 9/7 = 1.286 sec. Pick 9000 ms = 63 chars elapsed = 7
        // full loops. Offset = 63 % 9 = 0. Window from head.
        let now = at(9000, base);
        let r = marquee_visible_window("abcdef", 4, base, now, 7.0);
        assert_eq!(r, "abcd");
    }

    /// Output is always exactly viewport_chars wide for long labels.
    #[test]
    fn output_width_always_viewport_chars_for_long_labels() {
        let base = Local::now();
        let label = "abcdefghijklmnopqrstuvwxyz";
        for ms in [0_i64, 100, 500, 1000, 2500, 5000, 10000] {
            for viewport in [1_usize, 3, 8, 16, 25] {
                let now = at(ms, base);
                let r = marquee_visible_window(label, viewport, base, now, 7.0);
                assert_eq!(
                    r.chars().count(),
                    viewport,
                    "ms={ms} viewport={viewport} got {r:?}",
                );
            }
        }
    }

    /// marquee_active reflects whether scrolling will happen.
    #[test]
    fn marquee_active_matches_expected() {
        assert!(!marquee_active("hi", 10));
        assert!(!marquee_active("exactly10c", 10));
        assert!(marquee_active("longer than viewport", 10));
        assert!(!marquee_active("anything", 0));
    }

    /// Unicode label chars are counted correctly (not bytes).
    #[test]
    fn unicode_label_uses_scalar_count() {
        let base = Local::now();
        // "‹ étoile" = 8 scalar values (‹, space, é, t, o, i, l, e)
        // even though byte-len > 8.
        let label = "‹ étoile";
        assert_eq!(label.chars().count(), 8);
        let r = marquee_visible_window(label, 8, base, base, 7.0);
        // Fits exactly → returned unchanged.
        assert_eq!(r, label);
        let r2 = marquee_visible_window(label, 5, base, base, 7.0);
        // First frame → first 5 scalars.
        assert_eq!(r2.chars().count(), 5);
        assert_eq!(r2, "‹ éto");
    }

    /// Default chars-per-sec matches the R4-Q60 design lock.
    #[test]
    fn default_rate_matches_design_lock() {
        assert!((DEFAULT_CHARS_PER_SEC - 7.0).abs() < f32::EPSILON);
    }

    /// Gap constant is the documented 3 chars.
    #[test]
    fn gap_constant_matches_docs() {
        assert_eq!(MARQUEE_GAP_CHARS, 3);
    }
}
