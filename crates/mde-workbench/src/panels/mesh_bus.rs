//! BUS-7.2 — Network → Mackes Bus panel.
//!
//! 5-tab skeleton: Topics / Subscriptions / Hooks / Audit / DND.
//! Each tab renders a meaningful empty state; BUS-7.3..BUS-7.6
//! fill in the content.
//!
//! Cite: docs/design/v6.x-mackes-bus.md §7 (operator surfaces);
//! ref: Linear (notification-settings tab bar).

use iced::widget::button::Status as ButtonStatus;
use iced::widget::{button, column, row, text, Space};
use iced::{alignment, Background, Border, Color, Element, Length, Task};
use mde_theme::{Density, EmptyState, FontSize, Icon, Palette, Radii, TypeRole};

use crate::panel_chrome::{empty_state, panel_container};

/// The five Bus panel tabs, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Topics,
    Subscriptions,
    Hooks,
    Audit,
    Dnd,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Self::Topics => "Topics",
            Self::Subscriptions => "Subscriptions",
            Self::Hooks => "Hooks",
            Self::Audit => "Audit",
            Self::Dnd => "DND",
        }
    }

    const ALL: [Tab; 5] = [
        Tab::Topics,
        Tab::Subscriptions,
        Tab::Hooks,
        Tab::Audit,
        Tab::Dnd,
    ];
}

#[derive(Debug, Clone, Default)]
pub struct MeshBusPanel {
    pub active_tab: Tab,
}

#[derive(Debug, Clone)]
pub enum Message {
    SelectTab(Tab),
}

impl MeshBusPanel {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, msg: Message) -> Task<crate::Message> {
        match msg {
            Message::SelectTab(tab) => {
                self.active_tab = tab;
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, crate::Message> {
        let palette = Palette::dark();
        let density = Density::Comfortable;
        let sizes = FontSize::defaults();
        let radii = Radii::defaults();
        let accent = palette.accent.into_iced_color();
        let raised = palette.raised.into_iced_color();

        let title = text("Mackes Bus")
            .size(TypeRole::Display.size_in(sizes))
            .color(palette.text.into_iced_color());

        let subtitle = text("Per-peer notification distribution · ntfy over Nebula")
            .size(TypeRole::Body.size_in(sizes))
            .color(palette.text_muted.into_iced_color());

        let tab_bar: Element<'_, crate::Message> = {
            let r = f32::from(radii.sm);
            let buttons: Vec<Element<'_, crate::Message>> = Tab::ALL
                .iter()
                .map(|&tab| {
                    let is_active = tab == self.active_tab;
                    let (bg, fg) = if is_active {
                        (accent, Color::WHITE)
                    } else {
                        (Color::TRANSPARENT, palette.text.into_iced_color())
                    };
                    button(
                        text(tab.label())
                            .size(TypeRole::Body.size_in(sizes))
                            .color(fg),
                    )
                    .padding([6u16, 14u16])
                    .style(move |_t, status: ButtonStatus| {
                        let fill = match (is_active, status) {
                            (true, _) => bg,
                            (false, ButtonStatus::Hovered) => Color {
                                r: accent.r,
                                g: accent.g,
                                b: accent.b,
                                a: 0.08,
                            },
                            _ => bg,
                        };
                        button::Style {
                            background: Some(Background::Color(fill)),
                            text_color: fg,
                            border: Border {
                                color: Color::TRANSPARENT,
                                width: 0.0,
                                radius: r.into(),
                            },
                            shadow: iced::Shadow::default(),
                        }
                    })
                    .on_press(crate::Message::MeshBus(Message::SelectTab(tab)))
                    .into()
                })
                .collect();

            row(buttons).spacing(4).into()
        };

        let tab_separator = {
            use iced::widget::container;
            container(Space::new(Length::Fill, Length::Fixed(1.0)))
                .style(move |_t: &iced::Theme| iced::widget::container::Style {
                    background: Some(Background::Color(raised)),
                    ..Default::default()
                })
                .width(Length::Fill)
                .height(Length::Fixed(1.0))
        };

        let body: Element<'_, crate::Message> = match self.active_tab {
            Tab::Topics => empty_state(
                EmptyState::info(
                    "No topics active yet",
                    "Publish a message or start a webhook to create the first topic.",
                )
                .with_icon(Icon::Notification),
                palette,
                || crate::Message::Noop,
            ),
            Tab::Subscriptions => empty_state(
                EmptyState::info(
                    "No subscriptions configured",
                    "Add a subscription in subs.yaml to start receiving messages on this peer.",
                )
                .with_icon(Icon::Network),
                palette,
                || crate::Message::Noop,
            ),
            Tab::Hooks => empty_state(
                EmptyState::info(
                    "No webhook rules configured",
                    "Add a rule to bus-hooks.yaml to route incoming webhook events to topics.",
                )
                .with_icon(Icon::Settings),
                palette,
                || crate::Message::Noop,
            ),
            Tab::Audit => empty_state(
                EmptyState::info(
                    "No audit events recorded",
                    "Bus activity will appear here as messages flow through the broker.",
                )
                .with_icon(Icon::History),
                palette,
                || crate::Message::Noop,
            ),
            Tab::Dnd => empty_state(
                EmptyState::info(
                    "Do Not Disturb is off",
                    "Run `mde-bus dnd on` to suppress non-urgent alerts across the mesh.",
                )
                .with_icon(Icon::Notification),
                palette,
                || crate::Message::Noop,
            ),
        };

        let header = column![title, subtitle].spacing(4);

        let content = column![
            header,
            Space::with_height(12),
            tab_bar,
            tab_separator,
            Space::with_height(16),
            body,
        ]
        .spacing(0)
        .align_x(alignment::Horizontal::Left);

        panel_container(content.into(), density)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tab_is_topics() {
        let panel = MeshBusPanel::new();
        assert_eq!(panel.active_tab, Tab::Topics);
    }

    #[test]
    fn select_tab_updates_active() {
        let mut panel = MeshBusPanel::new();
        let _ = panel.update(Message::SelectTab(Tab::Subscriptions));
        assert_eq!(panel.active_tab, Tab::Subscriptions);
        let _ = panel.update(Message::SelectTab(Tab::Dnd));
        assert_eq!(panel.active_tab, Tab::Dnd);
    }

    #[test]
    fn all_tabs_cycle_without_panic() {
        let mut panel = MeshBusPanel::new();
        for tab in Tab::ALL {
            let _ = panel.update(Message::SelectTab(tab));
            assert_eq!(panel.active_tab, tab);
        }
    }

    #[test]
    fn tab_labels_are_non_empty() {
        for tab in Tab::ALL {
            assert!(!tab.label().is_empty());
        }
    }

    #[test]
    fn five_tabs_declared() {
        assert_eq!(Tab::ALL.len(), 5);
    }
}
