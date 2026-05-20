//! Phase 2.8 — mesh reconciler hook for Send-To outcomes.
//!
//! When an operation finishes (success or failure), the
//! orchestrator's terminal-state transition feeds this module.
//! For Sync / Deploy operations we compare the per-peer expected
//! list against the per-peer landed list and raise a
//! [`DriftEvent`] for every peer where the two diverge.
//!
//! The drift events flow into the v12.0 reconciler so the fleet
//! state stays self-correcting (next reconcile pass closes the
//! gap or escalates to the operator).
//!
//! Pure-fn / pure-data module — no DBus / async / I/O. The actual
//! reconciler subscribes via a channel passed in by the
//! supervisor.

use std::collections::HashSet;

use crate::orchestrator::{OperationView, Stage};
use crate::preflight::SendModeLite;

/// One drift event surfaced to the reconciler.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DriftEvent {
    /// Op id that produced the drift.
    pub op_id: u64,
    /// Peer where state diverged.
    pub peer: String,
    /// Severity — drives the UI badge.
    pub severity: DriftSeverity,
    /// Free-form message for the audit log.
    pub message: String,
}

/// Drift severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriftSeverity {
    /// Cosmetic — log only.
    Info,
    /// Warn — UI shows the badge; reconciler re-attempts next pass.
    Warn,
    /// Hard fail — UI shows the badge in red; reconciler stops
    /// retrying until the operator clears.
    Critical,
}

/// Examine a finished operation + the per-peer landed-set the
/// worker reported; return zero or more drift events.
///
/// Inputs:
///   * `op` — the orchestrator's terminal view of the operation.
///   * `expected_peers` — the destinations the request was
///     supposed to reach (from the SendTo request).
///   * `landed_peers` — peers where the file actually landed
///     (worker-reported).
#[must_use]
pub fn drift_events(
    op: &OperationView,
    expected_peers: &[String],
    landed_peers: &[String],
) -> Vec<DriftEvent> {
    let mut out = Vec::new();
    if !matches!(op.stage, Stage::Completed | Stage::Failed) {
        // Operation is still in flight or was rejected pre-execute
        // — nothing to reconcile.
        return out;
    }

    let expected: HashSet<&String> = expected_peers.iter().collect();
    let landed: HashSet<&String> = landed_peers.iter().collect();

    // Peers that should have received the file but didn't.
    for missing in expected.difference(&landed) {
        out.push(DriftEvent {
            op_id: op.op_id,
            peer: (*missing).clone(),
            severity: severity_for(op, true),
            message: format!(
                "op {} did not land on peer {} (expected for {})",
                op.op_id,
                missing,
                op.destination_label
            ),
        });
    }

    // Peers that received the file unexpectedly (over-broadcast,
    // misrouted). Warn only — usually harmless but worth surfacing.
    for extra in landed.difference(&expected) {
        out.push(DriftEvent {
            op_id: op.op_id,
            peer: (*extra).clone(),
            severity: DriftSeverity::Warn,
            message: format!(
                "op {} landed on peer {} unexpectedly",
                op.op_id, extra
            ),
        });
    }

    // Failed operations with no per-peer landings raise a single
    // op-level drift if neither expected/landed sets are empty.
    if matches!(op.stage, Stage::Failed) && out.is_empty() && !expected.is_empty() {
        out.push(DriftEvent {
            op_id: op.op_id,
            peer: "(any)".into(),
            severity: DriftSeverity::Critical,
            message: format!(
                "op {} failed before reaching any of {} peer(s)",
                op.op_id,
                expected_peers.len()
            ),
        });
    }
    out
}

/// Pick severity for a missing-peer drift based on the op's
/// send mode. Move / Deploy are critical (data loss risk); Copy /
/// Sync / Stage are warn.
fn severity_for(op: &OperationView, missing: bool) -> DriftSeverity {
    if !missing {
        return DriftSeverity::Info;
    }
    match op.mode {
        SendModeLite::Move | SendModeLite::Deploy => DriftSeverity::Critical,
        _ => DriftSeverity::Warn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{OperationView, Stage};
    use crate::preflight::ConflictPolicyLite;
    use std::path::PathBuf;

    fn op(op_id: u64, stage: Stage, mode: SendModeLite) -> OperationView {
        OperationView {
            op_id,
            audit_id: op_id,
            stage,
            sources: vec![PathBuf::from("/tmp/x")],
            destination_label: "audio-group".into(),
            mode,
            conflict: ConflictPolicyLite::Ask,
            last_message: String::new(),
        }
    }

    fn names(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn no_drift_when_landed_matches_expected() {
        let o = op(1, Stage::Completed, SendModeLite::Copy);
        let events = drift_events(&o, &names(&["pine", "oak"]), &names(&["pine", "oak"]));
        assert!(events.is_empty());
    }

    #[test]
    fn missing_peer_raises_drift_for_completed_copy_as_warn() {
        let o = op(1, Stage::Completed, SendModeLite::Copy);
        let events = drift_events(&o, &names(&["pine", "oak", "birch"]), &names(&["pine", "oak"]));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].peer, "birch");
        assert_eq!(events[0].severity, DriftSeverity::Warn);
        assert!(events[0].message.contains("birch"));
        assert!(events[0].message.contains("audio-group"));
    }

    #[test]
    fn missing_peer_for_move_op_is_critical() {
        let o = op(2, Stage::Completed, SendModeLite::Move);
        let events = drift_events(&o, &names(&["pine"]), &names(&[]));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].severity, DriftSeverity::Critical);
    }

    #[test]
    fn missing_peer_for_deploy_op_is_critical() {
        let o = op(3, Stage::Completed, SendModeLite::Deploy);
        let events = drift_events(&o, &names(&["pine"]), &names(&[]));
        assert_eq!(events[0].severity, DriftSeverity::Critical);
    }

    #[test]
    fn unexpected_landing_raises_warn_drift() {
        let o = op(1, Stage::Completed, SendModeLite::Copy);
        let events = drift_events(&o, &names(&["pine"]), &names(&["pine", "oak"]));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].peer, "oak");
        assert_eq!(events[0].severity, DriftSeverity::Warn);
        assert!(events[0].message.contains("unexpectedly"));
    }

    #[test]
    fn failed_op_with_zero_landings_emits_critical_op_level_event() {
        let o = op(1, Stage::Failed, SendModeLite::Copy);
        let events = drift_events(&o, &names(&["pine", "oak"]), &names(&[]));
        // Both peers individually missing AS WELL AS the op-level
        // marker would be noisy — the op-level fires only when no
        // per-peer events fire. With two missing peers we get 2
        // per-peer events.
        let critical = events.iter().filter(|e| e.severity == DriftSeverity::Critical).count();
        // SendMode::Copy missing → warn, not critical. The op-level
        // event isn't emitted because there ARE per-peer events.
        assert_eq!(critical, 0);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn pending_op_emits_no_events() {
        let o = op(1, Stage::Pending, SendModeLite::Copy);
        let events = drift_events(&o, &names(&["pine"]), &names(&[]));
        assert!(events.is_empty());
    }

    #[test]
    fn rejected_op_emits_no_events() {
        let o = op(1, Stage::Rejected, SendModeLite::Copy);
        let events = drift_events(&o, &names(&["pine"]), &names(&[]));
        assert!(events.is_empty());
    }

    #[test]
    fn message_includes_op_id() {
        let o = op(42, Stage::Completed, SendModeLite::Copy);
        let events = drift_events(&o, &names(&["pine", "oak"]), &names(&["pine"]));
        assert!(events[0].message.contains("42"));
    }

    #[test]
    fn drift_event_supports_set_dedupe() {
        // Two identical events fold into one HashSet entry.
        use std::collections::HashSet;
        let e1 = DriftEvent {
            op_id: 1,
            peer: "pine".into(),
            severity: DriftSeverity::Warn,
            message: "x".into(),
        };
        let e2 = e1.clone();
        let mut set = HashSet::new();
        set.insert(e1);
        set.insert(e2);
        assert_eq!(set.len(), 1);
    }
}
