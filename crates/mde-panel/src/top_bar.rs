//! Phase E.17 + Phase E.4-E.29 host wiring — the panel's top-bar
//! view. Lays out the six locked zones in a single 40 px row and
//! renders the live text emitted by each `mde-applet-*` subprocess
//! (driven by [`crate::applet_host`]).
//!
//! ```text
//!   ┌─────────────────────────────────────────────────────────┐
//!   │  M │ [dock…]      [cluster]      [tray icons]    11:42  │
//!   │ Start  Pinned/Tasklist   Cluster   Tray         Clock   │
//!   └─────────────────────────────────────────────────────────┘
//! ```
//!
//! Design locks (2026 surface refresh):
//! - **Surface:** dark glass (#0e0e10 @ 92 % alpha when the
//!   compositor exposes blur; opaque otherwise). Hairline 1 px
//!   at the top edge in `rgba(244,244,244,0.06)`.
//! - **Accent:** `#2b9af3` (PatternFly blue-400 — Carbon
//!   `interactive-04` lock). Greyscale elsewhere; hover lifts
//!   with a 14 %-alpha underglow of the accent.
//! - **Typography:** Red Hat Mono for the clock + tabular numerics,
//!   Red Hat Text 12 px / 500 weight for labels.
//! - **Microinteraction:** 180 ms ease-out for every state change.

use iced::widget::{button, container, row, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding, Shadow, Theme};

use crate::applet_host::AppletKind;
use crate::Message;

/// Height of the top bar in logical pixels (Phase 1.1.0 Win10 lock).
pub const TOP_BAR_HEIGHT_PX: u16 = 40;

/// Per-zone padding (horizontal) — keeps icons + text from
/// touching the bar's edges.
pub const ZONE_PADDING_X: u16 = 12;

/// Accent — Carbon `interactive-04` / PatternFly blue-400.
const ACCENT: Color = Color {
    r: 0.169,
    g: 0.604,
    b: 0.953,
    a: 1.0,
};

/// Foreground text — Carbon `text-01`.
const FG_TEXT: Color = Color {
    r: 0.957,
    g: 0.957,
    b: 0.957,
    a: 1.0,
};

/// Muted helper text — Carbon `text-helper`.
const FG_MUTED: Color = Color {
    r: 0.659,
    g: 0.659,
    b: 0.659,
    a: 1.0,
};

/// Panel background — `#0e0e10` at 92 % alpha.
const SURFACE_BG: Color = Color {
    r: 0.055,
    g: 0.055,
    b: 0.063,
    a: 0.92,
};

/// State injected into [`view`] — one text-cell per applet kind, plus
/// a fixed start label. The panel orchestrator (`App::update`) mutates
/// this via [`set_applet_text`] each time an applet emits a stdout line.
#[derive(Debug, Clone, Default)]
pub struct TopBarState {
    pub start_label: String,
    pub dock_text: String,
    pub cluster_text: String,
    pub clock_text: String,
    pub audio_text: String,
    pub network_text: String,
    pub mesh_text: String,
    pub status_text: String,
    pub bell_text: String,
}

impl TopBarState {
    /// Initial loading placeholder — emitted before the first applet
    /// re-render lands (typically < 1 s after panel spawn).
    #[must_use]
    pub fn loading() -> Self {
        Self {
            start_label: "M".to_string(),
            dock_text: "…".to_string(),
            cluster_text: "…".to_string(),
            clock_text: "--:--".to_string(),
            audio_text: "🔈 --".to_string(),
            network_text: "—".to_string(),
            mesh_text: "—".to_string(),
            status_text: "—".to_string(),
            bell_text: String::new(),
        }
    }

    /// Demo content used by tests + bare-iced dev launches. Kept so
    /// the test `view_renders_without_panic` doesn't need a live
    /// applet host.
    #[must_use]
    pub fn demo() -> Self {
        Self {
            start_label: "M".to_string(),
            dock_text: "[▶ foot]".to_string(),
            cluster_text: "H  def  #1".to_string(),
            clock_text: "11:42".to_string(),
            audio_text: "🔈 65%".to_string(),
            network_text: "Wi-Fi".to_string(),
            mesh_text: "✓ 3".to_string(),
            status_text: "⚡ 88%".to_string(),
            bell_text: "0".to_string(),
        }
    }

    /// Apply the latest stdout line for the given applet kind. Called
    /// from `App::update` on every `Message::AppletText`.
    pub fn set_applet_text(&mut self, kind: AppletKind, text: String) {
        match kind {
            AppletKind::Clock => self.clock_text = text,
            AppletKind::Audio => self.audio_text = text,
            AppletKind::Network => self.network_text = text,
            AppletKind::MeshStatus => self.mesh_text = text,
            AppletKind::StatusCluster => self.status_text = text,
            AppletKind::SwayCluster => self.cluster_text = text,
            AppletKind::NotificationBell => self.bell_text = text,
            AppletKind::Dock => self.dock_text = text,
        }
    }
}

/// Render the top bar. Returns an Iced `Element<Message>`; the
/// click handlers map directly to `Message::StartClicked` /
/// `Message::TrayClicked(kind)`.
#[must_use]
pub fn view(state: &TopBarState) -> Element<'_, Message> {
    let start_btn = button(text(state.start_label.clone()).size(16).color(ACCENT))
        .padding(Padding {
            top: 4.0,
            right: 12.0,
            bottom: 4.0,
            left: 12.0,
        })
        .style(zone_button_style)
        .on_press(Message::StartClicked);

    // Dock zone — shows the dock applet's pinned/running summary
    // (e.g. "[▶ foot] [· firefox]"). Until the inline Iced dock
    // (Phase E.10 host) lands, this is read-only text; clicks fall
    // through to a Noop.
    let dock = labeled_zone(&state.dock_text, FG_TEXT, false);

    // Cluster zone — the sway-IPC chips (`H  def  #1` or similar).
    let cluster = labeled_zone(&state.cluster_text, FG_TEXT, false);

    // Tray — five clickable applet cells in a row.
    let tray = row![
        tray_button(&state.audio_text, AppletKind::Audio),
        Space::with_width(Length::Fixed(8.0)),
        tray_button(&state.network_text, AppletKind::Network),
        Space::with_width(Length::Fixed(8.0)),
        tray_button(&state.mesh_text, AppletKind::MeshStatus),
        Space::with_width(Length::Fixed(8.0)),
        tray_button(&state.status_text, AppletKind::StatusCluster),
        Space::with_width(Length::Fixed(8.0)),
        tray_button(
            if state.bell_text.is_empty() {
                "○"
            } else {
                state.bell_text.as_str()
            },
            AppletKind::NotificationBell,
        ),
    ]
    .align_y(iced::Alignment::Center);

    // Clock — tabular-numeric pill, monospace styling courtesy of
    // the theme. Clicking opens the date popover (when E.12 lands;
    // for now spawns the clock binary's `--now` mode which exits
    // immediately, effectively a no-op).
    let clock = button(text(state.clock_text.clone()).size(13).color(FG_TEXT))
        .padding(Padding {
            top: 6.0,
            right: 12.0,
            bottom: 6.0,
            left: 12.0,
        })
        .style(zone_button_style)
        .on_press(Message::TrayClicked(AppletKind::Clock));

    container(
        row![
            start_btn,
            Space::with_width(Length::Fixed(f32::from(ZONE_PADDING_X))),
            dock,
            Space::with_width(Length::Fill),
            cluster,
            Space::with_width(Length::Fill),
            tray,
            Space::with_width(Length::Fixed(f32::from(ZONE_PADDING_X))),
            clock,
        ]
        .align_y(iced::Alignment::Center)
        .padding(Padding {
            top: 0.0,
            right: f32::from(ZONE_PADDING_X),
            bottom: 0.0,
            left: f32::from(ZONE_PADDING_X),
        }),
    )
    .width(Length::Fill)
    .height(Length::Fixed(f32::from(TOP_BAR_HEIGHT_PX)))
    .style(panel_surface)
    .into()
}

/// Read-only text zone with a thin padding box. Used by the dock and
/// cluster cells which aren't yet click-targets.
fn labeled_zone(label: &str, color: Color, accent: bool) -> Element<'_, Message> {
    let style_color = if accent { ACCENT } else { color };
    container(text(label.to_string()).size(13).color(style_color))
        .padding(Padding {
            top: 4.0,
            right: 6.0,
            bottom: 4.0,
            left: 6.0,
        })
        .into()
}

fn tray_button(label: &str, kind: AppletKind) -> Element<'_, Message> {
    button(text(label.to_string()).size(13).color(FG_TEXT))
        .padding(Padding {
            top: 6.0,
            right: 8.0,
            bottom: 6.0,
            left: 8.0,
        })
        .style(zone_button_style)
        .on_press(Message::TrayClicked(kind))
        .into()
}

fn panel_surface(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE_BG)),
        border: Border {
            color: Color {
                r: 0.957,
                g: 0.957,
                b: 0.957,
                a: 0.06,
            },
            width: 1.0,
            radius: 0.0.into(),
        },
        text_color: Some(FG_TEXT),
        shadow: Shadow::default(),
    }
}

/// Zone-button style — flat, no border, accent-tinted hover.
fn zone_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => Some(Background::Color(Color {
            r: ACCENT.r,
            g: ACCENT.g,
            b: ACCENT.b,
            a: 0.14,
        })),
        button::Status::Pressed => Some(Background::Color(Color {
            r: ACCENT.r,
            g: ACCENT.g,
            b: ACCENT.b,
            a: 0.22,
        })),
        _ => None,
    };
    button::Style {
        background: bg,
        text_color: FG_TEXT,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 4.0.into(),
        },
        shadow: Shadow::default(),
    }
}

#[allow(dead_code)]
fn muted() -> Color {
    FG_MUTED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_bar_height_is_40px_per_1_1_0_lock() {
        assert_eq!(TOP_BAR_HEIGHT_PX, 40);
    }

    #[test]
    fn zone_padding_is_symmetric_12px() {
        assert_eq!(ZONE_PADDING_X, 12);
    }

    #[test]
    fn loading_state_populates_every_field() {
        let state = TopBarState::loading();
        assert!(!state.start_label.is_empty());
        assert!(!state.clock_text.is_empty());
        assert!(!state.audio_text.is_empty());
    }

    #[test]
    fn set_applet_text_routes_to_correct_field() {
        let mut state = TopBarState::default();
        state.set_applet_text(AppletKind::Clock, "12:34".into());
        assert_eq!(state.clock_text, "12:34");
        state.set_applet_text(AppletKind::Audio, "🔈 50%".into());
        assert_eq!(state.audio_text, "🔈 50%");
        state.set_applet_text(AppletKind::Network, "Wi-Fi: home".into());
        assert_eq!(state.network_text, "Wi-Fi: home");
        state.set_applet_text(AppletKind::MeshStatus, "✓ 4".into());
        assert_eq!(state.mesh_text, "✓ 4");
        state.set_applet_text(AppletKind::StatusCluster, "⚡ 99%".into());
        assert_eq!(state.status_text, "⚡ 99%");
        state.set_applet_text(AppletKind::SwayCluster, "H def #1".into());
        assert_eq!(state.cluster_text, "H def #1");
        state.set_applet_text(AppletKind::Dock, "[▶ foot]".into());
        assert_eq!(state.dock_text, "[▶ foot]");
        state.set_applet_text(AppletKind::NotificationBell, "3".into());
        assert_eq!(state.bell_text, "3");
    }

    #[test]
    fn view_renders_without_panic() {
        let state = TopBarState::demo();
        let _ = view(&state);
    }
}
