//! Phase 12.1.2 — deploy layer.
//!
//! Houses the fleet-deploy pipeline: revision push, Ansible-pull
//! trigger, drift-recovery, rollback. Sits on top of the
//! `service::` traits + writes to the `store::` SQLite layer.
//!
//! Phase 12.1.2 only ships the directory layout; concrete deploy
//! sub-modules land as each phase G item arrives:
//!
//!   * `push.rs` — fleet revision push (Phase G.2).
//!   * `rollback.rs` — per-revision rollback (Phase G.3 + 2.7
//!     audit-store).
//!   * `ansible_pull.rs` — Ansible-pull trigger hook (lives in
//!     `workers::ansible_pull` for the worker body; the
//!     orchestration policy lives here).
