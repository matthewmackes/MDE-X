//! HYP-8.5 — operator-edited configuration files mded reads at
//! startup.
//!
//! Each submodule owns one config-file family:
//!
//! - [`tag_manifest`] — `~/.config/mde/tags/<name>.toml` per-tag
//!   compositor + UX policy. Source of truth for HYP-9 / HYP-10 /
//!   HYP-11 / HYP-12 / HYP-14 / HYP-22 + the Portal-* tag-aware
//!   features.
//!
//! Future submodules (per the v6.5 roadmap) will sit alongside
//! `tag_manifest` rather than scattered across the workers tree.

pub mod tag_manifest;

pub use tag_manifest::{
    default_manifests_dir, load_all as load_tag_manifests, parse_file as parse_tag_manifest,
    system_manifests_dir, LoadError as TagManifestLoadError, TagManifest,
};
