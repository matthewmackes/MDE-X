//! Portal-14.d (v6.0, R4-Q91) — continuous breath-line gradient sweep.
//!
//! The Dock's breadcrumb row sits above a 2 px-tall gradient baseline.
//! That baseline isn't static — its hue centerpoint sweeps left-to-
//! right on a 15-second cycle, mirroring the way a soft AppKit
//! traffic-light glow breathes. The effect is a continuous, low-
//! amplitude visual signal that "the Dock is alive" without
//! competing with any content above it.
//!
//! Pure functions only — Iced widgets are built by the Dock view
//! using the phase value this module returns.
//!
//! ## Why 15-second cycle
//!
//! Faster than 10 s feels nervous; slower than 30 s feels static.
//! 15 s matches the breath-cadence the design lock pins for the
//! M-watermark idle glow + the lock-widget pulse. Consistency
//! across surfaces wins over per-surface tuning.

use chrono::{DateTime, Local};

/// Default sweep-cycle length per the R4-Q91 design lock.
pub const DEFAULT_CYCLE_SECONDS: f64 = 15.0;

/// 33 ms tick interval — same as the typewriter so the Dock can
/// reuse a single render tick when both primitives are active.
pub const TICK_INTERVAL_MS: u64 = 33;

/// Return the current sweep phase as a 0.0..1.0 cyclic value.
/// At phase 0.0 the gradient centerpoint is at the left edge;
/// at 0.5 it's mid-Dock; at 1.0 it wraps back to the left.
///
/// `started_at` is the reference instant (typically Dock-spawned-
/// at); negative elapsed (clock skew) clamps to 0.0.
#[must_use]
pub fn breath_line_phase(
    started_at: DateTime<Local>,
    now: DateTime<Local>,
    cycle_seconds: f64,
) -> f32 {
    let elapsed_ms = now.signed_duration_since(started_at).num_milliseconds();
    if elapsed_ms < 0 || cycle_seconds <= 0.0 {
        return 0.0;
    }
    let cycle_ms = cycle_seconds * 1000.0;
    let position = (elapsed_ms as f64) % cycle_ms;
    (position / cycle_ms) as f32
}

/// One gradient stop: (position in 0..1, R, G, B) packed for
/// trivial f32-only consumption. The Dock view converts these
/// to `iced::Color` at render time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreathStop {
    /// Horizontal position of this stop, in 0..1 across the Dock width.
    pub position: f32,
    /// Stop color, RGB packed as 0..1 floats (alpha implicit 1.0).
    pub rgb: (f32, f32, f32),
}

/// Return 5 gradient stops describing the breath-line at `phase`.
/// The first and last stops are the dimmed baseline; the middle
/// stop tracks `phase` and carries the brighter sweep color. Two
/// flanking stops 0.08 wide on either side of the center fade
/// the sweep edges so the transition is soft.
///
/// Wrap behavior: when the center would fall past the right edge,
/// it appears on both sides simultaneously (continuous wrap).
#[must_use]
pub fn breath_line_stops(phase: f32, sweep_rgb: (f32, f32, f32), base_rgb: (f32, f32, f32)) -> Vec<BreathStop> {
    let half_width: f32 = 0.08;
    let mut stops = Vec::with_capacity(5);
    // Always-present baseline endpoints.
    stops.push(BreathStop { position: 0.0, rgb: base_rgb });
    // Sweep center + flanking edges. Clamp positions to 0..1 so
    // a near-edge phase doesn't blow out the renderer.
    let left_edge = (phase - half_width).max(0.0);
    let right_edge = (phase + half_width).min(1.0);
    if left_edge > 0.0 {
        stops.push(BreathStop { position: left_edge, rgb: base_rgb });
    }
    stops.push(BreathStop { position: phase.clamp(0.0, 1.0), rgb: sweep_rgb });
    if right_edge < 1.0 {
        stops.push(BreathStop { position: right_edge, rgb: base_rgb });
    }
    stops.push(BreathStop { position: 1.0, rgb: base_rgb });
    stops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms_after: i64, base: DateTime<Local>) -> DateTime<Local> {
        base + chrono::Duration::milliseconds(ms_after)
    }

    /// Default cycle const matches R4-Q91 design lock.
    #[test]
    fn default_cycle_matches_design_lock() {
        assert!((DEFAULT_CYCLE_SECONDS - 15.0).abs() < f64::EPSILON);
    }

    /// At t=0, phase is exactly 0.
    #[test]
    fn phase_at_zero_elapsed_is_zero() {
        let base = Local::now();
        let p = breath_line_phase(base, base, 15.0);
        assert!((p - 0.0).abs() < f32::EPSILON);
    }

    /// At half-cycle, phase is exactly 0.5.
    #[test]
    fn phase_at_half_cycle_is_half() {
        let base = Local::now();
        let now = at(7500, base);
        let p = breath_line_phase(base, now, 15.0);
        assert!((p - 0.5).abs() < 0.001);
    }

    /// One full cycle later, phase wraps back to 0.
    #[test]
    fn phase_wraps_at_full_cycle() {
        let base = Local::now();
        let now = at(15_000, base);
        let p = breath_line_phase(base, now, 15.0);
        assert!(p < 0.001, "got phase {p}");
    }

    /// 1.5 cycles later, phase is back to 0.5 (full wrap behavior).
    #[test]
    fn phase_wraps_continuously() {
        let base = Local::now();
        let now = at(22_500, base);
        let p = breath_line_phase(base, now, 15.0);
        assert!((p - 0.5).abs() < 0.001);
    }

    /// Negative elapsed (clock skew) returns 0.0 rather than panicking.
    #[test]
    fn phase_negative_elapsed_is_zero() {
        let base = at(500, Local::now());
        let now = at(0, Local::now());
        let p = breath_line_phase(base, now, 15.0);
        assert!((p - 0.0).abs() < f32::EPSILON);
    }

    /// Zero or negative cycle_seconds returns 0.0 (no panic).
    #[test]
    fn phase_zero_cycle_is_zero() {
        let base = Local::now();
        let now = at(1000, base);
        assert!((breath_line_phase(base, now, 0.0) - 0.0).abs() < f32::EPSILON);
        assert!((breath_line_phase(base, now, -1.0) - 0.0).abs() < f32::EPSILON);
    }

    /// At mid-sweep, stops list has 5 entries with center at phase.
    #[test]
    fn stops_at_mid_sweep_has_five_with_center_at_phase() {
        let sweep = (0.5, 0.6, 1.0);
        let base = (0.1, 0.1, 0.15);
        let stops = breath_line_stops(0.5, sweep, base);
        assert_eq!(stops.len(), 5);
        assert!((stops[0].position - 0.0).abs() < f32::EPSILON);
        assert!((stops[4].position - 1.0).abs() < f32::EPSILON);
        // Center stop should be at phase 0.5 with sweep color.
        let center = stops.iter().find(|s| (s.position - 0.5).abs() < 0.001).unwrap();
        assert_eq!(center.rgb, sweep);
    }

    /// At left edge (phase 0), the left flank stop drops out so
    /// the center collapses against position 0.
    #[test]
    fn stops_at_left_edge_drops_left_flank() {
        let sweep = (1.0, 1.0, 1.0);
        let base = (0.0, 0.0, 0.0);
        let stops = breath_line_stops(0.0, sweep, base);
        // Without left flank: [0.0=base, 0.0=sweep, 0.08=base, 1.0=base]
        // = 4 stops.
        assert_eq!(stops.len(), 4);
    }

    /// At right edge (phase 1.0), the right flank stop drops out.
    #[test]
    fn stops_at_right_edge_drops_right_flank() {
        let sweep = (1.0, 1.0, 1.0);
        let base = (0.0, 0.0, 0.0);
        let stops = breath_line_stops(1.0, sweep, base);
        // Without right flank: [0.0=base, 0.92=base, 1.0=sweep, 1.0=base]
        // = 4 stops.
        assert_eq!(stops.len(), 4);
    }

    /// Stops are always sorted by position ascending.
    #[test]
    fn stops_sorted_by_position_ascending() {
        let sweep = (1.0, 1.0, 1.0);
        let base = (0.0, 0.0, 0.0);
        for phase in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            let stops = breath_line_stops(phase, sweep, base);
            for window in stops.windows(2) {
                assert!(
                    window[0].position <= window[1].position,
                    "stops out of order at phase {phase}",
                );
            }
        }
    }
}
