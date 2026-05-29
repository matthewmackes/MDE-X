//! Shared library for the MDE installation manager (`mde-install`) and
//! updater (`mde-update`).
//!
//! INST-3 (v2.7). This is the **INST-3a** slice — the parts buildable
//! today with no upstream gaps:
//!
//! * [`profile`] — the three install profiles + parsing.
//! * [`confirm`] — TTY detection + typed-string confirms + the picker.
//! * [`intent_file`] — GlusterFS upgrade-intent barrier files (INST-10).
//! * [`wipe`] — local MDE-state wipe (config-path scope) + service
//!   control + the installed-profile marker.
//!
//! Deliberately **not** here yet:
//!
//! * `peer_registry` (INST-3b) — querying mackesd for per-peer
//!   `(hostname, version, last_seen)`. Blocked on **INST-PEERVER**:
//!   mackesd does not track per-peer RPM versions and exposes no such
//!   query. Building a client against a non-existent surface would
//!   violate §0.12 (no stubs), so it is split out.
//! * Nebula cert-revoke + GlusterFS brick-teardown (the re-install
//!   half of INST-7) — blocked on a mackesd `Ca.Revoke` method that
//!   does not exist. On a clean Fedora-Server build-up there is no
//!   cert or brick to tear down, so the [`wipe`] config-path path is
//!   the complete clean-install sequence.

pub mod confirm;
pub mod intent_file;
pub mod profile;
pub mod wipe;
