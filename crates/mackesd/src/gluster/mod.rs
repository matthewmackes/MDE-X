//! GF-1.3.b + future GF-2.x (v5.0.0) — GlusterFS management.
//!
//! The first inhabitant is [`bind`], the helper that rewrites
//! `/etc/glusterfs/glusterd.vol` so glusterd binds to the
//! local Nebula overlay IP instead of `0.0.0.0`. Future
//! commits will add a `gluster_worker` submodule + the
//! `bootstrap_or_join` logic per the
//! `docs/design/v5.0.0-gluster-mesh-home.md` lock.

pub mod bind;
