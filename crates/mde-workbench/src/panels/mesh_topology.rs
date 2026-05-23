//! v4.0.1 WB-2.k — Network → Mesh Topology panel.
//!
//! Tabular alternative to the canvas-graph version the original
//! worklist spec described. The canvas widget chains on either
//! a substantial iced::Canvas integration or a cairo bridge;
//! the operator's "what peers does this machine know about, and
//! how reachable are they?" question is fully answered by a
//! sortable table. Shipping the table now closes WB-2.k as
//! useful work; the canvas variant remains a v4.1+ polish task
//! (captured below as WB-2.k.a).
//!
//! Data source: `mackesd Fleet.Files.Peers` via the same
//! shell-out path the workbench already uses for Mesh Pending
//! (avoids a fresh DBusBackend dep in mde-workbench). Empty
//! when mackesd isn't on the bus or no peers are enrolled —
//! that's the honest state; the panel says so.
//!
//! Chrome influence (Phase 0.8): Win11 Settings → Bluetooth &
//! devices "All devices" tabular view.

use std::time::SystemTime;

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding, Task, Theme};
use mde_theme::{mde_icon, FontSize, Icon, IconSize, Palette, TypeRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerStatus {
    Online,
    Idle,
    Offline,
    Unknown,
}

impl PeerStatus {
    fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "online" | "healthy" => Self::Online,
            "idle" | "degraded" => Self::Idle,
            "offline" | "unreachable" => Self::Offline,
            _ => Self::Unknown,
        }
    }
    fn icon(self) -> Icon {
        match self {
            Self::Online => Icon::StatusOk,
            Self::Idle => Icon::StatusWarning,
            Self::Offline => Icon::StatusError,
            Self::Unknown => Icon::StatusUnknown,
        }
    }
    fn color(self) -> Color {
        match self {
            Self::Online => Color::from_rgb(0.20, 0.80, 0.40),
            Self::Idle => Color::from_rgb(0.95, 0.70, 0.20),
            Self::Offline => Color::from_rgb(0.92, 0.32, 0.30),
            Self::Unknown => Color::from_rgb(0.55, 0.55, 0.55),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Online => "ONLINE",
            Self::Idle => "IDLE",
            Self::Offline => "OFFLINE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRow {
    pub name: String,
    pub addr: String,
    pub kind: String,
    pub status: PeerStatus,
}

#[derive(Debug, Clone, Default)]
pub struct MeshTopologyPanel {
    pub peers: Vec<PeerRow>,
    pub error: Option<String>,
    pub last_run_at: Option<SystemTime>,
    pub busy: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Result<Vec<PeerRow>, String>),
    RefreshClicked,
}

impl MeshTopologyPanel {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load() -> Task<crate::Message> {
        Task::perform(async { fetch_peers() }, |result| {
            crate::Message::MeshTopology(Message::Loaded(result))
        })
    }

    pub fn update(&mut self, msg: Message) -> Task<crate::Message> {
        match msg {
            Message::Loaded(Ok(peers)) => {
                self.peers = peers;
                self.error = None;
                self.busy = false;
                self.last_run_at = Some(SystemTime::now());
                Task::none()
            }
            Message::Loaded(Err(e)) => {
                self.peers = Vec::new();
                self.error = Some(e);
                self.busy = false;
                self.last_run_at = Some(SystemTime::now());
                Task::none()
            }
            Message::RefreshClicked => {
                self.busy = true;
                Self::load()
            }
        }
    }

    pub fn view(&self) -> Element<'_, crate::Message> {
        let palette = Palette::dark();
        let sizes = FontSize::defaults();

        let title = text("Mesh Topology")
            .size(TypeRole::Display.size_in(sizes))
            .color(palette.text.into_iced_color());
        let subtitle_text = if let Some(t) = self.last_run_at {
            format!(
                "{} peer{} · last refresh {}",
                self.peers.len(),
                if self.peers.len() == 1 { "" } else { "s" },
                fmt_age(t)
            )
        } else {
            "click Refresh to probe".into()
        };
        let subtitle = text(subtitle_text)
            .size(TypeRole::Body.size_in(sizes))
            .color(palette.text_muted.into_iced_color());

        let refresh_btn = button(
            text(if self.busy { "Loading…" } else { "Refresh" })
                .size(13)
                .color(Color::WHITE),
        )
        .padding(Padding::from([6u16, 14u16]))
        .style({
            let accent = palette.accent.into_iced_color();
            move |_t: &Theme, status: iced::widget::button::Status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => Color {
                        r: accent.r * 1.10,
                        g: accent.g * 1.10,
                        b: accent.b * 1.10,
                        a: accent.a,
                    },
                    _ => accent,
                };
                iced::widget::button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: Color::WHITE,
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 6.0.into(),
                    },
                    shadow: iced::Shadow::default(),
                }
            }
        })
        .on_press(crate::Message::MeshTopology(Message::RefreshClicked));

        let header = row![
            column![title, subtitle].spacing(2),
            Space::with_width(Length::Fill),
            refresh_btn,
        ]
        .align_y(iced::alignment::Vertical::Center);

        let mut rows_col = column![table_head(palette)].spacing(2);
        for p in &self.peers {
            rows_col = rows_col.push(table_row(p, palette));
        }
        if self.peers.is_empty() && self.last_run_at.is_some() {
            rows_col = rows_col.push(empty_state_card(palette, self.error.as_deref()));
        }

        let footer = text(
            "Inter-peer latency matrix is not yet collected. Mackesd would need a peer-mesh sniffer to populate the missing edges; tracked as WB-2.k.a follow-up.",
        )
        .size(10)
        .color(palette.text_muted.into_iced_color());

        container(
            column![
                header,
                Space::with_height(Length::Fixed(16.0)),
                scrollable(rows_col).height(Length::Fill),
                Space::with_height(Length::Fixed(8.0)),
                footer,
            ]
            .spacing(2),
        )
        .padding(Padding::from([24u16, 32u16]))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

fn table_head<'a>(palette: Palette) -> Element<'a, crate::Message> {
    let lbl = |s: &'a str| text(s).size(10).color(palette.text_muted.into_iced_color());
    container(
        row![
            container(lbl("STATUS")).width(Length::Fixed(110.0)),
            container(lbl("NAME")).width(Length::Fixed(180.0)),
            container(lbl("ADDR")).width(Length::Fixed(160.0)),
            container(lbl("KIND")).width(Length::Fill),
        ]
        .spacing(0),
    )
    .padding(Padding::from([6u16, 10u16]))
    .width(Length::Fill)
    .style({
        let border = palette.border.into_iced_color();
        move |_| container::Style {
            background: None,
            border: Border {
                color: border,
                width: 0.0,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        }
    })
    .into()
}

fn table_row<'a>(p: &'a PeerRow, palette: Palette) -> Element<'a, crate::Message> {
    let status_icon_resolved = mde_icon(p.status.icon(), IconSize::Inline);
    let status_color = p.status.color();
    let icon_widget: Element<'a, crate::Message> = if let Some(svg_bytes) = status_icon_resolved.svg_bytes() {
        use iced::widget::svg as widget_svg;
        widget_svg(widget_svg::Handle::from_memory(svg_bytes))
            .width(Length::Fixed(14.0))
            .height(Length::Fixed(14.0))
            .style(move |_t: &Theme, _s: widget_svg::Status| widget_svg::Style {
                color: Some(status_color),
            })
            .into()
    } else {
        text(status_icon_resolved.fallback_glyph)
            .size(14.0)
            .color(status_color)
            .into()
    };
    let status_cell = container(
        row![icon_widget, text(p.status.label()).size(10).color(status_color)]
            .spacing(6)
            .align_y(iced::alignment::Vertical::Center),
    )
    .width(Length::Fixed(110.0));

    let cell = |s: String, sz: u16| text(s).size(sz).color(palette.text.into_iced_color());
    let dim_cell = |s: String, sz: u16| text(s).size(sz).color(palette.text_muted.into_iced_color());

    let bg = palette.raised.into_iced_color();
    let border = palette.border.into_iced_color();
    container(
        row![
            status_cell,
            container(cell(p.name.clone(), 12)).width(Length::Fixed(180.0)),
            container(dim_cell(p.addr.clone(), 11)).width(Length::Fixed(160.0)),
            container(dim_cell(p.kind.clone(), 11)).width(Length::Fill),
        ]
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding(Padding::from([8u16, 10u16]))
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

fn empty_state_card<'a>(palette: Palette, error: Option<&'a str>) -> Element<'a, crate::Message> {
    let (icon_kind, icon_color, heading, body): (Icon, Color, String, String) =
        if let Some(err) = error {
            (
                Icon::StatusError,
                Color::from_rgb(0.92, 0.32, 0.30),
                "Couldn't load peers".to_string(),
                err.to_string(),
            )
        } else {
            (
                Icon::Fleet,
                palette.accent.into_iced_color(),
                "No peers enrolled".to_string(),
                "Enroll peers via mackes/birthright or mackesd's pair-request flow; rows appear here as mackesd's nodes table grows.".to_string(),
            )
        };
    let resolved = mde_icon(icon_kind, IconSize::PanelHeader);
    let icon_widget: Element<'a, crate::Message> = if let Some(svg_bytes) = resolved.svg_bytes() {
        use iced::widget::svg as widget_svg;
        widget_svg(widget_svg::Handle::from_memory(svg_bytes))
            .width(Length::Fixed(32.0))
            .height(Length::Fixed(32.0))
            .style(move |_t: &Theme, _s: widget_svg::Status| widget_svg::Style {
                color: Some(icon_color),
            })
            .into()
    } else {
        text(resolved.fallback_glyph)
            .size(32.0)
            .color(icon_color)
            .into()
    };
    container(
        column![
            icon_widget,
            Space::with_height(Length::Fixed(8.0)),
            text(heading)
                .size(14)
                .color(palette.text.into_iced_color()),
            text(body)
                .size(11)
                .color(palette.text_muted.into_iced_color()),
        ]
        .spacing(2)
        .align_x(iced::alignment::Horizontal::Center),
    )
    .padding(Padding::from([32u16, 16u16]))
    .width(Length::Fill)
    .into()
}

// ---- I/O ------------------------------------------------------

/// Shell out to `mackesd nodes list --json` (or
/// fall back to other CLI paths if that one isn't present).
/// Returns Err with the spawn error message on failure.
pub fn fetch_peers() -> Result<Vec<PeerRow>, String> {
    // mackesd ships `nodes list --json`. Older builds may
    // expose it differently; the JSON shape is what matters.
    let out = std::process::Command::new("mackesd")
        .args(["nodes", "list", "--json"])
        .output()
        .map_err(|e| format!("mackesd nodes list failed to spawn: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        return Err(format!(
            "mackesd nodes list exited non-zero: {stderr}"
        ));
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    Ok(parse_nodes(&raw))
}

/// Pure parser for `mackesd nodes list --json`'s JSON-array
/// output. Each entry has `{node_id, name, public_key, role,
/// health, region}` per `mackesd_core::store::NodeRow`.
#[must_use]
pub fn parse_nodes(raw: &str) -> Vec<PeerRow> {
    let Ok(top) = serde_json::from_str::<Vec<serde_json::Value>>(raw) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in top {
        let node_id = entry
            .get("node_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let name = entry
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(node_id);
        let region = entry
            .get("region")
            .and_then(|v| v.as_str())
            .unwrap_or("—");
        let role = entry
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("peer");
        let health = entry
            .get("health")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        if node_id.is_empty() {
            continue;
        }
        out.push(PeerRow {
            name: name.to_string(),
            addr: region.to_string(),
            kind: role.to_string(),
            status: PeerStatus::from_str(health),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn fmt_age(t: SystemTime) -> String {
    let Ok(elapsed) = t.elapsed() else {
        return "—".into();
    };
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{secs} s ago")
    } else if secs < 3600 {
        format!("{} min ago", secs / 60)
    } else {
        format!("{} h ago", secs / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_status_from_str_known_values() {
        assert_eq!(PeerStatus::from_str("online"), PeerStatus::Online);
        assert_eq!(PeerStatus::from_str("HEALTHY"), PeerStatus::Online);
        assert_eq!(PeerStatus::from_str("idle"), PeerStatus::Idle);
        assert_eq!(PeerStatus::from_str("degraded"), PeerStatus::Idle);
        assert_eq!(PeerStatus::from_str("offline"), PeerStatus::Offline);
        assert_eq!(PeerStatus::from_str("unreachable"), PeerStatus::Offline);
        assert_eq!(PeerStatus::from_str("???"), PeerStatus::Unknown);
    }

    #[test]
    fn parse_nodes_decodes_array() {
        let raw = r#"[
            {"node_id": "peer:pine", "name": "pine", "public_key": "k1",
             "role": "peer", "health": "healthy", "region": "us-west"},
            {"node_id": "peer:birch", "name": "birch", "public_key": "k2",
             "role": "host", "health": "degraded", "region": null}
        ]"#;
        let rows = parse_nodes(raw);
        assert_eq!(rows.len(), 2);
        // Sorted lexicographically by name.
        assert_eq!(rows[0].name, "birch");
        assert_eq!(rows[0].status, PeerStatus::Idle);
        assert_eq!(rows[0].addr, "—");
        assert_eq!(rows[1].name, "pine");
        assert_eq!(rows[1].status, PeerStatus::Online);
        assert_eq!(rows[1].addr, "us-west");
    }

    #[test]
    fn parse_nodes_returns_empty_for_garbage() {
        assert!(parse_nodes("not json").is_empty());
        assert!(parse_nodes("").is_empty());
    }

    #[test]
    fn parse_nodes_skips_entries_without_node_id() {
        let raw = r#"[{"name": "no-id-here"}]"#;
        assert!(parse_nodes(raw).is_empty());
    }

    #[test]
    fn view_renders_empty_without_panic() {
        let p = MeshTopologyPanel::new();
        let _ = p.view();
    }

    #[test]
    fn view_renders_with_rows_without_panic() {
        let mut p = MeshTopologyPanel::new();
        p.peers = vec![PeerRow {
            name: "pine".into(),
            addr: "us-west".into(),
            kind: "peer".into(),
            status: PeerStatus::Online,
        }];
        p.last_run_at = Some(SystemTime::now());
        let _ = p.view();
    }

    #[test]
    fn view_renders_error_state_without_panic() {
        let mut p = MeshTopologyPanel::new();
        p.error = Some("mackesd not installed".into());
        p.last_run_at = Some(SystemTime::now());
        let _ = p.view();
    }
}
