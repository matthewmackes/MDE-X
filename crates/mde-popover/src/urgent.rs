//! `mde-popover urgent` — BUS-2.5 theater takeover for `urgent`
//! Bus messages. Fullscreen `Layer::Overlay` that paints a near-opaque
//! charcoal backdrop + a centered urgent card (⚠ + title + body) over
//! every other surface, capturing keyboard exclusively. Esc / Enter /
//! click dismisses (LOCAL ack; cross-peer first-to-ack-wins is BUS-6.4).
//! A one-shot alert sound plays on open.
//!
//! The message arrives via env vars the spawner sets — the mde-portal
//! Dock, on a `priority=urgent` Bus segment, sets `MDE_URGENT_TITLE` +
//! `MDE_URGENT_BODY` then spawns `mde-popover urgent` (mirrors the WM-3
//! window-actions / icon-mapper env-var hand-off pattern). A full-screen
//! takeover cannot be the 56 px Dock, so it is its own surface.

#![forbid(unsafe_code)]

use iced::widget::{column, container, mouse_area, row, text, Space};
use iced::widget::container::Style as ContainerStyle;
use iced::{Background, Border, Color, Element, Length, Padding, Shadow, Subscription, Task, Theme};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings};
use iced_layershell::to_layer_message;

// ── Design tokens (Classic ChromeOS, mirrors lock.rs) ─────────────────────────

/// Near-opaque charcoal dim — the takeover obscures the desktop.
const BACKDROP: Color = Color { r: 0.086, g: 0.090, b: 0.102, a: 0.97 };
const FG: Color = Color::WHITE;
const FG_DIM: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.70 };
const FG_LABEL: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 0.40 };
/// Urgent red (matches the status-panel danger token).
const URGENT_RED: Color = Color { r: 0.86, g: 0.21, b: 0.16, a: 1.0 };

const ENV_TITLE: &str = "MDE_URGENT_TITLE";
const ENV_BODY: &str = "MDE_URGENT_BODY";
/// BUS-2.7.c — JSON array of `{label,url}` action buttons the Dock passes.
const ENV_ACTIONS: &str = "MDE_URGENT_ACTIONS";
/// BUS-2.7.c — max action buttons rendered (`v6.x-mackes-bus.md` §9).
const MAX_URGENT_ACTIONS: usize = 5;

/// Read the urgent message from the spawner-set env vars. A blank or
/// missing title falls back to generic copy so the surface is never
/// empty; a missing body renders title-only.
fn read_urgent_message() -> (String, String) {
    let title = std::env::var(ENV_TITLE)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Urgent".to_string());
    let body = std::env::var(ENV_BODY)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_default();
    (title, body)
}

/// BUS-2.7.c — parse the optional action buttons the Dock passes as a
/// JSON array in `MDE_URGENT_ACTIONS` (e.g.
/// `[{"label":"Resolve","url":"mde://meshfs/resolve/x"}]`). Missing /
/// blank / malformed env → no buttons. Entries missing label or url are
/// skipped; the list is capped at `MAX_URGENT_ACTIONS` per the §9 lock.
fn read_urgent_actions() -> Vec<(String, String)> {
    let raw = match std::env::var(ENV_ACTIONS) {
        Ok(s) if !s.trim().is_empty() => s,
        _ => return Vec::new(),
    };
    let Ok(serde_json::Value::Array(arr)) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|a| {
            let label = a.get("label").and_then(|v| v.as_str())?.to_string();
            let url = a.get("url").and_then(|v| v.as_str())?.to_string();
            Some((label, url))
        })
        .take(MAX_URGENT_ACTIONS)
        .collect()
}

/// Best-effort one-shot urgent alert sound. Degrades to a logged no-op
/// when `canberra-gtk-play` isn't installed — the visual takeover is
/// the source of truth, the sound is an enhancement.
fn play_urgent_alert() {
    if std::process::Command::new("canberra-gtk-play")
        .args(["-i", "dialog-error"])
        .spawn()
        .is_err()
    {
        tracing::debug!("urgent theater: canberra-gtk-play unavailable; sound skipped");
    }
}

// ── Application ───────────────────────────────────────────────────────────────

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    Exit,
    /// BUS-2.7.c — operator clicked an action button; dispatch the url
    /// via `mde-open`, then exit (the exclusive-keyboard overlay must
    /// drop so the dispatched surface is reachable).
    OpenAction(String),
}

pub struct App {
    title: String,
    body: String,
    /// BUS-2.7.c — (label, url) action buttons from `MDE_URGENT_ACTIONS`.
    actions: Vec<(String, String)>,
}

fn namespace() -> String {
    "mde-popover-urgent".to_string()
}

fn update(_state: &mut App, msg: Message) -> Task<Message> {
    match msg {
        Message::Exit => std::process::exit(0),
        Message::OpenAction(url) => {
            // BUS-2.7.c — dispatch via `mde-open`, then drop the overlay
            // so the opened surface isn't trapped behind this exclusive-
            // keyboard takeover. Fire-and-forget spawn.
            let _ = std::process::Command::new("mde-open").arg(&url).spawn();
            std::process::exit(0);
        }
        _ => {}
    }
    Task::none()
}

/// BUS-2.7.c — one urgent-theater action button. Click dispatches `url`
/// via `mde-open` (the surface then exits). Outlined in the urgent-red
/// token so it reads as the card's primary control; the inner button
/// consumes the click so the backdrop's dismiss doesn't also fire.
fn action_button<'a>(label: &str, url: &str) -> Element<'a, Message> {
    let url = url.to_string();
    iced::widget::Button::new(text(label.to_string()).size(16).color(FG))
        .padding(Padding::from([8, 20]))
        .style(|_t: &Theme, status: iced::widget::button::Status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Color { a: 0.30, ..URGENT_RED },
                _ => Color { a: 0.06, ..FG },
            };
            iced::widget::button::Style {
                background: Some(Background::Color(bg)),
                text_color: FG,
                border: Border { color: URGENT_RED, width: 1.5, radius: 6.0.into() },
                shadow: Shadow::default(),
                snap: false,
            }
        })
        .on_press(Message::OpenAction(url))
        .into()
}

fn view(state: &App) -> Element<'_, Message> {
    let marker: Element<'_, Message> = text("⚠").size(64).color(URGENT_RED).into();
    let title: Element<'_, Message> = text(state.title.clone()).size(32).color(FG).into();

    let mut card = column![marker, Space::new().height(Length::Fixed(16.0)), title]
        .align_x(iced::Alignment::Center);
    if !state.body.is_empty() {
        card = card.push(Space::new().height(Length::Fixed(12.0)));
        card = card.push(text(state.body.clone()).size(16).color(FG_DIM));
    }

    let footer: Element<'_, Message> = text("Press Esc or Enter to dismiss")
        .size(12)
        .color(FG_LABEL)
        .into();

    let mut centered = column![
        Space::new().height(Length::Fill),
        card,
        Space::new().height(Length::Fixed(32.0)),
    ];
    // BUS-2.7.c — action buttons (e.g. "Resolve") above the dismiss hint.
    if !state.actions.is_empty() {
        let mut actions_row = row![].spacing(12);
        for (label, url) in &state.actions {
            actions_row = actions_row.push(action_button(label, url));
        }
        centered = centered
            .push(actions_row)
            .push(Space::new().height(Length::Fixed(24.0)));
    }
    let centered = centered
        .push(footer)
        .push(Space::new().height(Length::Fill))
        .align_x(iced::Alignment::Center);

    mouse_area(
        container(centered)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding::from([0, 48]))
            .style(|_: &Theme| ContainerStyle {
                background: Some(Background::Color(BACKDROP)),
                ..Default::default()
            }),
    )
    .on_press(Message::Exit)
    .into()
}

fn subscription(_state: &App) -> Subscription<Message> {
    use iced::event;
    event::listen_with(|event, status, _window| {
        use iced::keyboard;
        match event {
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                if status == event::Status::Ignored =>
            {
                use iced::keyboard::{key::Named, Key};
                match key {
                    Key::Named(Named::Escape) | Key::Named(Named::Enter) => Some(Message::Exit),
                    _ => None,
                }
            }
            _ => None,
        }
    })
}

pub fn run() -> iced_layershell::Result {
    iced_layershell::application(
        || {
            let (title, body) = read_urgent_message();
            let actions = read_urgent_actions();
            tracing::info!(%title, actions = actions.len(), "urgent theater open");
            play_urgent_alert();
            App { title, body, actions }
        },
        namespace,
        update,
        view,
    )
    .theme(|_: &App| iced::Theme::Dark)
    .subscription(subscription)
    .settings(Settings {
        id: Some("mde-popover-urgent".to_string()),
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
    .run()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    // `set_var`/`remove_var` mutate process-global state; serialize the
    // env-touching tests so they don't race each other.
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn read_urgent_message_falls_back_when_unset() {
        let _g = env_guard();
        std::env::remove_var(ENV_TITLE);
        std::env::remove_var(ENV_BODY);
        let (t, b) = read_urgent_message();
        assert_eq!(t, "Urgent");
        assert!(b.is_empty());
    }

    #[test]
    fn read_urgent_message_reads_env() {
        let _g = env_guard();
        std::env::set_var(ENV_TITLE, "Disk full");
        std::env::set_var(ENV_BODY, "peer-3 root at 98%");
        let (t, b) = read_urgent_message();
        assert_eq!(t, "Disk full");
        assert_eq!(b, "peer-3 root at 98%");
        std::env::remove_var(ENV_TITLE);
        std::env::remove_var(ENV_BODY);
    }

    #[test]
    fn read_urgent_message_treats_blank_title_as_unset() {
        let _g = env_guard();
        std::env::set_var(ENV_TITLE, "   ");
        let (t, _) = read_urgent_message();
        assert_eq!(t, "Urgent");
        std::env::remove_var(ENV_TITLE);
    }

    #[test]
    fn read_urgent_actions_parses_json() {
        // BUS-2.7.c — the MESHFS-conflict → "Resolve" urgent use-case.
        let _g = env_guard();
        std::env::set_var(
            ENV_ACTIONS,
            r#"[{"label":"Resolve","url":"mde://meshfs/resolve/x"}]"#,
        );
        let a = read_urgent_actions();
        std::env::remove_var(ENV_ACTIONS);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].0, "Resolve");
        assert_eq!(a[0].1, "mde://meshfs/resolve/x");
    }

    #[test]
    fn read_urgent_actions_empty_when_unset() {
        let _g = env_guard();
        std::env::remove_var(ENV_ACTIONS);
        assert!(read_urgent_actions().is_empty());
    }

    #[test]
    fn read_urgent_actions_empty_on_malformed() {
        let _g = env_guard();
        std::env::set_var(ENV_ACTIONS, "not json");
        let a = read_urgent_actions();
        std::env::remove_var(ENV_ACTIONS);
        assert!(a.is_empty());
    }

    #[test]
    fn read_urgent_actions_skips_malformed_and_caps_at_five() {
        let _g = env_guard();
        // one entry missing `url` (dropped) + 6 well-formed → 5 after cap.
        let mut items = vec![r#"{"label":"NoUrl"}"#.to_string()];
        for i in 0..6 {
            items.push(format!(r#"{{"label":"a{i}","url":"mde://x/{i}"}}"#));
        }
        std::env::set_var(ENV_ACTIONS, format!("[{}]", items.join(",")));
        let a = read_urgent_actions();
        std::env::remove_var(ENV_ACTIONS);
        assert_eq!(a.len(), MAX_URGENT_ACTIONS);
        assert_eq!(a[0].0, "a0", "the malformed NoUrl entry was skipped");
    }
}
