//! Phase 12.18 — HTTPS-tunneled fallback policy layer.
//!
//! Locked 2026-05-19 (Q10 + Q18 of the connectivity survey,
//! `docs/design/v12-connectivity-scope.md`):
//!
//!   * Activates after **3 consecutive failed direct-UDP +
//!     DERP-UDP probe pairs** (one "failure cycle" = one direct
//!     UDP probe failing AND its DERP-UDP counterpart failing in
//!     the same observation window). Two failure cycles = wait;
//!     three = activate.
//!   * Targets TCP/443 + a realistic TLS handshake + SNI + a
//!     Let's Encrypt cert chain. Goal: indistinguishable from
//!     real HTTPS to deep-packet-inspection middleboxes.
//!   * Once activated, stays activated until a fresh **direct-UDP
//!     OR DERP-UDP probe succeeds**, at which point we revert to
//!     the upstream path.
//!
//! This module ships the **policy layer** — the failure-window
//! detector + the activation state machine + the pure-fn
//! transition rules. The actual TLS handshake + tunnel transport
//! is a separate wire-protocol module that consumes
//! `HttpsFallback::is_active()`; gated behind future work that
//! pulls in `rustls` + the realistic SNI / cert chain bits.
//!
//! Pure-fn / pure-data — testable in microseconds.

/// Observed outcome of one probe pair (direct-UDP +
/// DERP-UDP) in a single observation window. The connectivity
/// worker emits one of these per probe cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbePairOutcome {
    /// At least one of (direct-UDP, DERP-UDP) succeeded — the
    /// peer is reachable via a UDP path.
    AnyUdpSucceeded,
    /// Both direct-UDP and DERP-UDP failed in the same window —
    /// the UDP-only path is wholly down.
    BothUdpFailed,
}

/// Locked failure threshold. Three consecutive
/// `BothUdpFailed` outcomes = activate the HTTPS fallback.
pub const FAILURE_THRESHOLD: u32 = 3;

/// Sliding-window counter that tracks consecutive UDP-only
/// failures. Resets to 0 on any `AnyUdpSucceeded` observation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FailureWindow {
    consecutive_failures: u32,
}

impl FailureWindow {
    /// Construct a fresh window with no failures yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one probe-pair outcome. Returns the new failure count.
    pub fn observe(&mut self, outcome: ProbePairOutcome) -> u32 {
        match outcome {
            ProbePairOutcome::BothUdpFailed => {
                self.consecutive_failures =
                    self.consecutive_failures.saturating_add(1);
            }
            ProbePairOutcome::AnyUdpSucceeded => {
                self.consecutive_failures = 0;
            }
        }
        self.consecutive_failures
    }

    /// Current consecutive failure count.
    #[must_use]
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// `true` when the failure count has reached the locked
    /// threshold (default 3) — caller should activate the HTTPS
    /// fallback.
    #[must_use]
    pub fn threshold_met(&self) -> bool {
        self.consecutive_failures >= FAILURE_THRESHOLD
    }
}

/// HTTPS-tunnel activation state machine. The connectivity
/// worker drives transitions; the tunnel transport reads
/// `is_active()` to decide whether to spray packets over the
/// HTTPS path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpsFallbackState {
    /// Default state. Direct-UDP / DERP-UDP paths are healthy.
    #[default]
    Inactive,
    /// Failure threshold met; TLS handshake in flight. Treated
    /// as "soon-to-be-active" by the routing layer — the panel
    /// surfaces a brief "connecting via HTTPS…" toast.
    Activating,
    /// Tunnel up + carrying traffic. Routing layer sprays
    /// packets here.
    Active,
    /// Tunnel was up but the TLS handshake or the underlying
    /// TCP connection failed; reverting to the unmodified
    /// failure-window state. From Failing we go back to
    /// Inactive when a fresh UDP probe succeeds, OR back to
    /// Activating after one more threshold cycle.
    Failing,
}

impl HttpsFallbackState {
    /// `true` when the routing layer should send packets over
    /// the HTTPS tunnel. Active is the only state where traffic
    /// flows through the fallback; Activating means we're still
    /// in TLS handshake.
    #[must_use]
    pub fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    /// `true` when the UI should surface the "connecting via
    /// HTTPS…" toast.
    #[must_use]
    pub fn is_activating(self) -> bool {
        matches!(self, Self::Activating)
    }
}

/// Pure-fn transition table. Public so unit tests can pin every
/// edge. The connectivity worker calls this with the current
/// state + the next probe outcome OR a TLS-handshake outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionInput {
    /// One probe-pair outcome (direct-UDP + DERP-UDP).
    Probe(ProbePairOutcome),
    /// TLS handshake completed successfully.
    HandshakeOk,
    /// TLS handshake failed.
    HandshakeFailed,
    /// Active tunnel's TCP connection broke.
    TunnelLost,
}

/// Apply one input to the (state, window) pair. Returns the new
/// state; the window is mutated in place.
///
/// Rules:
///
///   * Inactive + Probe(BothUdpFailed) ×3 → Activating.
///   * Activating + HandshakeOk → Active.
///   * Activating + HandshakeFailed → Failing.
///   * Active + Probe(AnyUdpSucceeded) → Inactive (revert).
///   * Active + TunnelLost → Failing.
///   * Failing + Probe(AnyUdpSucceeded) → Inactive (revert).
///   * Failing + Probe(BothUdpFailed) ×3 → Activating (retry).
#[must_use]
pub fn transition(
    state: HttpsFallbackState,
    window: &mut FailureWindow,
    input: TransitionInput,
) -> HttpsFallbackState {
    match (state, input) {
        // From Inactive — tally failures, activate on threshold.
        (HttpsFallbackState::Inactive, TransitionInput::Probe(outcome)) => {
            window.observe(outcome);
            if window.threshold_met() {
                // Reset window so a re-entry into Inactive starts
                // clean (the next failure cycle counts from 0).
                *window = FailureWindow::new();
                HttpsFallbackState::Activating
            } else {
                HttpsFallbackState::Inactive
            }
        }
        // Handshake outcomes while Inactive are no-ops (shouldn't
        // happen in normal flow, but no harm if they do).
        (HttpsFallbackState::Inactive, _) => HttpsFallbackState::Inactive,

        // From Activating — wait for handshake outcome; ignore
        // probe outcomes (we'll re-window-tally once we're back
        // in Inactive or Failing).
        (HttpsFallbackState::Activating, TransitionInput::HandshakeOk) => {
            HttpsFallbackState::Active
        }
        (HttpsFallbackState::Activating, TransitionInput::HandshakeFailed) => {
            HttpsFallbackState::Failing
        }
        (HttpsFallbackState::Activating, _) => HttpsFallbackState::Activating,

        // From Active — revert to Inactive on UDP recovery; flip
        // to Failing on tunnel loss; ignore the BothUdpFailed
        // outcome (we're already routing around it).
        (
            HttpsFallbackState::Active,
            TransitionInput::Probe(ProbePairOutcome::AnyUdpSucceeded),
        ) => {
            *window = FailureWindow::new();
            HttpsFallbackState::Inactive
        }
        (HttpsFallbackState::Active, TransitionInput::TunnelLost) => {
            HttpsFallbackState::Failing
        }
        (HttpsFallbackState::Active, _) => HttpsFallbackState::Active,

        // From Failing — recovery returns us to Inactive;
        // re-meeting the threshold retries Activating; other
        // inputs hold.
        (
            HttpsFallbackState::Failing,
            TransitionInput::Probe(ProbePairOutcome::AnyUdpSucceeded),
        ) => {
            *window = FailureWindow::new();
            HttpsFallbackState::Inactive
        }
        (
            HttpsFallbackState::Failing,
            TransitionInput::Probe(ProbePairOutcome::BothUdpFailed),
        ) => {
            window.observe(ProbePairOutcome::BothUdpFailed);
            if window.threshold_met() {
                *window = FailureWindow::new();
                HttpsFallbackState::Activating
            } else {
                HttpsFallbackState::Failing
            }
        }
        (HttpsFallbackState::Failing, _) => HttpsFallbackState::Failing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fail(n: u32, fw: &mut FailureWindow) -> u32 {
        let mut last = 0;
        for _ in 0..n {
            last = fw.observe(ProbePairOutcome::BothUdpFailed);
        }
        last
    }

    // --- FailureWindow -----------------------------------------------

    #[test]
    fn fresh_window_has_zero_failures() {
        let fw = FailureWindow::new();
        assert_eq!(fw.consecutive_failures(), 0);
        assert!(!fw.threshold_met());
    }

    #[test]
    fn observing_failures_accumulates() {
        let mut fw = FailureWindow::new();
        assert_eq!(fw.observe(ProbePairOutcome::BothUdpFailed), 1);
        assert_eq!(fw.observe(ProbePairOutcome::BothUdpFailed), 2);
        assert_eq!(fw.observe(ProbePairOutcome::BothUdpFailed), 3);
    }

    #[test]
    fn any_udp_success_resets_window() {
        let mut fw = FailureWindow::new();
        fail(2, &mut fw);
        assert_eq!(fw.consecutive_failures(), 2);
        fw.observe(ProbePairOutcome::AnyUdpSucceeded);
        assert_eq!(fw.consecutive_failures(), 0);
        assert!(!fw.threshold_met());
    }

    #[test]
    fn threshold_met_at_three_consecutive_failures() {
        let mut fw = FailureWindow::new();
        fail(2, &mut fw);
        assert!(!fw.threshold_met());
        fail(1, &mut fw);
        assert!(fw.threshold_met());
    }

    // --- HttpsFallbackState -----------------------------------------

    #[test]
    fn default_state_is_inactive() {
        let s = HttpsFallbackState::default();
        assert_eq!(s, HttpsFallbackState::Inactive);
        assert!(!s.is_active());
        assert!(!s.is_activating());
    }

    #[test]
    fn is_active_only_for_active() {
        assert!(!HttpsFallbackState::Inactive.is_active());
        assert!(!HttpsFallbackState::Activating.is_active());
        assert!(HttpsFallbackState::Active.is_active());
        assert!(!HttpsFallbackState::Failing.is_active());
    }

    #[test]
    fn is_activating_only_for_activating() {
        assert!(!HttpsFallbackState::Inactive.is_activating());
        assert!(HttpsFallbackState::Activating.is_activating());
        assert!(!HttpsFallbackState::Active.is_activating());
        assert!(!HttpsFallbackState::Failing.is_activating());
    }

    // --- transition table -------------------------------------------

    #[test]
    fn inactive_to_activating_after_three_failures() {
        let mut fw = FailureWindow::new();
        let mut state = HttpsFallbackState::Inactive;
        let bad = TransitionInput::Probe(ProbePairOutcome::BothUdpFailed);
        state = transition(state, &mut fw, bad);
        assert_eq!(state, HttpsFallbackState::Inactive);
        state = transition(state, &mut fw, bad);
        assert_eq!(state, HttpsFallbackState::Inactive);
        state = transition(state, &mut fw, bad);
        assert_eq!(state, HttpsFallbackState::Activating);
        // Window is reset on activation so the next entry starts
        // clean.
        assert_eq!(fw.consecutive_failures(), 0);
    }

    #[test]
    fn inactive_recovery_resets_window() {
        let mut fw = FailureWindow::new();
        let mut state = HttpsFallbackState::Inactive;
        let bad = TransitionInput::Probe(ProbePairOutcome::BothUdpFailed);
        let good = TransitionInput::Probe(ProbePairOutcome::AnyUdpSucceeded);
        state = transition(state, &mut fw, bad);
        state = transition(state, &mut fw, bad);
        assert_eq!(fw.consecutive_failures(), 2);
        state = transition(state, &mut fw, good);
        assert_eq!(state, HttpsFallbackState::Inactive);
        assert_eq!(fw.consecutive_failures(), 0);
    }

    #[test]
    fn activating_to_active_on_handshake_ok() {
        let mut fw = FailureWindow::new();
        let state = transition(
            HttpsFallbackState::Activating,
            &mut fw,
            TransitionInput::HandshakeOk,
        );
        assert_eq!(state, HttpsFallbackState::Active);
    }

    #[test]
    fn activating_to_failing_on_handshake_failed() {
        let mut fw = FailureWindow::new();
        let state = transition(
            HttpsFallbackState::Activating,
            &mut fw,
            TransitionInput::HandshakeFailed,
        );
        assert_eq!(state, HttpsFallbackState::Failing);
    }

    #[test]
    fn activating_ignores_probe_inputs() {
        let mut fw = FailureWindow::new();
        let bad = TransitionInput::Probe(ProbePairOutcome::BothUdpFailed);
        let state = transition(HttpsFallbackState::Activating, &mut fw, bad);
        assert_eq!(state, HttpsFallbackState::Activating);
    }

    #[test]
    fn active_reverts_to_inactive_when_udp_recovers() {
        let mut fw = FailureWindow::new();
        let good = TransitionInput::Probe(ProbePairOutcome::AnyUdpSucceeded);
        let state = transition(HttpsFallbackState::Active, &mut fw, good);
        assert_eq!(state, HttpsFallbackState::Inactive);
    }

    #[test]
    fn active_flips_to_failing_on_tunnel_lost() {
        let mut fw = FailureWindow::new();
        let state = transition(
            HttpsFallbackState::Active,
            &mut fw,
            TransitionInput::TunnelLost,
        );
        assert_eq!(state, HttpsFallbackState::Failing);
    }

    #[test]
    fn active_holds_on_both_udp_failed() {
        let mut fw = FailureWindow::new();
        let bad = TransitionInput::Probe(ProbePairOutcome::BothUdpFailed);
        let state = transition(HttpsFallbackState::Active, &mut fw, bad);
        assert_eq!(state, HttpsFallbackState::Active);
    }

    #[test]
    fn failing_recovers_to_inactive_on_udp_success() {
        let mut fw = FailureWindow::new();
        let good = TransitionInput::Probe(ProbePairOutcome::AnyUdpSucceeded);
        let state = transition(HttpsFallbackState::Failing, &mut fw, good);
        assert_eq!(state, HttpsFallbackState::Inactive);
    }

    #[test]
    fn failing_retries_activating_after_three_more_failures() {
        let mut fw = FailureWindow::new();
        let mut state = HttpsFallbackState::Failing;
        let bad = TransitionInput::Probe(ProbePairOutcome::BothUdpFailed);
        state = transition(state, &mut fw, bad);
        assert_eq!(state, HttpsFallbackState::Failing);
        state = transition(state, &mut fw, bad);
        assert_eq!(state, HttpsFallbackState::Failing);
        state = transition(state, &mut fw, bad);
        assert_eq!(state, HttpsFallbackState::Activating);
    }

    #[test]
    fn locked_failure_threshold_is_three() {
        assert_eq!(FAILURE_THRESHOLD, 3, "Q10 lock — changing this is a wire-protocol change");
    }

    #[test]
    fn end_to_end_walk_through_full_lifecycle() {
        // Inactive → 3 failures → Activating → HandshakeOk →
        // Active → UDP recovers → Inactive.
        let mut fw = FailureWindow::new();
        let mut state = HttpsFallbackState::Inactive;
        let bad = TransitionInput::Probe(ProbePairOutcome::BothUdpFailed);
        let good = TransitionInput::Probe(ProbePairOutcome::AnyUdpSucceeded);

        for _ in 0..3 {
            state = transition(state, &mut fw, bad);
        }
        assert_eq!(state, HttpsFallbackState::Activating);

        state = transition(state, &mut fw, TransitionInput::HandshakeOk);
        assert_eq!(state, HttpsFallbackState::Active);
        assert!(state.is_active());

        state = transition(state, &mut fw, good);
        assert_eq!(state, HttpsFallbackState::Inactive);
        assert!(!state.is_active());
    }

    #[test]
    fn end_to_end_handshake_failure_recovery_path() {
        let mut fw = FailureWindow::new();
        let mut state = HttpsFallbackState::Inactive;
        let bad = TransitionInput::Probe(ProbePairOutcome::BothUdpFailed);

        for _ in 0..3 {
            state = transition(state, &mut fw, bad);
        }
        // Handshake fails on first attempt → Failing.
        state = transition(state, &mut fw, TransitionInput::HandshakeFailed);
        assert_eq!(state, HttpsFallbackState::Failing);
        // Three more failures → retry.
        for _ in 0..3 {
            state = transition(state, &mut fw, bad);
        }
        assert_eq!(state, HttpsFallbackState::Activating);
        // Handshake succeeds this time.
        state = transition(state, &mut fw, TransitionInput::HandshakeOk);
        assert_eq!(state, HttpsFallbackState::Active);
    }
}
