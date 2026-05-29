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
//! * [`peers`] — peer registry + version-skew (PEERVER-3, closes
//!   INST-3b). Reads the converged peer-data from the GFS-replicated
//!   `<mesh-home>/peers/` dir (no mackesd/D-Bus/Bus dependency), per
//!   `docs/design/v2.7-peer-data-convergence.md`.
//!
//! Deliberately **not** here yet:
//!
//! * Nebula cert-revoke + GlusterFS brick-teardown (the re-install
//!   half of INST-7) — blocked on a mackesd `Ca.Revoke` method that
//!   does not exist. On a clean Fedora-Server build-up there is no
//!   cert or brick to tear down, so the [`wipe`] config-path path is
//!   the complete clean-install sequence.

pub mod confirm;
pub mod intent_file;
pub mod peers;
pub mod profile;
pub mod smoke;
pub mod wipe;
