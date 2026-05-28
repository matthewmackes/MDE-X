//! `mde-motion` — the canonical motion primitives for MDE's Iced
//! layer-shell surfaces.
//!
//! Sway is the compositor and windows snap (no compositor-level
//! animation); *all* motion lives in the MDE-drawn surfaces. This
//! crate is the single source those surfaces consume so every
//! animation resolves to the same locked grid and curves:
//!
//!   * [`grid`]    — the four-value duration grid + exit-tier + stagger.
//!   * [`easing`]  — Material ease-out (arrival) / ease-in (dismissal).
//!   * [`tween`]   — an interruptible eased-redirect tween.
//!   * [`stagger`] — capped per-item list-reveal offsets.
//!
//! **Interruptible means eased redirect, NOT a physics spring.** A new
//! target mid-animation re-bases the tween from its current value (no
//! snap, no wait) but the curve still has no overshoot/bounce — the
//! "no spring/overshoot/bounce" lock in `motion-language.md` §1 /
//! `data/css/motion-vocabulary.css` §1 holds. See
//! `docs/design/sway-native-shell.md` §2 (Q3/Q6/Q8/Q61).
//!
//! Pure Rust (no toolkit dep) so the math unit-tests headless. The
//! crate is the unblock for the ANIM-1..13 epic; widget authors reach
//! it through `mde_iced_components::motion`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// The locked timing grid (durations in milliseconds) + the derived
/// exit-tier and stagger constants.
pub mod grid {
    /// Default state change (`--mde-duration-standard`).
    pub const STANDARD_MS: u32 = 150;
    /// Button press / toggle (`--mde-duration-active`).
    pub const ACTIVE_MS: u32 = 100;
    /// Toast / popover / surface open (`--mde-duration-info-arrival`).
    pub const INFO_ARRIVAL_MS: u32 = 200;
    /// Toast / popover / surface close (`--mde-duration-info-dismissal`).
    pub const INFO_DISMISSAL_MS: u32 = 120;

    /// Per-item delay for a staggered list/grid reveal.
    pub const STAGGER_STEP_MS: u32 = 20;
    /// Items beyond this index in a staggered reveal appear instantly,
    /// so a long list never crawls (sway-native lock Q15).
    pub const STAGGER_CAP: usize = 8;

    /// The dismiss duration for a surface that revealed over
    /// `reveal_ms`: one grid tier faster (200→150, 150→120, 120→100),
    /// floored at [`ACTIVE_MS`]; any non-grid value is returned
    /// unchanged but still floored. Implements the sway-native
    /// exit rule (Q8).
    #[must_use]
    pub const fn exit_ms(reveal_ms: u32) -> u32 {
        match reveal_ms {
            INFO_ARRIVAL_MS => STANDARD_MS,
            STANDARD_MS => INFO_DISMISSAL_MS,
            INFO_DISMISSAL_MS => ACTIVE_MS,
            other if other > ACTIVE_MS => other,
            _ => ACTIVE_MS,
        }
    }
}

/// Material-standard easing curves (cubic-bézier), with no
/// spring/overshoot/bounce.
pub mod easing {
    /// A CSS-style cubic-bézier easing curve with implicit `(0,0)` and
    /// `(1,1)` endpoints and two control points.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct CubicBezier {
        /// First control-point x.
        pub x1: f32,
        /// First control-point y.
        pub y1: f32,
        /// Second control-point x.
        pub x2: f32,
        /// Second control-point y.
        pub y2: f32,
    }

    /// Material standard **ease-out** — arrival/reveal motion.
    /// `cubic-bezier(0.0, 0.0, 0.2, 1.0)`.
    pub const EASE_OUT: CubicBezier = CubicBezier { x1: 0.0, y1: 0.0, x2: 0.2, y2: 1.0 };
    /// Material standard **ease-in** — dismissal motion.
    /// `cubic-bezier(0.4, 0.0, 1.0, 1.0)`.
    pub const EASE_IN: CubicBezier = CubicBezier { x1: 0.4, y1: 0.0, x2: 1.0, y2: 1.0 };

    impl CubicBezier {
        /// Eased output for linear progress `p` (clamped to `[0, 1]`).
        /// Solves `x(t) = p` for the curve parameter `t` (bisection,
        /// monotonic for our control points), then evaluates `y(t)`.
        #[must_use]
        pub fn eval(&self, p: f32) -> f32 {
            let p = p.clamp(0.0, 1.0);
            let t = self.solve_t_for_x(p);
            Self::component(t, self.y1, self.y2)
        }

        /// One axis of the bézier at parameter `t` with endpoints 0..1.
        fn component(t: f32, c1: f32, c2: f32) -> f32 {
            let u = 1.0 - t;
            3.0 * u * u * t * c1 + 3.0 * u * t * t * c2 + t * t * t
        }

        /// Find `t` such that `x(t) ≈ x` by bisection.
        fn solve_t_for_x(&self, x: f32) -> f32 {
            let mut lo = 0.0_f32;
            let mut hi = 1.0_f32;
            let mut t = x;
            for _ in 0..32 {
                let xt = Self::component(t, self.x1, self.x2);
                let err = xt - x;
                if err.abs() < 1e-4 {
                    break;
                }
                if err > 0.0 {
                    hi = t;
                } else {
                    lo = t;
                }
                t = 0.5 * (lo + hi);
            }
            t
        }
    }
}

/// An interruptible eased tween between two `f32` values.
pub mod tween {
    use crate::easing::CubicBezier;

    /// A tween from `from` to `to` over `duration_ms`, started at
    /// `start_ms` (caller-supplied monotonic milliseconds), shaped by
    /// `easing`. Interruptible via [`Tween::redirect`].
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Tween {
        /// Start value.
        pub from: f32,
        /// Target value.
        pub to: f32,
        /// Duration in milliseconds (always ≥ 1).
        pub duration_ms: u32,
        /// Easing curve.
        pub easing: CubicBezier,
        /// Start timestamp in milliseconds (monotonic, caller clock).
        pub start_ms: u64,
    }

    impl Tween {
        /// Construct a tween. `duration_ms` is floored at 1 to avoid
        /// divide-by-zero.
        #[must_use]
        pub fn new(from: f32, to: f32, duration_ms: u32, easing: CubicBezier, start_ms: u64) -> Self {
            Self { from, to, duration_ms: duration_ms.max(1), easing, start_ms }
        }

        /// Linear progress in `[0, 1]` at `now_ms`.
        #[must_use]
        pub fn progress(&self, now_ms: u64) -> f32 {
            let elapsed = now_ms.saturating_sub(self.start_ms) as f32;
            (elapsed / self.duration_ms as f32).clamp(0.0, 1.0)
        }

        /// Eased value at `now_ms`.
        #[must_use]
        pub fn value_at(&self, now_ms: u64) -> f32 {
            let eased = self.easing.eval(self.progress(now_ms));
            self.from + (self.to - self.from) * eased
        }

        /// Whether the tween has reached its target at `now_ms`.
        #[must_use]
        pub fn is_done(&self, now_ms: u64) -> bool {
            self.progress(now_ms) >= 1.0
        }

        /// Interruptible redirect: a fresh tween from the **current**
        /// eased value toward `new_to`, re-based at `now_ms`. This is
        /// the eased-redirect that gives motion its premium,
        /// never-stuck feel — no snap, no overshoot.
        #[must_use]
        pub fn redirect(&self, new_to: f32, now_ms: u64, duration_ms: u32) -> Self {
            Self::new(self.value_at(now_ms), new_to, duration_ms, self.easing, now_ms)
        }

        /// Resolve honoring reduced motion: when `reduce` is set, jump
        /// straight to `to` (the accessibility toggle path, Q4).
        #[must_use]
        pub fn resolve(&self, now_ms: u64, reduce: bool) -> f32 {
            if reduce {
                self.to
            } else {
                self.value_at(now_ms)
            }
        }
    }
}

/// Capped per-item offsets for staggered list/grid reveals.
pub mod stagger {
    use crate::grid::{STAGGER_CAP, STAGGER_STEP_MS};

    /// Delay (ms) before item `index` begins its reveal. Items at or
    /// beyond [`STAGGER_CAP`](crate::grid::STAGGER_CAP) start
    /// immediately (`0`) so a long list never crawls.
    #[must_use]
    pub const fn delay_ms(index: usize) -> u32 {
        if index >= STAGGER_CAP {
            0
        } else {
            (index as u32) * STAGGER_STEP_MS
        }
    }
}

pub use easing::{CubicBezier, EASE_IN, EASE_OUT};
pub use tween::Tween;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_tier_steps_down_one_grid_level() {
        assert_eq!(grid::exit_ms(grid::INFO_ARRIVAL_MS), grid::STANDARD_MS);
        assert_eq!(grid::exit_ms(grid::STANDARD_MS), grid::INFO_DISMISSAL_MS);
        assert_eq!(grid::exit_ms(grid::INFO_DISMISSAL_MS), grid::ACTIVE_MS);
        // Floor at ACTIVE_MS for sub-grid values.
        assert_eq!(grid::exit_ms(50), grid::ACTIVE_MS);
        // Non-grid larger value passes through unchanged.
        assert_eq!(grid::exit_ms(500), 500);
    }

    #[test]
    fn easing_hits_endpoints() {
        assert!((EASE_OUT.eval(0.0) - 0.0).abs() < 1e-3);
        assert!((EASE_OUT.eval(1.0) - 1.0).abs() < 1e-3);
        assert!((EASE_IN.eval(0.0) - 0.0).abs() < 1e-3);
        assert!((EASE_IN.eval(1.0) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn easing_clamps_out_of_range_progress() {
        assert!((EASE_OUT.eval(-1.0) - 0.0).abs() < 1e-3);
        assert!((EASE_OUT.eval(2.0) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn ease_out_decelerates_ease_in_accelerates() {
        // ease-out is ahead of linear at the midpoint; ease-in behind.
        assert!(EASE_OUT.eval(0.5) > 0.5);
        assert!(EASE_IN.eval(0.5) < 0.5);
    }

    #[test]
    fn easing_is_monotonic() {
        let mut prev = EASE_OUT.eval(0.0);
        let mut p = 0.05;
        while p <= 1.0 {
            let v = EASE_OUT.eval(p);
            assert!(v >= prev - 1e-3, "non-monotonic at {p}: {v} < {prev}");
            prev = v;
            p += 0.05;
        }
    }

    #[test]
    fn tween_endpoints_and_clamp() {
        let t = Tween::new(0.0, 100.0, 200, EASE_OUT, 1_000);
        assert!((t.value_at(1_000) - 0.0).abs() < 1e-3);
        assert!((t.value_at(1_200) - 100.0).abs() < 1e-1);
        // Past the end stays clamped at the target.
        assert!((t.value_at(5_000) - 100.0).abs() < 1e-3);
        // Before the start stays at from.
        assert!((t.value_at(0) - 0.0).abs() < 1e-3);
        assert!(t.is_done(1_200));
        assert!(!t.is_done(1_100));
    }

    #[test]
    fn redirect_starts_from_current_value_no_jump() {
        // Reveal 0→100 over 200ms; interrupt at the midpoint toward 0.
        let t = Tween::new(0.0, 100.0, 200, EASE_OUT, 0);
        let mid = t.value_at(100); // current eased value at 100ms
        let r = t.redirect(0.0, 100, 150);
        // The redirected tween's value at its start equals the current
        // value — continuity, no snap.
        assert!((r.value_at(100) - mid).abs() < 1e-3);
        // And it heads toward the new target.
        assert!((r.value_at(100 + 150) - 0.0).abs() < 1e-1);
    }

    #[test]
    fn resolve_honors_reduced_motion() {
        let t = Tween::new(0.0, 1.0, 200, EASE_OUT, 0);
        // Mid-flight, reduced motion jumps to the target.
        assert!((t.resolve(50, true) - 1.0).abs() < 1e-6);
        // Without reduction it follows the curve.
        assert!(t.resolve(50, false) < 1.0);
    }

    #[test]
    fn stagger_caps_long_lists() {
        assert_eq!(stagger::delay_ms(0), 0);
        assert_eq!(stagger::delay_ms(3), 60);
        assert_eq!(stagger::delay_ms(grid::STAGGER_CAP - 1), 140);
        // Beyond the cap: instant.
        assert_eq!(stagger::delay_ms(grid::STAGGER_CAP), 0);
        assert_eq!(stagger::delay_ms(100), 0);
    }
}
