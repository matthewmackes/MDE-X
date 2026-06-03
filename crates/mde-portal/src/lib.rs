//! Public library surface for the `mde-portal` crate.
//!
//! Most of the portal lives inside the `mde-portal` binary (`main.rs`)
//! and the `mde-portal-full` binary (`portal_full_main.rs`). This
//! library file exists so external binaries — currently just
//! `mde-open` (Portal-35) — can reuse the `mde://` URI parser without
//! duplicating the source. (DBUS-2 retired the `dev.mackes.MDE.Portal`
//! D-Bus proxy that used to live here; mde-open now publishes the
//! parsed URI to the Bus at `action/shell/open-uri`.)

#![forbid(unsafe_code)]

/// `mde://` URI scheme parser (Portal-35).
pub mod uri;

/// Workspace template → swaymsg batch emitter (SWAY-5 / Portal-51).
pub mod template;
