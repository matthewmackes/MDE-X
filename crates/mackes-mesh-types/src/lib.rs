//! Shared mesh-resource types for `mackes-panel`.
//!
//! A `MeshResource` is anything the mackes mesh exposes that can be rendered
//! as a first-class dock item — a peer, a mounted share, or an advertised
//! service. Per the 50-question lock (Q9 / Q10 / Q33), these interleave
//! with apps in the bottom dock.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// One thing the mesh exposes that the panel can render as a dock item.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MeshResource {
    /// A mesh peer (Headscale-known machine). Click → action popover (Q34):
    /// Files / SSH / RDP / VNC / Services / Send file.
    Peer {
        /// Hostname / mesh node name. Stable across reboots.
        name: String,
        /// Mesh IP (Tailscale-assigned 100.x.x.x).
        mesh_ip: Option<String>,
        /// Whether the peer has been seen as online in the last poll.
        online: bool,
    },

    /// A QNM-Shared bucket exposed by a peer. Click → Thunar at the share.
    MountedShare {
        /// Owning peer's name.
        peer: String,
        /// Bucket path under `~/QNM-Shared/`.
        bucket: String,
    },

    /// A service the mesh advertises (Sublime Music, Delfin, Caddy, …).
    /// Click → opens the service's URL or launches its client.
    Service {
        /// Owning peer's name (or `local` if this peer hosts it).
        peer: String,
        /// Service slug (`sublime-music`, `delfin`, `caddy`, …).
        slug: String,
        /// Service URL the dock click should open.
        url: String,
    },
}

impl MeshResource {
    /// Stable identifier used to look up the resource's Carbon icon
    /// and to dedupe entries in the dock's pin list.
    #[must_use]
    pub fn id(&self) -> String {
        match self {
            Self::Peer { name, .. } => format!("peer:{name}"),
            Self::MountedShare { peer, bucket } => format!("share:{peer}:{bucket}"),
            Self::Service { peer, slug, .. } => format!("svc:{peer}:{slug}"),
        }
    }

    /// Human-readable label rendered in the dock tooltip.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Peer {
                name, online: true, ..
            } => format!("{name} (online)"),
            Self::Peer {
                name,
                online: false,
                ..
            } => format!("{name} (offline)"),
            Self::MountedShare { peer, bucket } => format!("{peer}: {bucket}"),
            Self::Service { peer, slug, .. } => format!("{peer}: {slug}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_id_is_stable() {
        let r = MeshResource::Peer {
            name: "anvil".into(),
            mesh_ip: Some("100.64.0.7".into()),
            online: true,
        };
        assert_eq!(r.id(), "peer:anvil");
    }

    #[test]
    fn service_id_includes_peer_and_slug() {
        let r = MeshResource::Service {
            peer: "anvil".into(),
            slug: "sublime-music".into(),
            url: "http://anvil.mesh:4040".into(),
        };
        assert_eq!(r.id(), "svc:anvil:sublime-music");
    }

    #[test]
    fn label_reflects_online_state() {
        let online = MeshResource::Peer {
            name: "anvil".into(),
            mesh_ip: None,
            online: true,
        };
        let offline = MeshResource::Peer {
            name: "anvil".into(),
            mesh_ip: None,
            online: false,
        };
        assert!(online.label().contains("online"));
        assert!(offline.label().contains("offline"));
    }
}
