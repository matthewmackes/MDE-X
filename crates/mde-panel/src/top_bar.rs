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

use iced::widget::{button, column, container, mouse_area, row, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding, Shadow, Theme};

use crate::applet_host::AppletKind;
use crate::hero::Hero;
use crate::toplevels::Toplevel;
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
/// `Message::TrayClicked(kind)` / `Message::WindowMinimize` etc.
///
/// v3.0.3 — signature gained `hero` (focused-window display, from
/// the Phase E.4.2 widget) and `focused` (current focused
/// toplevel, used by the window-management buttons to grey out
/// when no window is focused).
#[must_use]
pub fn view<'a>(
    state: &'a TopBarState,
    hero: &'a Hero,
    focused: Option<&'a Toplevel>,
) -> Element<'a, Message> {
    // v3.0.3 Tier 1D fix — wrap the Start button in `mouse_area`
    // so right-click is observable. Iced's built-in `button` is
    // left-click only; the operator-reported "right click on the
    // start menu does not work" bug stemmed from this gap. The
    // left-click `Message::StartClicked` keeps coming from the
    // inner button; the right-click maps to a new
    // `Message::StartRightClicked` that opens the admin-menu
    // popover.
    let start_btn = mouse_area(
        button(text(state.start_label.clone()).size(16).color(ACCENT))
            .padding(Padding {
                top: 4.0,
                right: 12.0,
                bottom: 4.0,
                left: 12.0,
            })
            .style(zone_button_style)
            .on_press(Message::StartClicked),
    )
    .on_right_press(Message::StartRightClicked);

    // Dock zone — shows the dock applet's pinned/running summary
    // (e.g. "[▶ foot] [· firefox]"). Until the inline Iced dock
    // (Phase E.10 host) lands, this is read-only text; clicks fall
    // through to a Noop.
    let dock = labeled_zone(&state.dock_text, FG_TEXT, false);

    // v3.0.3 Phase E.4.2 wiring — focused-window hero. Shows the
    // ellipsized title (max 64 chars) or empty when no window is
    // focused. Sits between Dock and the right-flex spacer so it
    // gets generous horizontal space without crowding the tray.
    let hero_zone = hero_view(hero);

    // Cluster zone — the sway-IPC chips (`H  def  #1` or similar).
    let cluster = labeled_zone(&state.cluster_text, FG_TEXT, false);

    // Tray — six clickable cells in a row.
    // v4.0.1 BUG-7: clipboard tray icon added between status_cluster
    // and notification-bell. Click fires Message::ClipboardClicked,
    // routed to `mde-popover clipboard` (same surface as Super+V).
    // It's a static icon (no applet text stream); the glyph is the
    // Unicode clipboard codepoint U+1F4CB until the BUG-13 Carbon
    // SVG wiring lands.
    let tray = row![
        tray_button(&state.audio_text, AppletKind::Audio),
        Space::with_width(Length::Fixed(8.0)),
        tray_button(&state.network_text, AppletKind::Network),
        Space::with_width(Length::Fixed(8.0)),
        tray_button(&state.mesh_text, AppletKind::MeshStatus),
        Space::with_width(Length::Fixed(8.0)),
        tray_button(&state.status_text, AppletKind::StatusCluster),
        Space::with_width(Length::Fixed(8.0)),
        clipboard_button(),
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

    // v3.0.3 Tier 1E (v8.7 lock) — min/max/close cluster. Reads
    // `focused` for its enabled/disabled state. Per the lock,
    // "maximize = floating-fill, not fullscreen" — see
    // `Message::WindowMaximize` for the swaymsg argv.
    let window_buttons = window_button_cluster(focused.is_some());

    // Clock — Win10 two-line stack. v4.0.1 BUG-14: the applet emits
    // "H:MM AM/PM\nM/D/YYYY"; render the time on top with a slightly
    // larger size, the date below in the muted foreground. Falls back
    // to single-line rendering for the loading-state ("--:--").
    let clock_lines: Vec<&str> = state.clock_text.split('\n').collect();
    let clock_body: Element<'_, Message> = if clock_lines.len() >= 2 {
        column![
            text(clock_lines[0].to_string()).size(13).color(FG_TEXT),
            text(clock_lines[1].to_string()).size(10).color(FG_MUTED),
        ]
        .spacing(0)
        .align_x(iced::alignment::Horizontal::Right)
        .into()
    } else {
        text(state.clock_text.clone()).size(13).color(FG_TEXT).into()
    };
    let clock = button(clock_body)
        .padding(Padding {
            top: 2.0,
            right: 12.0,
            bottom: 2.0,
            left: 12.0,
        })
        .style(zone_button_style)
        .on_press(Message::TrayClicked(AppletKind::Clock));

    // v4.0.1 BUG-6: operator asked for the min/max/close cluster
    // centered on the panel (was far-right per the v8.7 lock).
    // Newer-wins-silently — window_buttons now occupies the center
    // slot between two flex spaces; cluster (sway-IPC chips) moves
    // adjacent to the clock on the right, where it's a less-prominent
    // status surface rather than the "title area" the operator was
    // mistaking it for (BUG-3).
    container(
        row![
            start_btn,
            Space::with_width(Length::Fixed(f32::from(ZONE_PADDING_X))),
            dock,
            Space::with_width(Length::Fixed(f32::from(ZONE_PADDING_X))),
            hero_zone,
            Space::with_width(Length::Fill),
            window_buttons,
            Space::with_width(Length::Fill),
            tray,
            Space::with_width(Length::Fixed(f32::from(ZONE_PADDING_X))),
            cluster,
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

/// v3.0.3 Phase E.4.2 wiring — render the focused-window hero.
/// Shows the ellipsized title (or "" when no window is focused).
/// Sliding animation is driven by `hero::Hero::tick` from the
/// panel's `Message::Tick` reducer; this function just reads the
/// post-tick state.
fn hero_view<'a>(hero: &'a Hero) -> Element<'a, Message> {
    let label = hero
        .display_title()
        .unwrap_or_else(|| String::new());
    container(text(label).size(13).color(FG_TEXT))
        .padding(Padding {
            top: 4.0,
            right: 8.0,
            bottom: 4.0,
            left: 8.0,
        })
        .into()
}

/// v3.0.3 Tier 1E — three-button cluster (min/max/close) per the
/// v8.7 design lock. Greys out (no on_press handler attached) when
/// `enabled` is false, i.e. no toplevel is focused.
///
/// Glyphs are the Carbon-style "−" (minimize), "□" (maximize/
/// restore), "×" (close). Close gains a destructive hover tint
/// to match the popover close button (`crate::dismiss::close_button`
/// shape — color shared across the panel + popover surfaces).
fn window_button_cluster<'a>(enabled: bool) -> Element<'a, Message> {
    row![
        window_btn("−", enabled.then_some(Message::WindowMinimize), false),
        Space::with_width(Length::Fixed(4.0)),
        window_btn("□", enabled.then_some(Message::WindowMaximize), false),
        Space::with_width(Length::Fixed(4.0)),
        window_btn("×", enabled.then_some(Message::WindowClose), true),
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

/// One window-management button. `on_press` is `None` to grey-out;
/// `destructive` flips the hover tint to the destructive accent.
fn window_btn<'a>(
    glyph: &str,
    on_press: Option<Message>,
    destructive: bool,
) -> Element<'a, Message> {
    let color = if on_press.is_some() {
        FG_TEXT
    } else {
        FG_MUTED
    };
    let mut btn = button(text(glyph.to_string()).size(14).color(color))
        .padding(Padding {
            top: 4.0,
            right: 8.0,
            bottom: 4.0,
            left: 8.0,
        })
        .style(if destructive {
            destructive_button_style
        } else {
            zone_button_style
        });
    if let Some(msg) = on_press {
        btn = btn.on_press(msg);
    }
    btn.into()
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

/// v4.0.1 BUG-7 — static clipboard-history tray icon. Spawns
/// `mde-popover clipboard` on press (same path as Super+V). Lives
/// outside the AppletKind enum because there's no applet
/// subprocess feeding text into it — the glyph is the static
/// Unicode clipboard codepoint until BUG-13's Carbon SVG wiring.
fn clipboard_button<'a>() -> Element<'a, Message> {
    button(text("\u{1F4CB}".to_string()).size(13).color(FG_TEXT))
        .padding(Padding {
            top: 6.0,
            right: 8.0,
            bottom: 6.0,
            left: 8.0,
        })
        .style(zone_button_style)
        .on_press(Message::ClipboardClicked)
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

/// v3.0.3 Tier 1E — destructive variant of the zone button style.
/// Used by the window-close button so its hover state reads as
/// "this closes." Shares the destructive color with
/// `crate::dismiss::close_button` in the popover crate.
fn destructive_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => Some(Background::Color(Color {
            r: 0.98,
            g: 0.31,
            b: 0.34,
            a: 0.20,
        })),
        button::Status::Pressed => Some(Background::Color(Color {
            r: 0.98,
            g: 0.31,
            b: 0.34,
            a: 0.35,
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
        let hero = crate::hero::Hero::new();
        let _ = view(&state, &hero, None);
    }

    /// v3.0.3 — view renders with a focused toplevel + populated
    /// hero so the new hero zone + window buttons exercise their
    /// happy-path branches too.
    #[test]
    fn view_renders_with_hero_and_focused() {
        let state = TopBarState::demo();
        let mut hero = crate::hero::Hero::new();
        hero.set_focused("Terminal".into(), "foot".into());
        let focused = crate::toplevels::Toplevel {
            id: 7,
            title: "Terminal".into(),
            app_id: "foot".into(),
            state: crate::toplevels::ToplevelState {
                focused: true,
                ..Default::default()
            },
        };
        let _ = view(&state, &hero, Some(&focused));
    }

    /// Window-button cluster: greys out (no on_press) when no
    /// toplevel is focused, takes message bindings when one is.
    /// Both render-paths exit cleanly.
    #[test]
    fn window_button_cluster_renders_both_states() {
        let _enabled: Element<'_, Message> = window_button_cluster(true);
        let _disabled: Element<'_, Message> = window_button_cluster(false);
    }
}
