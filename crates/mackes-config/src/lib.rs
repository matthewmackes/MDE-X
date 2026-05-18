//! Serde schema for `~/.config/mackes-panel/panel.toml`.
//!
//! Per the 50-question lock (Q18–Q22), this file lives in TOML, is mesh-
//! replicated whole-file via QNM-Shared, and is hot-reloaded by inotify
//! diff-and-apply. This crate carries the schema only — I/O and inotify
//! watching live in `mackes-panel`.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// Root document of `panel.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelConfig {
    /// Top status bar configuration (20 px, monochrome Carbon, Q13/Q14).
    #[serde(default)]
    pub top_bar: TopBarConfig,

    /// Bottom dock configuration (48 px icons, no magnification, Q11/Q12).
    #[serde(default)]
    pub dock: DockConfig,

    /// Mesh-sync behavior for this very file (Q18–Q21).
    #[serde(default)]
    pub mesh: MeshConfig,
}

/// What lives in the top bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopBarConfig {
    /// Ordered list of status-cluster items rendered on the right
    /// (e.g. `["mesh", "clipboard", "volume", "battery", "notifications", "user"]`).
    #[serde(default = "default_status_items")]
    pub status_items: Vec<String>,

    /// Whether to render the global appmenu (`DBusMenu`) on the left.
    #[serde(default = "true_default")]
    pub appmenu: bool,
}

impl Default for TopBarConfig {
    fn default() -> Self {
        Self {
            status_items: default_status_items(),
            appmenu: true,
        }
    }
}

/// What lives in the dock, in render order. Apps and mesh resources are
/// interleaved per Q10 — no segmentation, no separator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DockConfig {
    /// Ordered list of dock entries. Each entry is either a `.desktop`
    /// reference or a `MeshResource` ID.
    #[serde(default)]
    pub items: Vec<DockItem>,
}

/// One slot in the dock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DockItem {
    /// A pinned application by `.desktop` ID
    /// (e.g. `firefox.desktop`, `org.gnome.Terminal.desktop`).
    App {
        /// The basename of the `.desktop` file under `applications/`.
        desktop: String,
    },

    /// A mesh resource by its `MeshResource::id()`
    /// (e.g. `peer:anvil`, `share:anvil:code`).
    Mesh {
        /// Stable ID emitted by `mackes_mesh_types::MeshResource::id()`.
        id: String,
    },
}

/// Mesh-sync behavior for the panel.toml file itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshConfig {
    /// Whether to mirror this file to `~/.qnm-sync/mackes-panel/panel.toml`
    /// (Q18 — defaults to true).
    #[serde(default = "true_default")]
    pub replicate: bool,

    /// How often to hash-compare against the peer mirrors. 0 disables drift
    /// detection. Default 300 (5 min) per Q22.
    #[serde(default = "default_drift_seconds")]
    pub drift_check_seconds: u32,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            replicate: true,
            drift_check_seconds: default_drift_seconds(),
        }
    }
}

const fn true_default() -> bool {
    true
}

const fn default_drift_seconds() -> u32 {
    300
}

fn default_status_items() -> Vec<String> {
    [
        "mesh",
        "clipboard",
        "volume",
        "battery",
        "notifications",
        "user",
    ]
    .iter()
    .map(|&s| s.to_owned())
    .collect()
}

/// Parse a `panel.toml` document. Unknown fields are silently dropped so
/// older binaries don't blow up on newer files.
///
/// # Errors
/// Returns the underlying `toml::de::Error` if the document is malformed.
pub fn parse(text: &str) -> Result<PanelConfig, toml::de::Error> {
    toml::from_str(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_document_yields_defaults() {
        let cfg = parse("").expect("empty doc should parse");
        assert!(cfg.top_bar.appmenu);
        assert_eq!(cfg.top_bar.status_items.len(), 6);
        assert!(cfg.mesh.replicate);
        assert_eq!(cfg.mesh.drift_check_seconds, 300);
        assert!(cfg.dock.items.is_empty());
    }

    #[test]
    fn dock_items_round_trip() {
        let doc = r"
            [[dock.items]]
            kind    = 'app'
            desktop = 'firefox.desktop'

            [[dock.items]]
            kind = 'mesh'
            id   = 'peer:anvil'
        ";
        let cfg = parse(doc).expect("parse");
        assert_eq!(cfg.dock.items.len(), 2);
        assert_eq!(
            cfg.dock.items[0],
            DockItem::App {
                desktop: "firefox.desktop".into()
            }
        );
        assert_eq!(
            cfg.dock.items[1],
            DockItem::Mesh {
                id: "peer:anvil".into()
            }
        );
    }

    #[test]
    fn unknown_top_level_keys_are_tolerated() {
        let doc = r"
            [unknown_section]
            x = 1
        ";
        // Should NOT error. Unknown sections at the top level are ignored
        // by serde unless we explicitly forbid them (we don't).
        let _ = parse(doc).expect("tolerate unknown sections");
    }

    #[test]
    fn mesh_drift_can_be_disabled() {
        let doc = r"
            [mesh]
            drift_check_seconds = 0
        ";
        let cfg = parse(doc).expect("parse");
        assert_eq!(cfg.mesh.drift_check_seconds, 0);
        assert!(cfg.mesh.replicate); // default still applied
    }
}
