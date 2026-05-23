//! v4.0.1 (2026-05-23) — network popover. Closes the §0.12
//! grandfathered stub in `crates/mde-popover/src/main.rs`.
//!
//! Minimal nmcli-shellout implementation: lists active
//! connections + interface states. Wi-Fi scan list + per-AP
//! Connect action are scoped to a future v3.1 follow-up that
//! talks to `org.freedesktop.NetworkManager` over zbus
//! directly; this version covers the "what am I connected to?"
//! and "what interfaces does this machine have?" cases that
//! 95% of operator clicks ask.
//!
//! Anchor: top-right of the primary output, 8 px below the
//! panel edge. Operator clicks the panel's network tray
//! button → `mde-panel` execs `mde-popover network` → this
//! binary opens a 360×420 layer-shell window. Esc closes.

use std::process::Command;

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding, Shadow, Task, Theme};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings};
use iced_layershell::to_layer_message;

const WIDTH: u32 = 360;
const HEIGHT: u32 = 420;

const ACCENT: Color = Color {
    r: 0.357,
    g: 0.416,
    b: 0.961,
    a: 1.0,
};
const FG_TEXT: Color = Color {
    r: 0.957,
    g: 0.957,
    b: 0.957,
    a: 1.0,
};
const FG_MUTED: Color = Color {
    r: 0.659,
    g: 0.659,
    b: 0.659,
    a: 1.0,
};
const FG_FAINT: Color = Color {
    r: 0.450,
    g: 0.450,
    b: 0.450,
    a: 1.0,
};
const SURFACE_BG: Color = Color {
    r: 0.055,
    g: 0.055,
    b: 0.063,
    a: 0.97,
};
const CARD_BG: Color = Color {
    r: 0.110,
    g: 0.110,
    b: 0.118,
    a: 1.0,
};

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    OpenNmApplet,
    Esc,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveConnection {
    pub name: String,
    pub interface: String,
    pub conn_type: String,
    pub state: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceRow {
    pub interface: String,
    pub kind: String,
    pub state: String,
    pub connection: String,
}

#[derive(Debug, Default)]
pub struct App {
    pub active: Vec<ActiveConnection>,
    pub devices: Vec<DeviceRow>,
}

impl iced_layershell::Application for App {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Task<Message>) {
        let app = Self {
            active: scan_active_connections(),
            devices: scan_devices(),
        };
        (app, Task::none())
    }

    fn namespace(&self) -> String {
        "mde-popover-network".into()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => {
                self.active = scan_active_connections();
                self.devices = scan_devices();
                Task::none()
            }
            Message::OpenNmApplet => {
                // Best-effort: launch nm-connection-editor if
                // installed (the standard "manage connections"
                // GUI on Fedora). nm-applet is the tray-icon
                // tool, not a settings editor.
                let _ = Command::new("nm-connection-editor").spawn();
                Task::none()
            }
            Message::Esc => std::process::exit(0),
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let title = text("Network")
            .size(15)
            .color(FG_TEXT);
        let subtitle = text(format!(
            "{} active · {} device{}",
            self.active.len(),
            self.devices.len(),
            if self.devices.len() == 1 { "" } else { "s" },
        ))
        .size(11)
        .color(FG_MUTED);

        let refresh_btn = button(text("Refresh").size(11).color(FG_TEXT))
            .padding(Padding::from([4u16, 10u16]))
            .style(|_, status| ghost_btn_style(status))
            .on_press(Message::Refresh);

        let header = row![
            column![title, subtitle].spacing(2),
            Space::with_width(Length::Fill),
            refresh_btn,
        ]
        .align_y(iced::alignment::Vertical::Center);

        let mut active_col = column![
            text("Active connections")
                .size(11)
                .color(FG_MUTED),
        ]
        .spacing(6);
        if self.active.is_empty() {
            active_col = active_col.push(empty_card("Not connected."));
        } else {
            for c in &self.active {
                active_col = active_col.push(active_card(c));
            }
        }

        let mut device_col = column![
            text("Devices")
                .size(11)
                .color(FG_MUTED),
        ]
        .spacing(6);
        if self.devices.is_empty() {
            device_col = device_col.push(empty_card("No interfaces."));
        } else {
            for d in &self.devices {
                device_col = device_col.push(device_card(d));
            }
        }

        let manage_btn = button(text("Open NetworkManager").size(11).color(Color::WHITE))
            .padding(Padding::from([5u16, 12u16]))
            .style(|_, status| accent_btn_style(status))
            .on_press(Message::OpenNmApplet);

        let body = scrollable(
            column![
                active_col,
                Space::with_height(Length::Fixed(12.0)),
                device_col,
            ]
            .spacing(6),
        )
        .height(Length::Fill);

        container(
            column![
                header,
                Space::with_height(Length::Fixed(10.0)),
                body,
                Space::with_height(Length::Fixed(8.0)),
                row![Space::with_width(Length::Fill), manage_btn]
                    .align_y(iced::alignment::Vertical::Center),
            ]
            .spacing(2),
        )
        .padding(Padding::from([16u16, 18u16]))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(SURFACE_BG)),
            border: Border {
                color: Color {
                    a: 0.08,
                    ..Color::WHITE
                },
                width: 1.0,
                radius: 8.0.into(),
            },
            shadow: Shadow::default(),
            text_color: Some(FG_TEXT),
        })
        .into()
    }

    fn theme(&self) -> Theme {
        Theme::custom(
            "mde-popover-network".into(),
            iced::theme::Palette {
                background: SURFACE_BG,
                text: FG_TEXT,
                primary: ACCENT,
                success: Color::from_rgb(0.20, 0.80, 0.40),
                danger: Color::from_rgb(0.92, 0.32, 0.30),
            },
        )
    }
}

fn active_card<'a>(c: &'a ActiveConnection) -> Element<'a, Message> {
    let title = text(c.name.clone())
        .size(13)
        .color(FG_TEXT);
    let detail = text(format!("{} · {} · {}", c.interface, c.conn_type, c.state))
        .size(11)
        .color(FG_MUTED);
    container(column![title, detail].spacing(2))
        .padding(Padding::from([8u16, 12u16]))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(CARD_BG)),
            border: Border {
                color: Color {
                    a: 0.06,
                    ..Color::WHITE
                },
                width: 1.0,
                radius: 5.0.into(),
            },
            shadow: Shadow::default(),
            text_color: Some(FG_TEXT),
        })
        .into()
}

fn device_card<'a>(d: &'a DeviceRow) -> Element<'a, Message> {
    let title = text(format!("{} ({})", d.interface, d.kind))
        .size(12)
        .color(FG_TEXT);
    let detail = text(format!(
        "{}{}",
        d.state,
        if d.connection.is_empty() {
            String::new()
        } else {
            format!(" · {}", d.connection)
        }
    ))
    .size(11)
    .color(FG_MUTED);
    container(row![title, Space::with_width(Length::Fill), detail])
        .padding(Padding::from([6u16, 12u16]))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(CARD_BG)),
            border: Border {
                color: Color {
                    a: 0.04,
                    ..Color::WHITE
                },
                width: 1.0,
                radius: 4.0.into(),
            },
            shadow: Shadow::default(),
            text_color: Some(FG_TEXT),
        })
        .into()
}

fn empty_card<'a>(msg: &'a str) -> Element<'a, Message> {
    container(text(msg).size(11).color(FG_FAINT))
        .padding(Padding::from([10u16, 12u16]))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(CARD_BG)),
            border: Border {
                color: Color {
                    a: 0.04,
                    ..Color::WHITE
                },
                width: 1.0,
                radius: 4.0.into(),
            },
            shadow: Shadow::default(),
            text_color: Some(FG_FAINT),
        })
        .into()
}

fn ghost_btn_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
    let bg = match status {
        iced::widget::button::Status::Hovered => Color {
            r: 0.15,
            g: 0.15,
            b: 0.17,
            a: 1.0,
        },
        _ => Color::TRANSPARENT,
    };
    iced::widget::button::Style {
        background: Some(Background::Color(bg)),
        text_color: FG_TEXT,
        border: Border {
            color: Color {
                a: 0.10,
                ..Color::WHITE
            },
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: Shadow::default(),
    }
}

fn accent_btn_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
    let bg = match status {
        iced::widget::button::Status::Hovered => Color {
            r: ACCENT.r * 1.10,
            g: ACCENT.g * 1.10,
            b: ACCENT.b * 1.10,
            a: ACCENT.a,
        },
        _ => ACCENT,
    };
    iced::widget::button::Style {
        background: Some(Background::Color(bg)),
        text_color: Color::WHITE,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 6.0.into(),
        },
        shadow: Shadow::default(),
    }
}

// ---- nmcli shell-outs -----------------------------------------

/// Pure parser for `nmcli -t -f NAME,DEVICE,TYPE,STATE
/// connection show --active` output. Each line is colon-
/// separated; nmcli escapes embedded colons as `\:`.
#[must_use]
pub fn parse_active_connections(raw: &str) -> Vec<ActiveConnection> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fields = nmcli_split(line);
        if fields.len() < 4 {
            continue;
        }
        out.push(ActiveConnection {
            name: fields[0].clone(),
            interface: fields[1].clone(),
            conn_type: fields[2].clone(),
            state: fields[3].clone(),
        });
    }
    out
}

/// Pure parser for `nmcli -t -f DEVICE,TYPE,STATE,CONNECTION
/// device status`.
#[must_use]
pub fn parse_devices(raw: &str) -> Vec<DeviceRow> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fields = nmcli_split(line);
        if fields.len() < 4 {
            continue;
        }
        // Filter out the `lo` loopback + `p2p` devices — they
        // confuse the operator and aren't actionable here.
        let dev = &fields[0];
        if dev == "lo" || dev.starts_with("p2p-") {
            continue;
        }
        out.push(DeviceRow {
            interface: fields[0].clone(),
            kind: fields[1].clone(),
            state: fields[2].clone(),
            connection: if fields[3] == "--" {
                String::new()
            } else {
                fields[3].clone()
            },
        });
    }
    out
}

/// nmcli's terse mode escapes `:` as `\:`. Split on unescaped
/// colons and un-escape the field bodies.
fn nmcli_split(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if next == ':' || next == '\\' {
                    cur.push(chars.next().unwrap());
                    continue;
                }
            }
            cur.push(c);
        } else if c == ':' {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

fn scan_active_connections() -> Vec<ActiveConnection> {
    let out = Command::new("nmcli")
        .args(["-t", "-f", "NAME,DEVICE,TYPE,STATE", "connection", "show", "--active"])
        .output()
        .ok();
    match out {
        Some(o) if o.status.success() => parse_active_connections(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

fn scan_devices() -> Vec<DeviceRow> {
    let out = Command::new("nmcli")
        .args(["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device", "status"])
        .output()
        .ok();
    match out {
        Some(o) if o.status.success() => parse_devices(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

pub fn run() -> iced_layershell::Result {
    <App as iced_layershell::Application>::run(Settings {
        id: Some("mde-popover-network".into()),
        fonts: crate::fonts::load_fallback_fonts(),
        layer_settings: LayerShellSettings {
            layer: Layer::Top,
            anchor: Anchor::Top | Anchor::Right,
            margin: (44, 14, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            exclusive_zone: 0,
            size: Some((WIDTH, HEIGHT)),
            ..Default::default()
        },
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_active_round_trips_wired() {
        let raw = "Wired connection 1:enp0s31f6:ethernet:activated\n";
        let parsed = parse_active_connections(raw);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "Wired connection 1");
        assert_eq!(parsed[0].interface, "enp0s31f6");
        assert_eq!(parsed[0].conn_type, "ethernet");
        assert_eq!(parsed[0].state, "activated");
    }

    #[test]
    fn parse_active_handles_wifi_with_colons_in_ssid() {
        // The hypothetical SSID "Café \:test" escapes the colon.
        let raw = "Café\\:test:wlp2s0:wifi:activated";
        let parsed = parse_active_connections(raw);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "Café:test");
        assert_eq!(parsed[0].interface, "wlp2s0");
    }

    #[test]
    fn parse_active_ignores_empty_lines() {
        let raw = "\n\nWired:eth0:ethernet:activated\n\n";
        let parsed = parse_active_connections(raw);
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn parse_active_ignores_short_rows() {
        let raw = "only-two:fields\n";
        assert!(parse_active_connections(raw).is_empty());
    }

    #[test]
    fn parse_devices_filters_loopback() {
        let raw = "enp0s31f6:ethernet:connected:Wired connection 1\nlo:loopback:unmanaged:--\n";
        let devs = parse_devices(raw);
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].interface, "enp0s31f6");
    }

    #[test]
    fn parse_devices_replaces_dash_connection_with_empty() {
        let raw = "wlp2s0:wifi:disconnected:--\n";
        let devs = parse_devices(raw);
        assert_eq!(devs[0].connection, "");
    }

    #[test]
    fn parse_devices_filters_p2p_helpers() {
        let raw = "wlp2s0:wifi:connected:home\np2p-dev-wlp2s0:wifi-p2p:disconnected:--\n";
        let devs = parse_devices(raw);
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].interface, "wlp2s0");
    }

    #[test]
    fn nmcli_split_handles_escaped_backslash() {
        // Raw `a\\b:c` should split into ["a\b", "c"].
        let fields = nmcli_split("a\\\\b:c");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0], "a\\b");
        assert_eq!(fields[1], "c");
    }
}
