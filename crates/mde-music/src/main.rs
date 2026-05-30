//! `mde-music` binary — AIR-10/11 shell.
//!
//! Renders the 7-card library hub + a breadcrumb the user navigates,
//! plus an Airsonic connection banner (from the shared creds). The live
//! grids behind each card + playback land with the `mde-musicd` data
//! path (AIR-10.b / AIR-2); this shell is the §0.12 runtime-reachable
//! entry point that makes the [`hub`]/[`nav`] models live.

use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Element, Length, Size, Task};

use mde_music::hub::HubCard;
use mde_music::library::{self, LibraryItem};
use mde_music::nav::{NavState, Route};
use mde_musicd::creds::{self, Creds};

fn main() -> iced::Result {
    iced::application(
        |_state: &State| String::from("MDE Music"),
        State::update,
        State::view,
    )
    .window_size(Size::new(1100.0, 720.0))
    .run_with(|| (State::new(), Task::none()))
}

/// The first-run "connect your Airsonic server" form, shown until valid
/// creds exist.
#[derive(Default)]
struct FirstRunForm {
    url: String,
    user: String,
    pass: String,
    error: Option<String>,
}

struct State {
    nav: NavState,
    /// `Some` until the operator connects a server (first run); `None`
    /// once creds exist and the library shell is shown.
    form: Option<FirstRunForm>,
    /// The Airsonic connection status line (set once connected).
    connection: String,
    /// The current category's items (fetched from the daemon over the Bus).
    items: Vec<LibraryItem>,
    /// True while a category fetch is in flight.
    loading: bool,
    /// Last fetch error (e.g. "daemon not responding"), shown in-pane.
    load_error: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    /// Open one of the seven hub categories.
    OpenCard(HubCard),
    /// Jump to a breadcrumb segment (0 = Library root).
    Ascend(usize),
    /// A category fetch resolved.
    ItemsLoaded(Vec<LibraryItem>),
    /// A category fetch failed (daemon down / no server).
    ItemsFailed(String),
    /// First-run form field edits.
    UrlChanged(String),
    UserChanged(String),
    PassChanged(String),
    /// Validate + save the first-run creds, then show the library.
    Connect,
}

impl State {
    fn new() -> Self {
        let (form, connection) = match creds::load() {
            Ok(c) => (None, format!("Connected to {}", c.server_url)),
            Err(_) => (Some(FirstRunForm::default()), String::new()),
        };
        Self { nav: NavState::new(), form, connection, items: Vec::new(), loading: false, load_error: None }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenCard(card) => {
                self.nav.push(Route::Category(card));
                self.items.clear();
                self.load_error = None;
                // Fetch the category from the daemon over the Bus (AIR-10.b)
                // when it's backed by a verb; the rest are AIR-4.b endpoints.
                if let Some(verb) = library::verb_for(card) {
                    self.loading = true;
                    Task::perform(library::fetch(verb), |r| match r {
                        Ok(items) => Message::ItemsLoaded(items),
                        Err(e) => Message::ItemsFailed(e),
                    })
                } else {
                    Task::none()
                }
            }
            Message::ItemsLoaded(items) => {
                self.items = items;
                self.loading = false;
                Task::none()
            }
            Message::ItemsFailed(e) => {
                self.items.clear();
                self.loading = false;
                self.load_error = Some(e);
                Task::none()
            }
            Message::Ascend(index) => {
                self.nav.ascend_to(index);
                Task::none()
            }
            Message::UrlChanged(s) => {
                if let Some(f) = &mut self.form {
                    f.url = s;
                }
                Task::none()
            }
            Message::UserChanged(s) => {
                if let Some(f) = &mut self.form {
                    f.user = s;
                }
                Task::none()
            }
            Message::PassChanged(s) => {
                if let Some(f) = &mut self.form {
                    f.pass = s;
                }
                Task::none()
            }
            Message::Connect => {
                if let Some(f) = &mut self.form {
                    if creds::is_valid(&f.url, &f.user) {
                        let c = Creds {
                            server_url: f.url.trim().to_string(),
                            username: f.user.trim().to_string(),
                            password: f.pass.clone(),
                        };
                        match creds::save(&c) {
                            Ok(()) => {
                                self.connection = format!("Connected to {}", c.server_url);
                                self.nav = NavState::new();
                                self.form = None;
                            }
                            Err(e) => f.error = Some(format!("Couldn't save: {e}")),
                        }
                    } else {
                        f.error = Some(
                            "Enter an http(s):// server URL and a username.".to_string(),
                        );
                    }
                }
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        if let Some(f) = &self.form {
            return self.first_run_view(f);
        }
        self.library_view()
    }

    /// The first-run connect form.
    fn first_run_view(&self, f: &FirstRunForm) -> Element<'_, Message> {
        let mut col = column![
            text("Connect your music").size(22),
            Space::with_height(Length::Fixed(8.0)),
            text("Point MDE Music at your Airsonic / Navidrome server.").size(13),
            Space::with_height(Length::Fixed(16.0)),
            text_input("https://music.your-mesh:4040", &f.url)
                .on_input(Message::UrlChanged),
            text_input("username", &f.user).on_input(Message::UserChanged),
            text_input("password", &f.pass)
                .secure(true)
                .on_input(Message::PassChanged),
            Space::with_height(Length::Fixed(12.0)),
            button(text("Connect")).on_press(Message::Connect),
        ]
        .spacing(8)
        .padding(28)
        .max_width(440);
        if let Some(err) = &f.error {
            col = col.push(Space::with_height(Length::Fixed(8.0)));
            col = col.push(text(err.clone()).size(13));
        }
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The library shell (hub + breadcrumb).
    fn library_view(&self) -> Element<'_, Message> {
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
            route => {
                let mut col = column![text(route.segment()).size(20)].spacing(6);
                if self.loading {
                    col = col.push(text("Loading…").size(13));
                } else if let Some(err) = &self.load_error {
                    col = col.push(text(err.clone()).size(13));
                } else if self.items.is_empty() {
                    col = col.push(
                        text("Nothing here yet — start mde-musicd to load your library.").size(13),
                    );
                } else {
                    for item in &self.items {
                        col = col.push(button(text(item.label.clone())));
                    }
                }
                col.into()
            }
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
