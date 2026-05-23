//! mde-applet-sway-cluster — SPLIT / LAYOUT / WINDOW chips
//! sourced from `swaymsg -t get_tree` + `get_workspaces`.
//!
//! Phase E.4.1 follow-up — the panel Cluster zone displays
//! three glyphs at 12px each, separated by 6px of negative
//! space:
//!
//! - **SPLIT** chip: focused container's split direction
//!   (`H` / `V`) or `T` / `S` for tabbed / stacked.
//! - **LAYOUT** chip: workspace-level layout (`def` for default,
//!   `tab` for fullscreen-tabbed, `stk` for fullscreen-stacked).
//! - **WINDOW** chip: focused window's `con_id` (or `—` when
//!   the workspace is empty).
//!
//! The data layer is `pub fn parse_get_tree_focus(json)` which
//! consumes the same JSON sway emits and walks down to the
//! focused leaf. The applet binary spawns `swaymsg -t get_tree
//! --pretty=false`, parses the output, formats the chip row,
//! and exits 0 — same pattern as the other status-strip applets.

#![forbid(unsafe_code)]

use serde::Deserialize;

/// One row of cluster output — three short chip strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterRow {
    pub split: String,
    pub layout: String,
    pub window: String,
}

impl ClusterRow {
    /// Render the row as a single space-joined chip string.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}  {}  {}", self.split, self.layout, self.window)
    }

    /// Empty row — used when there's no focused workspace.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            split: "—".into(),
            layout: "—".into(),
            window: "—".into(),
        }
    }
}

/// Sway `get_tree` node — pruned to the fields we walk.
#[derive(Debug, Deserialize)]
struct Node {
    #[serde(default)]
    focused: bool,
    #[serde(default)]
    layout: String,
    #[serde(default)]
    id: u64,
    #[serde(default, rename = "type")]
    node_type: String,
    #[serde(default)]
    nodes: Vec<Node>,
    #[serde(default)]
    floating_nodes: Vec<Node>,
}

/// Pure parser — walks `swaymsg -t get_tree` JSON output to
/// the focused leaf and returns the ClusterRow.
#[must_use]
pub fn parse_get_tree_focus(json: &str) -> ClusterRow {
    let Ok(root) = serde_json::from_str::<Node>(json) else {
        return ClusterRow::empty();
    };
    let path = focused_path(&root);
    if path.is_empty() {
        return ClusterRow::empty();
    }
    let focused_leaf = path.last().copied().unwrap_or(&root);

    let workspace = path
        .iter()
        .copied()
        .find(|n| n.node_type == "workspace")
        .unwrap_or(&root);

    let parent_container = path
        .iter()
        .rev()
        .copied()
        .skip(1)
        .find(|n| n.node_type == "con")
        .unwrap_or(focused_leaf);

    ClusterRow {
        split: split_glyph(&parent_container.layout),
        layout: layout_glyph(&workspace.layout),
        window: format!("#{}", focused_leaf.id),
    }
}

fn focused_path(root: &Node) -> Vec<&Node> {
    let mut path = Vec::new();
    walk(root, &mut path);
    path
}

fn walk<'a>(node: &'a Node, path: &mut Vec<&'a Node>) -> bool {
    path.push(node);
    if node.focused {
        return true;
    }
    for child in node.nodes.iter().chain(node.floating_nodes.iter()) {
        if walk(child, path) {
            return true;
        }
    }
    path.pop();
    false
}

/// Pure helper — sway's `splith` / `splitv` / `tabbed` /
/// `stacked` → one-glyph chip.
///
/// v4.0.1 BUG-3: sway emits `"none"` for leaf cons that aren't
/// themselves a split container — that's the common case for a
/// single focused window. The pre-v4.0.1 implementation fell
/// through to `"?"` which surfaced as "? def #N" in the panel
/// cluster and read like a broken render to operators. `"none"`
/// now collapses to the em-dash placeholder, same as the
/// empty-string branch.
#[must_use]
pub fn split_glyph(layout: &str) -> String {
    match layout {
        "splith" => "H".into(),
        "splitv" => "V".into(),
        "tabbed" => "T".into(),
        "stacked" => "S".into(),
        "none" => "—".into(),
        other if other.is_empty() => "—".into(),
        _ => "?".into(),
    }
}

/// Pure helper — workspace-level `output` / `splith` / `splitv`
/// / `tabbed` / `stacked` → 3-char chip.
#[must_use]
pub fn layout_glyph(layout: &str) -> String {
    match layout {
        "splith" | "splitv" | "output" => "def".into(),
        "tabbed" => "tab".into(),
        "stacked" => "stk".into(),
        other if other.is_empty() => "—".into(),
        _ => "?".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_row_uses_em_dashes() {
        let r = ClusterRow::empty();
        assert_eq!(r.split, "—");
        assert_eq!(r.layout, "—");
        assert_eq!(r.window, "—");
    }

    #[test]
    fn render_joins_three_chips_with_double_spaces() {
        let r = ClusterRow {
            split: "H".into(),
            layout: "def".into(),
            window: "#42".into(),
        };
        assert_eq!(r.render(), "H  def  #42");
    }

    #[test]
    fn split_glyph_maps_known_layouts() {
        assert_eq!(split_glyph("splith"), "H");
        assert_eq!(split_glyph("splitv"), "V");
        assert_eq!(split_glyph("tabbed"), "T");
        assert_eq!(split_glyph("stacked"), "S");
    }

    #[test]
    fn split_glyph_handles_empty_and_unknown() {
        assert_eq!(split_glyph(""), "—");
        assert_eq!(split_glyph("weird"), "?");
    }

    #[test]
    fn split_glyph_renders_none_as_em_dash() {
        // v4.0.1 BUG-3 regression — sway emits "none" for leaf
        // cons under non-tabbed/stacked workspaces, which used
        // to fall through to "?" and surfaced as "? def #N" on
        // the panel cluster.
        assert_eq!(split_glyph("none"), "—");
    }

    #[test]
    fn layout_glyph_collapses_splits_to_def() {
        assert_eq!(layout_glyph("splith"), "def");
        assert_eq!(layout_glyph("splitv"), "def");
        assert_eq!(layout_glyph("output"), "def");
    }

    #[test]
    fn layout_glyph_keeps_tabbed_and_stacked_short() {
        assert_eq!(layout_glyph("tabbed"), "tab");
        assert_eq!(layout_glyph("stacked"), "stk");
    }

    #[test]
    fn parse_handles_garbage_json() {
        let row = parse_get_tree_focus("not json");
        assert_eq!(row, ClusterRow::empty());
    }

    #[test]
    fn parse_handles_no_focused_window() {
        let json = r#"{
            "id": 1, "type": "root", "layout": "splith", "focused": false,
            "nodes": [], "floating_nodes": []
        }"#;
        let row = parse_get_tree_focus(json);
        assert_eq!(row, ClusterRow::empty());
    }

    #[test]
    fn parse_walks_to_focused_leaf() {
        // Root → output → workspace → con → leaf (focused).
        let json = r#"{
            "id": 1, "type": "root", "layout": "splith",
            "nodes": [{
                "id": 2, "type": "output", "layout": "output",
                "nodes": [{
                    "id": 3, "type": "workspace", "layout": "splith",
                    "nodes": [{
                        "id": 4, "type": "con", "layout": "splith",
                        "nodes": [{
                            "id": 99, "type": "con", "layout": "none",
                            "focused": true,
                            "nodes": [], "floating_nodes": []
                        }],
                        "floating_nodes": []
                    }],
                    "floating_nodes": []
                }],
                "floating_nodes": []
            }],
            "floating_nodes": []
        }"#;
        let row = parse_get_tree_focus(json);
        assert_eq!(row.split, "H"); // parent con's layout
        assert_eq!(row.layout, "def"); // workspace layout
        assert_eq!(row.window, "#99"); // focused leaf id
    }

    #[test]
    fn parse_picks_up_tabbed_workspace() {
        let json = r#"{
            "id": 1, "type": "root", "layout": "splith",
            "nodes": [{
                "id": 3, "type": "workspace", "layout": "tabbed",
                "nodes": [{
                    "id": 99, "type": "con", "layout": "splitv",
                    "focused": true,
                    "nodes": [], "floating_nodes": []
                }],
                "floating_nodes": []
            }],
            "floating_nodes": []
        }"#;
        let row = parse_get_tree_focus(json);
        assert_eq!(row.layout, "tab");
        assert_eq!(row.window, "#99");
    }
}
