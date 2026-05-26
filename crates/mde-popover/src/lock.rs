//! `mde-popover lock` — lock screen layer-shell overlay (Portal-25).
//!
//! Fullscreen Layer::Overlay surface that paints the MDE lock screen
//! over every other surface, capturing keyboard input exclusively.
//! Replaces swaylock theming with a single render path that matches
//! Portal visual identity.
//!
//! Layout (Classic ChromeOS palette, Intel One Mono):
//!   • Top-left breadcrumb       `M › <hostname>`
//!   • Centered big clock         `HH:MM`           (96 px)
//!   • Date                       `Mon, May 25`     (18 px)
//!   • Mesh / network / battery indicator row
//!   • Weather hint               read from ~/.cache/mde/weather.json
//!   • Footer                    `Press Esc or Enter to unlock`
//!
//! Esc or Enter exits the process (the compositor session-lock
//! protocol covers actual credential entry; this surface is the
//! visual layer that replaces swaylock theming — R2-Q54 / R4-Q4).

#![forbid(unsafe_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use iced::widget::{column, container, mouse_area, row, text, Space};
use iced::widget::container::Style as ContainerStyle;
use iced::{Background, Border, Color, Element, Length, Padding, Subscription, Task, Theme};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings};
use iced_layershell::to_layer_message;

// ── Design tokens (Classic ChromeOS, §3 lock) ─────────────────────────────────

const CHARCOAL: Color = Color { r: 0.125, g: 0.129, b: 0.141, a: 1.0 };
const FG: Color = Color::WHITE;
const FG_DIM: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.55 };
const FG_LABEL: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.35 };
const ACCENT: Color = Color { r: 0.357, g: 0.416, b: 0.961, a: 1.0 }; // indigo
const DOT_UP: Color = Color { r: 0.345, g: 0.871, b: 0.475, a: 1.0 }; // soft green
const DOT_DOWN: Color = Color { r: 0.498, g: 0.498, b: 0.498, a: 1.0 };

// ── Data collection ───────────────────────────────────────────────────────────

fn read_hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn read_battery_pct() -> Option<u8> {
    for name in &["BAT0", "BAT1"] {
        let path = format!("/sys/class/power_supply/{name}/capacity");
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(n) = s.trim().parse::<u8>() {
                return Some(n);
            }
        }
    }
    None
}

fn read_interface_up(iface: &str) -> bool {
    let path = format!("/sys/class/net/{iface}/operstate");
    std::fs::read_to_string(path)
        .map(|s| s.trim() == "up")
        .unwrap_or(false)
}

fn read_network_up() -> bool {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let iface = name.to_string_lossy();
        if iface == "lo" {
            continue;
        }
        if read_interface_up(&iface) {
            return true;
        }
    }
    false
}

fn read_weather_summary() -> String {
    let Some(cache) = dirs::cache_dir() else {
        return "—".to_string();
    };
    let path = cache.join("mde").join("weather.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return "—".to_string();
    };
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return "—".to_string(),
    };
    let temp = v
        .get("temp_c")
        .and_then(|t| t.as_i64())
        .map(|t| format!("{t}°C"))
        .unwrap_or_else(|| "—".to_string());
    let cond = v
        .get("condition")
        .and_then(|c| c.as_str())
        .unwrap_or("—")
        .to_string();
    format!("{cond} · {temp}")
}

// ── Date / time helpers (mirrors crate::clock) ────────────────────────────────

fn now_secs_local() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if mo <= 2 { y + 1 } else { y };
    (year as i32, mo as u32, d as u32)
}

fn ymd_to_days(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + (d as u64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

fn weekday_short(y: i32, m: u32, d: u32) -> &'static str {
    let days = ymd_to_days(y, m, d);
    let w = ((days.rem_euclid(7) + 4) % 7) as usize; // 0 = Sun
    ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][w]
}

const MONTH_NAMES_SHORT: &[&str] = &[
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn format_hm(secs_in_day: u32) -> String {
    let h = (secs_in_day / 3600) % 24;
    let m = (secs_in_day % 3600) / 60;
    format!("{h:02}:{m:02}")
}

fn format_date(y: i32, m: u32, d: u32) -> String {
    let wd = weekday_short(y, m, d);
    let mo = MONTH_NAMES_SHORT[(m - 1) as usize];
    format!("{wd}, {mo} {d}")
}

// ── Application ───────────────────────────────────────────────────────────────

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    Exit,
}

pub struct App {
    hostname: String,
    clock: String,
    date: String,
    mesh_up: bool,
    network_up: bool,
    battery_pct: Option<u8>,
    weather: String,
}

impl App {
    fn refresh() -> Self {
        let now = now_secs_local();
        let secs_in_day = (now.rem_euclid(86_400)) as u32;
        let (y, mo, d) = days_to_ymd(now.div_euclid(86_400));
        Self {
            hostname: read_hostname(),
            clock: format_hm(secs_in_day),
            date: format_date(y, mo, d),
            mesh_up: read_interface_up("nebula0"),
            network_up: read_network_up(),
            battery_pct: read_battery_pct(),
            weather: read_weather_summary(),
        }
    }
}

impl iced_layershell::Application for App {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Task<Message>) {
        let app = App::refresh();
        tracing::info!(hostname = %app.hostname, clock = %app.clock, "lock popover open");
        (app, Task::none())
    }

    fn namespace(&self) -> String {
        "mde-popover-lock".to_string()
    }

    fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Tick => {
                let fresh = App::refresh();
                self.hostname = fresh.hostname;
                self.clock = fresh.clock;
                self.date = fresh.date;
                self.mesh_up = fresh.mesh_up;
                self.network_up = fresh.network_up;
                self.battery_pct = fresh.battery_pct;
                self.weather = fresh.weather;
            }
            Message::Exit => std::process::exit(0),
            _ => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let breadcrumb: Element<'_, Message> = row![
            text("M")
                .size(13)
                .color(ACCENT),
            Space::with_width(Length::Fixed(6.0)),
            text("›").size(13).color(FG_LABEL),
            Space::with_width(Length::Fixed(6.0)),
            text(self.hostname.clone()).size(13).color(FG_DIM),
        ]
        .align_y(iced::Alignment::Center)
        .into();

        let big_clock: Element<'_, Message> =
            text(self.clock.clone()).size(96).color(FG).into();

        let date_line: Element<'_, Message> =
            text(self.date.clone()).size(18).color(FG_DIM).into();

        // ── Indicator row (mesh / net / battery / weather) ────────────────────
        let dot = |label: &str, up: bool| -> Element<'static, Message> {
            row![
                container(Space::with_width(Length::Fill))
                    .width(Length::Fixed(8.0))
                    .height(Length::Fixed(8.0))
                    .style(move |_: &Theme| ContainerStyle {
                        background: Some(Background::Color(if up { DOT_UP } else { DOT_DOWN })),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }),
                Space::with_width(Length::Fixed(6.0)),
                text(label.to_string()).size(12).color(FG_DIM),
            ]
            .align_y(iced::Alignment::Center)
            .into()
        };

        let battery_chip: Element<'_, Message> = match self.battery_pct {
            Some(p) => row![
                text("bat").size(10).color(FG_LABEL),
                Space::with_width(Length::Fixed(4.0)),
                text(format!("{p}%")).size(12).color(FG_DIM),
            ]
            .align_y(iced::Alignment::Center)
            .into(),
            None => row![text("bat —").size(12).color(FG_LABEL)]
                .align_y(iced::Alignment::Center)
                .into(),
        };

        let indicators: Element<'_, Message> = row![
            dot("mesh", self.mesh_up),
            Space::with_width(Length::Fixed(20.0)),
            dot("net", self.network_up),
            Space::with_width(Length::Fixed(20.0)),
            battery_chip,
            Space::with_width(Length::Fixed(20.0)),
            text(self.weather.clone()).size(12).color(FG_DIM),
        ]
        .align_y(iced::Alignment::Center)
        .into();

        // ── Centered card ────────────────────────────────────────────────────
        let card_body = column![
            breadcrumb,
            Space::with_height(Length::Fixed(28.0)),
            big_clock,
            Space::with_height(Length::Fixed(4.0)),
            date_line,
            Space::with_height(Length::Fixed(24.0)),
            indicators,
        ]
        .align_x(iced::Alignment::Center);

        let footer: Element<'_, Message> =
            text("Press Esc or Enter to unlock").size(11).color(FG_LABEL).into();

        let centered = column![
            Space::with_height(Length::Fill),
            card_body,
            Space::with_height(Length::Fill),
            footer,
            Space::with_height(Length::Fixed(40.0)),
        ]
        .align_x(iced::Alignment::Center)
        .padding(Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 0.0 });

        // mouse_area on the whole surface so an Enter-equivalent
        // click (rare but possible on touch devices) dismisses.
        let backdrop = mouse_area(
            container(centered)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_: &Theme| ContainerStyle {
                    background: Some(Background::Color(CHARCOAL)),
                    ..Default::default()
                }),
        )
        .on_press(Message::Exit);

        backdrop.into()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }

    fn subscription(&self) -> Subscription<Message> {
        let keys = iced::keyboard::on_key_press(|key, _| {
            use iced::keyboard::{key::Named, Key};
            match key {
                Key::Named(Named::Escape) | Key::Named(Named::Enter) => Some(Message::Exit),
                _ => None,
            }
        });

        // Tick every 10 s so the clock + battery + mesh dot stay fresh.
        let tick = iced::time::every(std::time::Duration::from_secs(10))
            .map(|_| Message::Tick);

        Subscription::batch([keys, tick])
    }
}

pub fn run() -> iced_layershell::Result {
    <App as iced_layershell::Application>::run(Settings {
        id: Some("mde-popover-lock".to_string()),
        fonts: crate::fonts::load_fallback_fonts(),
        layer_settings: LayerShellSettings {
            size: None,
            exclusive_zone: -1,
            anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
            margin: (0, 0, 0, 0),
            layer: Layer::Overlay,
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            ..Default::default()
        },
        ..Default::default()
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_hostname_is_short() {
        let h = read_hostname();
        assert!(h.len() < 256);
    }

    #[test]
    fn format_hm_pads_zero() {
        assert_eq!(format_hm(0), "00:00");
        assert_eq!(format_hm(60), "00:01");
        assert_eq!(format_hm(9 * 3600 + 7 * 60), "09:07");
        assert_eq!(format_hm(23 * 3600 + 59 * 60), "23:59");
    }

    #[test]
    fn format_hm_wraps_24h() {
        // 25:00 should wrap to 01:00.
        assert_eq!(format_hm(25 * 3600), "01:00");
    }

    #[test]
    fn weekday_short_known_dates() {
        // 1970-01-01 = Thursday.
        assert_eq!(weekday_short(1970, 1, 1), "Thu");
        // 2026-05-25 = Monday (per user message frontmatter).
        assert_eq!(weekday_short(2026, 5, 25), "Mon");
    }

    #[test]
    fn format_date_includes_weekday_and_month() {
        let s = format_date(2026, 5, 25);
        assert!(s.starts_with("Mon"));
        assert!(s.contains("May"));
        assert!(s.contains("25"));
    }

    #[test]
    fn days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn read_battery_pct_returns_valid_range_or_none() {
        if let Some(p) = read_battery_pct() {
            assert!(p <= 100);
        }
    }

    #[test]
    fn read_network_up_does_not_panic() {
        let _ = read_network_up();
    }

    #[test]
    fn read_interface_up_handles_missing_iface() {
        assert!(!read_interface_up("definitely-not-a-real-iface"));
    }

    #[test]
    fn read_weather_summary_returns_string_when_cache_absent() {
        // Either real data or "—" placeholder — never panics.
        let s = read_weather_summary();
        assert!(!s.is_empty());
    }

    #[test]
    fn app_refresh_populates_fields() {
        let app = App::refresh();
        assert!(!app.clock.is_empty());
        assert_eq!(app.clock.len(), 5); // HH:MM
        assert!(!app.date.is_empty());
        assert!(!app.weather.is_empty());
    }

    #[test]
    fn charcoal_matches_chromeos_design_lock() {
        let r = (CHARCOAL.r * 255.0).round() as u8;
        let g = (CHARCOAL.g * 255.0).round() as u8;
        let b = (CHARCOAL.b * 255.0).round() as u8;
        assert_eq!((r, g, b), (32, 33, 36), "#202124 charcoal lock");
    }
}
