//! Peer-probe schema — back-compat re-export from
//! `mde_mesh_types::peer_probe` (PC-2 production home).
//!
//! Until PC-2 (2026-05-21) the schema lived here as a
//! placeholder per PC-1's acceptance. It now lives in
//! `mackes-mesh-types` so cross-crate consumers (mded's
//! peer-join worker, future tooling) share a single
//! definition. This module re-exports the canonical types so
//! existing call sites (`use mde_peer_card::probe::PeerProbe`)
//! continue to work without churn.

pub use mde_mesh_types::peer_probe::{
    BusTopology, Descriptors, KernelDriver, NatClass, PeerProbe, PowerThermal,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn re_export_resolves_peer_probe() {
        // PC-2 sanity — `mde_peer_card::probe::PeerProbe` still
        // resolves to the canonical type after the move.
        let p = PeerProbe::fixture();
        assert_eq!(p.peer_id, "fixture-peer-1");
    }
}
