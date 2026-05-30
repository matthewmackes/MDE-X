//! `mde-music` binary — AIR-10/11 shell.
//!
//! Renders the 7-card library hub + a breadcrumb the user navigates,
//! plus an Airsonic connection banner (from the shared creds). The live
//! grids behind each card + playback land with the `mde-musicd` data
//! path (AIR-10.b / AIR-2); this shell is the §0.12 runtime-reachable
//! entry point that makes the [`hub`]/[`nav`] models live.

use iced::widget::{button, column, container, row, text, Space};
use iced::{Element, Length, Size, Task};

use mde_music::hub::HubCard;
use mde_music::nav::{NavState, Route};
use mde_musicd::creds;

fn main() -> iced::Result {
    iced::application(
        |_state: &State| String::from("MDE Music"),
        State::update,
        State::view,
    )
    .window_size(Size::new(1100.0, 720.0))
    .run_with(|| (State::new(), Task::none()))
}

struct State {
    nav: NavState,
    /// The Airsonic connection status line (read once at launch).
    connection: String,
}

#[derive(Debug, Clone)]
enum Message {
    /// Open one of the seven hub categories.
    OpenCard(HubCard),
    /// Jump to a breadcrumb segment (0 = Library root).
    Ascend(usize),
}

impl State {
    fn new() -> Self {
        let connection = match creds::load() {
            Ok(c) => format!("Connected to {}", c.server_url),
            Err(_) => {
                "No Airsonic server configured — run `mde-music --first-run` to connect".to_string()
            }
        };
        Self { nav: NavState::new(), connection }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenCard(card) => self.nav.push(Route::Category(card)),
            Message::Ascend(index) => self.nav.ascend_to(index),
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        // Breadcrumb — each segment is a button that ascends to it.
        let mut crumbs = row![].spacing(6);
        let segments = self.nav.breadcrumb();
        let last = segments.len().saturating_sub(1);
        for (i, seg) in segments.iter().enumerate() {
            if i > 0 {
                crumbs = crumbs.push(text("›"));
            }
            // The ellipsis isn't navigable; the current (last) segment is
            // shown as plain text.
            if seg == "…" || i == last {
                crumbs = crumbs.push(text(seg.clone()));
            } else {
                crumbs = crumbs.push(button(text(seg.clone())).on_press(Message::Ascend(i)));
            }
        }

        // Body — the hub renders its seven cards; a category page renders
        // an honest empty state until the daemon data path lands.
        let body: Element<'_, Message> = match self.nav.current() {
            Route::Hub => {
                let mut cards = column![].spacing(8);
                for card in HubCard::all() {
                    cards = cards
                        .push(button(text(card.label())).on_press(Message::OpenCard(card)));
                }
                cards.into()
            }
            route => column![
                text(route.segment()).size(20),
                Space::with_height(Length::Fixed(8.0)),
                text("Start mde-musicd to load this from your library."),
            ]
            .spacing(4)
            .into(),
        };

        container(
            column![
                text(&self.connection).size(13),
                Space::with_height(Length::Fixed(8.0)),
                crumbs,
                Space::with_height(Length::Fixed(16.0)),
                body,
            ]
            .padding(20)
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
