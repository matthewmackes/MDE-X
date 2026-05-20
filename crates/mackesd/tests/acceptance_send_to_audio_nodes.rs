//! Phase 6.5 — acceptance scenario.
//!
//! Locked scenario (per the v2.0.0 mde-files design spec):
//!
//!   "User right-clicks a file, picks **Send To → Audio Nodes**;
//!    mded validates, transfers, verifies checksum, shows per-peer
//!    progress, writes audit trail, updates mesh state, offers
//!    rollback."
//!
//! Implementation walks the orchestrator state machine end-to-end
//! against an in-process mded surface:
//!
//!   1. Set up a `PathPolicy` with one allowed root.
//!   2. Place a source file in the root.
//!   3. Build the Send-To request (mode=Copy, conflict=Ask,
//!      destination=audio-group, two peers: pine, oak).
//!   4. `Orchestrator::accept` (runs path-safety + pre-flight,
//!      allocates op id).
//!   5. Walk Pending → Validating → Executing → Verifying →
//!      Completed via four `advance(_, false, ...)` calls.
//!   6. Feed the terminal view + landed-peer set to
//!      `reconciler_hook::drift_events` — expect zero drift on the
//!      happy path.
//!   7. Pull the events log; assert exactly one event per stage
//!      transition with the op id we got back from accept().
//!
//! Plus a sad-path companion that loses one of the two peers
//! mid-transfer + verifies the reconciler raises a single Warn
//! drift on the missing one.

#![cfg(feature = "async-services")]

use std::fs;
use std::path::PathBuf;

use mackesd_core::orchestrator::{Orchestrator, Stage};
use mackesd_core::path_safety::{AllowedRoot, PathPolicy};
use mackesd_core::preflight::{ConflictPolicyLite, Request, SendModeLite};
use mackesd_core::reconciler_hook::{drift_events, DriftSeverity};

use tempfile::tempdir;

fn audio_request(src: PathBuf) -> Request {
    Request {
        sources: vec![src],
        destination_label: "audio-group".into(),
        total_bytes: 1024,
        destination_free_bytes: 1_000_000_000,
        destination_last_seen_ms: 1_500,
        rollback_available_for_target: true,
        target_exists: false,
        mode: SendModeLite::Copy,
        conflict: ConflictPolicyLite::Ask,
    }
}

#[test]
fn user_sends_file_to_audio_nodes_end_to_end_happy_path() {
    let tmp = tempdir().expect("tmpdir");
    let mut policy = PathPolicy::empty();
    policy.allow(AllowedRoot::new(tmp.path(), "scratch").expect("canonicalise"));
    let src = tmp.path().join("track-01.flac");
    fs::write(&src, b"FLaC...").expect("write source");

    // The user right-clicks the file and picks Send To → Audio
    // Nodes. The UI translates that into a Send-To request.
    let request = audio_request(src.clone());

    // mded accepts the request — path-safety + pre-flight pass.
    let mut orchestrator = Orchestrator::new();
    let op_id = orchestrator.accept(request, &policy).expect("accept");
    assert_eq!(orchestrator.operation(op_id).unwrap().stage, Stage::Pending);

    // Walk the state machine through to Completed. Each step
    // would normally be driven by a worker after IO completes —
    // here we drive them inline.
    assert_eq!(orchestrator.advance(op_id, false, "").unwrap(), Stage::Validating);
    assert_eq!(orchestrator.advance(op_id, false, "").unwrap(), Stage::Executing);
    assert_eq!(orchestrator.advance(op_id, false, "").unwrap(), Stage::Verifying);
    assert_eq!(orchestrator.advance(op_id, false, "").unwrap(), Stage::Completed);

    // The audit trail: 5 events (Pending + 4 transitions), all
    // tied to the same op id.
    let events = orchestrator.events();
    assert_eq!(events.len(), 5, "audit trail must carry every transition");
    assert_eq!(events.iter().filter(|e| e.op_id == op_id).count(), 5);
    assert_eq!(events[0].stage, Stage::Pending);
    assert_eq!(events[1].stage, Stage::Validating);
    assert_eq!(events[2].stage, Stage::Executing);
    assert_eq!(events[3].stage, Stage::Verifying);
    assert_eq!(events[4].stage, Stage::Completed);

    // Reconciler hook: file landed on both peers, no drift.
    let op = orchestrator.operation(op_id).unwrap();
    let expected_peers = vec!["pine".to_string(), "oak".to_string()];
    let landed_peers = vec!["pine".to_string(), "oak".to_string()];
    let drift = drift_events(op, &expected_peers, &landed_peers);
    assert!(drift.is_empty(), "happy path must produce no drift events");

    // Source file still exists (Copy mode, not Move).
    assert!(src.exists(), "Copy mode must leave source in place");
}

#[test]
fn user_send_with_one_unreachable_peer_raises_warn_drift() {
    let tmp = tempdir().expect("tmpdir");
    let mut policy = PathPolicy::empty();
    policy.allow(AllowedRoot::new(tmp.path(), "scratch").expect("canonicalise"));
    let src = tmp.path().join("track-02.flac");
    fs::write(&src, b"FLaC...").expect("write source");

    let request = audio_request(src);
    let mut orchestrator = Orchestrator::new();
    let op_id = orchestrator.accept(request, &policy).expect("accept");
    for _ in 0..4 {
        orchestrator.advance(op_id, false, "").unwrap();
    }
    assert_eq!(orchestrator.operation(op_id).unwrap().stage, Stage::Completed);

    // Worker reports: file landed on pine but not oak.
    let op = orchestrator.operation(op_id).unwrap();
    let drift = drift_events(
        op,
        &["pine".to_string(), "oak".to_string()],
        &["pine".to_string()],
    );
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].peer, "oak");
    assert_eq!(drift[0].severity, DriftSeverity::Warn);
    assert!(drift[0].message.contains("audio-group"));
}

#[test]
fn user_send_blocked_by_preflight_never_reaches_pending() {
    let tmp = tempdir().expect("tmpdir");
    let mut policy = PathPolicy::empty();
    policy.allow(AllowedRoot::new(tmp.path(), "scratch").expect("canonicalise"));
    let src = tmp.path().join("track-03.flac");
    fs::write(&src, b"FLaC...").expect("write source");

    // Block on disk-space.
    let mut request = audio_request(src);
    request.total_bytes = 1_000_000;
    request.destination_free_bytes = 1_000;

    let mut orchestrator = Orchestrator::new();
    let err = orchestrator.accept(request, &policy).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("disk-space"), "block reason must mention disk-space");
    // No op id allocated → orchestrator is still empty.
    assert!(orchestrator.is_empty());
}

#[test]
fn user_send_with_execute_failure_lands_in_failed_state() {
    let tmp = tempdir().expect("tmpdir");
    let mut policy = PathPolicy::empty();
    policy.allow(AllowedRoot::new(tmp.path(), "scratch").expect("canonicalise"));
    let src = tmp.path().join("track-04.flac");
    fs::write(&src, b"FLaC...").expect("write source");

    let request = audio_request(src);
    let mut orchestrator = Orchestrator::new();
    let op_id = orchestrator.accept(request, &policy).expect("accept");
    orchestrator.advance(op_id, false, "").unwrap(); // → Validating
    orchestrator.advance(op_id, false, "").unwrap(); // → Executing
    // Worker reports the transfer crashed.
    let stage = orchestrator
        .advance(op_id, true, "network broke")
        .unwrap();
    assert_eq!(stage, Stage::Failed);
    let op = orchestrator.operation(op_id).unwrap();
    assert_eq!(op.last_message, "network broke");

    // Reconciler raises a critical for Copy-mode complete loss
    // when both peers missing → 2 per-peer warns (Copy doesn't
    // promote to critical).
    let drift = drift_events(
        op,
        &["pine".to_string(), "oak".to_string()],
        &[],
    );
    assert_eq!(drift.len(), 2);
    for ev in &drift {
        assert_eq!(ev.severity, DriftSeverity::Warn);
    }
}
