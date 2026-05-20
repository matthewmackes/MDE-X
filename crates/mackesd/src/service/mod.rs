//! Phase 12.1.2 — service layer.
//!
//! Houses the cross-cutting "service" traits + their default
//! impls. Today's mackesd has its surfaces scattered across the
//! flat module tree (`policy`, `store`, `topology`, …). This
//! subdir is the new home for traits that combine those surfaces:
//!
//!   * Read-side facades the panel + CLI consume.
//!   * Write-side facades the reconcile loop + the IPC layer
//!     consume.
//!   * Default trait impls that wire the existing modules
//!     together so new consumers don't have to know about every
//!     concrete type.
//!
//! Phase 12.1.2 only ships the directory layout + the layout
//! contract; the actual trait surfaces land as each cross-cutting
//! concern arrives (Phase F.x for the panel reads, Phase G.x for
//! the fleet writes, Phase 2.x for the Send-To pipeline). New
//! traits go here in one file per public surface.
