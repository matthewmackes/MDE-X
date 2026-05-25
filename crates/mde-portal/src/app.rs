//! Portal-1 Iced application — minimal layer-shell Dock placeholder.
//!
//! Renders a solid charcoal (dark) 56 px bottom strip as the Dock
//! baseline. Portal-2 adds the exclusive zone and per-output binding.
//! Portal-4 onward fills in the nav buttons and interactive segments.

use iced::widget::container;
use iced::{Element, Length, Task, Theme};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings};
use iced_layershell::to_layer_message;

/// Crate-private app-id constant visible to the layer-shell compositor.
pub(crate) const APP_ID: &str = "dev.mackes.MDE.Portal";

/// Height of the Dock strip in logical pixels (Portal-2 lock).
pub const DOCK_HEIGHT_PX: u32 = 56;

/// Charcoal background colour — `#202124` per Classic ChromeOS visual lock.
const CHARCOAL: iced::Color = iced::Color {
    r: 0.125_f32,
    g: 0.129_f32,
    b: 0.141_f32,
    a: 1.0,
};

/// Messages the Dock application handles.
///
/// `#[to_layer_message]` generates the `TryInto<LayershellCustomActions>`
/// impl required by `iced_layershell::Application::run`.
#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    /// No-op placeholder — Portal-4 onward adds domain-specific variants.
    Noop,
}

/// Minimal Dock application state (Portal-1 placeholder).
#[derive(Debug, Default)]
pub struct DockApp;

impl DockApp {
    /// Construct iced_layershell settings for the Dock surface.
    pub fn settings() -> Settings<()> {
        Settings {
            layer_settings: LayerShellSettings {
                size: Some((0, DOCK_HEIGHT_PX)),
                exclusive_zone: DOCK_HEIGHT_PX as i32,
                anchor: Anchor::Bottom | Anchor::Left | Anchor::Right,
                layer: Layer::Top,
                keyboard_interactivity: KeyboardInteractivity::None,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

impl iced_layershell::Application for DockApp {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Task<Message>) {
        (Self, Task::none())
    }

    fn namespace(&self) -> String {
        APP_ID.to_string()
    }

    fn update(&mut self, _message: Message) -> Task<Message> {
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        container(iced::widget::Space::new(Length::Fill, Length::Fill))
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(CHARCOAL)),
                ..Default::default()
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_height_is_56() {
        assert_eq!(DOCK_HEIGHT_PX, 56, "Portal-2 lock: Dock is 56 px");
    }

    #[test]
    fn charcoal_matches_chromeos_lock() {
        // Classic ChromeOS visual lock: #202124 = rgb(32, 33, 36)
        // ≈ (0.125, 0.129, 0.141) in [0,1].
        let r = (CHARCOAL.r * 255.0).round() as u8;
        let g = (CHARCOAL.g * 255.0).round() as u8;
        let b = (CHARCOAL.b * 255.0).round() as u8;
        assert_eq!((r, g, b), (32, 33, 36), "#202124 charcoal");
    }

    #[test]
    fn app_id_is_portal_bus_name() {
        assert_eq!(APP_ID, "dev.mackes.MDE.Portal");
    }
}
