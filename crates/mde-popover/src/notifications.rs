//! Notifications popover — recent notifications list.
//!
//! Anchored bottom-right of the primary output above the panel.
//! Reads `~/.cache/mackes/notifications.json` (the same cache the
//! notification-bell applet polls) and renders the rows grouped by
//! peer, with phone-origin rows badged via the locked glyph.

use std::fs;
use std::path::{Path, PathBuf};

use iced::widget::{column, container, mouse_area, row, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding, Shadow, Task, Theme};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings};
use iced_layershell::to_layer_message;
use mde_applet_notifications::{
    group_and_sort, group_by_app, is_phone_origin, notifications_cache_path,
    parse_notifications, visible, NotificationRow,
};

const WIDTH: u32 = 480;
const HEIGHT: u32 = 600;

// BUS-2.3 — priority accent colors (Carbon red/orange/blue, matching BUS-2.2).
const BUS_URGENT_COLOR: Color = Color { r: 0.91, g: 0.30, b: 0.36, a: 1.0 };
const BUS_HIGH_COLOR: Color   = Color { r: 0.93, g: 0.55, b: 0.21, a: 1.0 };
const BUS_DEFAULT_COLOR: Color = Color { r: 0.20, g: 0.69, b: 1.00, a: 1.0 };

const ACCENT: Color = Color {
    r: 0.169,
    g: 0.604,
    b: 0.953,
    a: 1.0,
};
const FG_TEXT: Color = Color {
    r: 0.957,
    g: 0.957,
    b: 0.957,
    a: 1.0,
};
const FG_FAINT: Color = Color {
    r: 0.45,
    g: 0.45,
    b: 0.45,
    a: 1.0,
};

const FG_MUTED: Color = Color {
    r: 0.659,
    g: 0.659,
    b: 0.659,
    a: 1.0,
};
const SURFACE_BG: Color = Color {
    r: 0.055,
    g: 0.055,
    b: 0.063,
    a: 0.97,
};

// ──────────────────────────────────────────────────────────────
// BUS-2.3 — Bus message integration (pure data layer)
// ──────────────────────────────────────────────────────────────

/// A Bus message loaded from the GFS file tree for popover display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusPopoverMessage {
    pub ulid: String,
    pub topic: String,
    pub priority: String,
    pub title: String,
    pub body: String,
}

/// Resolve `$XDG_DATA_HOME/mde/bus` (or `~/.local/share/mde/bus`).
#[must_use]
pub fn bus_data_root() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("mde").join("bus"))
}

/// Parse one Bus JSON file from the store.
/// Returns `None` for `min`-priority or malformed files.
/// Mirrors `parse_breadcrumb_file` from `mde-portal/src/workspace.rs`.
#[must_use]
pub fn parse_bus_message(path: &Path, ulid: &str, topic: &str) -> Option<BusPopoverMessage> {
    let raw = std::fs::read_to_string(path).ok()?;
    let outer: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let priority = outer
        .get("priority")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();
    if priority == "min" {
        return None;
    }
    let title = outer
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let body = outer
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(BusPopoverMessage { ulid: ulid.to_string(), topic: topic.to_string(), priority, title, body })
}

/// Walk `dir` recursively, collecting `(topic, ulid, path)` for each
/// ULID-named `.json` file. Topic is the relative path of the parent
/// directory from `bus_root` with `/` separators.
fn collect_bus_files(dir: &Path, bus_root: &Path, out: &mut Vec<(String, String, PathBuf)>) {
    let Ok(iter) = std::fs::read_dir(dir) else { return };
    for entry in iter.flatten() {
        let path = entry.path();
        let name_os = entry.file_name();
        let name = name_os.to_str().unwrap_or("");
        if path.is_dir() {
            if !name.starts_with('.') {
                collect_bus_files(&path, bus_root, out);
            }
        } else if name.ends_with(".json") {
            let parent = path.parent().unwrap_or(bus_root);
            let rel = parent
                .strip_prefix(bus_root)
                .map(|p| p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
                .unwrap_or_default();
            // Skip top-level (index.sqlite sibling) and audit/ paths.
            if rel.is_empty() || rel.starts_with("audit") {
                continue;
            }
            let ulid = name.trim_end_matches(".json").to_string();
            out.push((rel, ulid, path));
        }
    }
}

/// Load all displayable Bus messages from `bus_root`, skipping
/// `min`-priority, audit topics, and malformed files.
#[must_use]
pub fn load_bus_messages(bus_root: &Path) -> Vec<BusPopoverMessage> {
    let mut triples: Vec<(String, String, PathBuf)> = Vec::new();
    collect_bus_files(bus_root, bus_root, &mut triples);
    triples
        .into_iter()
        .filter_map(|(topic, ulid, path)| parse_bus_message(&path, &ulid, &topic))
        .collect()
}

/// Partition `messages` (excluding `acked` ULIDs) into
/// `(urgent, high, default)` buckets, newest-first within each.
#[must_use]
pub fn bucket_by_priority<'a>(
    messages: &'a [BusPopoverMessage],
    acked: &std::collections::HashSet<String>,
) -> (Vec<&'a BusPopoverMessage>, Vec<&'a BusPopoverMessage>, Vec<&'a BusPopoverMessage>) {
    let active: Vec<&BusPopoverMessage> =
        messages.iter().filter(|m| !acked.contains(&m.ulid)).collect();
    let mut urgent: Vec<&BusPopoverMessage> =
        active.iter().copied().filter(|m| m.priority == "urgent").collect();
    let mut high: Vec<&BusPopoverMessage> =
        active.iter().copied().filter(|m| m.priority == "high").collect();
    let mut default: Vec<&BusPopoverMessage> = active
        .iter()
        .copied()
        .filter(|m| m.priority != "urgent" && m.priority != "high")
        .collect();
    for bucket in [&mut urgent, &mut high, &mut default] {
        bucket.sort_by(|a, b| b.ulid.cmp(&a.ulid));
    }
    (urgent, high, default)
}

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    Exit,
    /// BUG-8.a (2026-05-23) — clear the cache file then exit.
    ClearAll,
    /// BUG-8.b (2026-05-23) — toggle the mute state for a peer
    /// group. Writes the new state to
    /// `~/.config/mde/notification-mutes.toml` and refreshes the
    /// in-memory groups list so muted peers disappear
    /// immediately.
    ToggleMute(String),
    /// BUG-8.c (2026-05-23) — flip between peer-grouped and
    /// app-grouped layouts.
    ToggleGroupMode,
    /// BUG-8.c — collapse / expand a single group bucket. Key
    /// is the bucket label (peer name or app_id).
    ToggleCollapse(String),
    /// BUS-2.3 — move a Bus message ULID to the acked-list.
    AckBusMessage(String),
}

/// BUG-8.c — group layout selector. Default is `Peer` so existing
/// users see the previously-shipped layout without surprise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupMode {
    Peer,
    App,
}

pub struct App {
    groups: Vec<(String, Vec<NotificationRow>)>,
    /// BUG-8.b — set of peer names currently muted. Backed by
    /// `~/.config/mde/notification-mutes.toml`. When a peer is
    /// in this set, its group is filtered out of `groups`
    /// before render.
    muted_peers: std::collections::HashSet<String>,
    /// BUG-8.c — active group-by selector. Survives a single
    /// popover open; not persisted across sessions (the user
    /// re-clicks "By app" each time they want it, matching
    /// nm-connection-editor-style transient view options).
    group_mode: GroupMode,
    /// BUG-8.c — bucket keys the user has collapsed. Persists
    /// for the popover's lifetime. Click the header to flip.
    collapsed: std::collections::HashSet<String>,
    /// BUS-2.3 — Bus messages loaded from the GFS file tree at open.
    bus_messages: Vec<BusPopoverMessage>,
    /// BUS-2.3 — ULIDs the operator has acked; filtered from the
    /// active buckets and shown in the acked-list section instead.
    bus_acked: std::collections::HashSet<String>,
}

impl iced_layershell::Application for App {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Task<Message>) {
        let muted_peers = load_muted_peers();
        let group_mode = GroupMode::Peer;
        let groups = load_groups_for(group_mode, &muted_peers);
        let bus_messages = bus_data_root()
            .map(|root| load_bus_messages(&root))
            .unwrap_or_default();
        tracing::info!(
            group_count = groups.len(),
            muted = muted_peers.len(),
            bus_messages = bus_messages.len(),
            "notifications popover open"
        );
        (
            Self {
                groups,
                muted_peers,
                group_mode,
                collapsed: std::collections::HashSet::new(),
                bus_messages,
                bus_acked: std::collections::HashSet::new(),
            },
            Task::none(),
        )
    }

    fn namespace(&self) -> String {
        "mde-popover-notifications".to_string()
    }

    fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Exit => std::process::exit(0),
            Message::ClearAll => {
                // BUG-8.a — empty the cache file (atomic via
                // write to "") so the next open of any source
                // re-reads zero notifications. Then exit so
                // the operator sees the cleared state on next
                // open.
                let path = notifications_cache_path();
                let _ = std::fs::write(&path, "");
                std::process::exit(0);
            }
            Message::ToggleMute(peer) => {
                // BUG-8.b — flip the mute state for `peer`,
                // persist to ~/.config/mde/notification-mutes.toml,
                // and refresh the in-memory groups so the peer's
                // rows disappear (or reappear) immediately.
                if self.muted_peers.contains(&peer) {
                    self.muted_peers.remove(&peer);
                } else {
                    self.muted_peers.insert(peer);
                }
                let _ = save_muted_peers(&self.muted_peers);
                self.groups = load_groups_for(self.group_mode, &self.muted_peers);
                Task::none()
            }
            Message::ToggleGroupMode => {
                self.group_mode = match self.group_mode {
                    GroupMode::Peer => GroupMode::App,
                    GroupMode::App => GroupMode::Peer,
                };
                // Reset collapses on mode flip — the bucket
                // keys mean different things across modes.
                self.collapsed.clear();
                self.groups = load_groups_for(self.group_mode, &self.muted_peers);
                Task::none()
            }
            Message::ToggleCollapse(key) => {
                if self.collapsed.contains(&key) {
                    self.collapsed.remove(&key);
                } else {
                    self.collapsed.insert(key);
                }
                Task::none()
            }
            Message::AckBusMessage(ulid) => {
                self.bus_acked.insert(ulid);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let header = text("Notifications").size(14).color(FG_TEXT);
        let total_rows: usize = self.groups.iter().map(|(_, r)| r.len()).sum();
        let subhead = text(format!("{total_rows} total"))
            .size(11)
            .color(FG_MUTED);

        let mut list = column![].spacing(10);
        if self.groups.is_empty() {
            list = list.push(
                container(text("No notifications").size(13).color(FG_MUTED))
                    .padding(Padding {
                        top: 28.0,
                        right: 0.0,
                        bottom: 0.0,
                        left: 0.0,
                    }),
            );
        }
        for (group_name, rows) in &self.groups {
            let label_text = if group_name.is_empty() {
                "Local".to_string()
            } else {
                group_name.clone()
            };
            // BUG-8.c — collapsed flag drives the chevron glyph
            // + body visibility.
            let is_collapsed = self.collapsed.contains(group_name);
            let chevron = if is_collapsed { "▶" } else { "▼" };
            let header_label = format!("{chevron}  {label_text}  ({})", rows.len());
            let collapse_key = group_name.clone();
            let header_btn: Element<'_, Message> = iced::widget::Button::new(
                text(header_label).size(11).color(FG_TEXT),
            )
            .padding(Padding {
                top: 2.0,
                right: 8.0,
                bottom: 2.0,
                left: 8.0,
            })
            .style(|_t: &Theme, status: iced::widget::button::Status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => Color {
                        r: 0.14,
                        g: 0.14,
                        b: 0.16,
                        a: 1.0,
                    },
                    _ => Color::TRANSPARENT,
                };
                iced::widget::button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: FG_TEXT,
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 3.0.into(),
                    },
                    shadow: Shadow::default(),
                }
            })
            .on_press(Message::ToggleCollapse(collapse_key))
            .into();

            // BUG-8.b — per-peer Mute button only makes sense in
            // peer-grouped mode; hide it in app-grouped mode
            // (muting "firefox" doesn't have the same wire
            // semantics — that would be a future BUG-8.d).
            let mute_btn: Element<'_, Message> = if self.group_mode == GroupMode::Peer {
                let peer_for_mute = group_name.clone();
                iced::widget::Button::new(text("Mute").size(10).color(FG_MUTED))
                    .padding(Padding {
                        top: 2.0,
                        right: 8.0,
                        bottom: 2.0,
                        left: 8.0,
                    })
                    .style(|_t: &Theme, status: iced::widget::button::Status| {
                        let bg = match status {
                            iced::widget::button::Status::Hovered => Color {
                                r: 0.18,
                                g: 0.18,
                                b: 0.20,
                                a: 1.0,
                            },
                            _ => Color::TRANSPARENT,
                        };
                        iced::widget::button::Style {
                            background: Some(Background::Color(bg)),
                            text_color: FG_MUTED,
                            border: Border {
                                color: Color {
                                    a: 0.12,
                                    ..Color::WHITE
                                },
                                width: 1.0,
                                radius: 3.0.into(),
                            },
                            shadow: Shadow::default(),
                        }
                    })
                    .on_press(Message::ToggleMute(peer_for_mute))
                    .into()
            } else {
                Space::with_width(Length::Fixed(0.0)).into()
            };

            let group_header = row![
                header_btn,
                Space::with_width(Length::Fill),
                mute_btn,
            ]
            .align_y(iced::Alignment::Center);

            let mut group_column = column![group_header].spacing(4);
            if !is_collapsed {
                for row_data in rows.iter().take(40) {
                    group_column = group_column.push(render_row(row_data));
                }
            }
            list = list.push(group_column);
        }

        // BUS-2.3 — Bus Messages section (priority-bucketed, below FDO).
        {
            let (urgent, high, default) = bucket_by_priority(&self.bus_messages, &self.bus_acked);
            let acked_msgs: Vec<&BusPopoverMessage> = self
                .bus_messages
                .iter()
                .filter(|m| self.bus_acked.contains(&m.ulid))
                .collect();
            let has_bus = !urgent.is_empty() || !high.is_empty() || !default.is_empty() || !acked_msgs.is_empty();
            if !self.bus_messages.is_empty() || has_bus {
                // Section divider + header
                list = list.push(Space::with_height(Length::Fixed(8.0)));
                list = list.push(
                    container(Space::with_height(Length::Fixed(1.0)))
                        .width(Length::Fill)
                        .style(|_: &Theme| container::Style {
                            background: Some(iced::Background::Color(Color { r: 1.0, g: 1.0, b: 1.0, a: 0.08 })),
                            ..Default::default()
                        }),
                );
                list = list.push(Space::with_height(Length::Fixed(6.0)));
                let bus_active_total = urgent.len() + high.len() + default.len();
                list = list.push(
                    text(format!("Bus Messages  ({bus_active_total} active)"))
                        .size(11)
                        .color(FG_FAINT),
                );
                // Urgent bucket
                if !urgent.is_empty() {
                    list = list.push(
                        text(format!("⚠ Urgent  ({})", urgent.len()))
                            .size(10)
                            .color(BUS_URGENT_COLOR),
                    );
                    for msg in urgent.iter().take(20) {
                        list = list.push(render_bus_row(msg));
                    }
                }
                // High bucket
                if !high.is_empty() {
                    list = list.push(
                        text(format!("! High  ({})", high.len()))
                            .size(10)
                            .color(BUS_HIGH_COLOR),
                    );
                    for msg in high.iter().take(20) {
                        list = list.push(render_bus_row(msg));
                    }
                }
                // Default bucket
                if !default.is_empty() {
                    list = list.push(
                        text(format!("Default  ({})", default.len()))
                            .size(10)
                            .color(BUS_DEFAULT_COLOR),
                    );
                    for msg in default.iter().take(20) {
                        list = list.push(render_bus_row(msg));
                    }
                }
                // Empty state
                if bus_active_total == 0 && acked_msgs.is_empty() {
                    list = list.push(
                        container(text("No bus messages").size(13).color(FG_MUTED))
                            .padding(Padding { top: 6.0, right: 0.0, bottom: 0.0, left: 0.0 }),
                    );
                }
                // Acked-list (if any)
                if !acked_msgs.is_empty() {
                    list = list.push(
                        text(format!("✓ Acked  ({})", acked_msgs.len()))
                            .size(10)
                            .color(FG_FAINT),
                    );
                    for msg in acked_msgs.iter().take(10) {
                        list = list.push(
                            container(
                                text(format!(
                                    "✓  {}",
                                    if msg.title.is_empty() { &msg.topic } else { &msg.title }
                                ))
                                .size(11)
                                .color(FG_FAINT),
                            )
                            .padding(Padding { top: 3.0, right: 8.0, bottom: 3.0, left: 8.0 }),
                        );
                    }
                }
            }
        }

        if !self.muted_peers.is_empty() {
            let muted_list: Vec<&str> = self.muted_peers.iter().map(|s| s.as_str()).collect();
            list = list.push(
                container(
                    text(format!("Muted: {}", muted_list.join(", ")))
                        .size(10)
                        .color(FG_FAINT),
                )
                .padding(Padding {
                    top: 8.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                }),
            );
        }

        let scroll = scrollable(list).height(Length::Fill);

        // BUG-8.a — "Clear all" button (rendered only when
        // ≥1 notification exists). Click empties the cache
        // file + exits.
        let clear_btn: Element<'_, Message> = if total_rows > 0 {
            iced::widget::Button::new(text("Clear all").size(11).color(FG_TEXT))
                .padding(Padding {
                    top: 3.0,
                    right: 10.0,
                    bottom: 3.0,
                    left: 10.0,
                })
                .style(|_t: &Theme, status: iced::widget::button::Status| {
                    let bg = match status {
                        iced::widget::button::Status::Hovered => Color {
                            r: 0.18,
                            g: 0.18,
                            b: 0.20,
                            a: 1.0,
                        },
                        _ => Color::TRANSPARENT,
                    };
                    iced::widget::button::Style {
                        background: Some(Background::Color(bg)),
                        text_color: FG_TEXT,
                        border: Border {
                            color: Color {
                                a: 0.15,
                                ..Color::WHITE
                            },
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        shadow: Shadow::default(),
                    }
                })
                .on_press(Message::ClearAll)
                .into()
        } else {
            Space::with_width(Length::Fixed(0.0)).into()
        };

        // BUG-8.c — group-mode toggle. Label reflects the
        // mode the click will switch TO (so "By app" means
        // currently grouped by peer; clicking flips to app).
        let mode_label = match self.group_mode {
            GroupMode::Peer => "By app",
            GroupMode::App => "By peer",
        };
        let mode_btn: Element<'_, Message> =
            iced::widget::Button::new(text(mode_label).size(11).color(FG_TEXT))
                .padding(Padding {
                    top: 3.0,
                    right: 10.0,
                    bottom: 3.0,
                    left: 10.0,
                })
                .style(|_t: &Theme, status: iced::widget::button::Status| {
                    let bg = match status {
                        iced::widget::button::Status::Hovered => Color {
                            r: 0.18,
                            g: 0.18,
                            b: 0.20,
                            a: 1.0,
                        },
                        _ => Color::TRANSPARENT,
                    };
                    iced::widget::button::Style {
                        background: Some(Background::Color(bg)),
                        text_color: FG_TEXT,
                        border: Border {
                            color: Color {
                                a: 0.15,
                                ..Color::WHITE
                            },
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        shadow: Shadow::default(),
                    }
                })
                .on_press(Message::ToggleGroupMode)
                .into();

        let body = column![
            row![
                header,
                Space::with_width(Length::Fill),
                subhead,
                Space::with_width(Length::Fixed(8.0)),
                mode_btn,
                Space::with_width(Length::Fixed(6.0)),
                clear_btn,
                Space::with_width(Length::Fixed(8.0)),
                // v3.0.3 — always-visible close button (Esc still
                // works via subscription below).
                crate::dismiss::close_button(Message::Exit),
            ]
            .align_y(iced::Alignment::Center),
            Space::with_height(Length::Fixed(8.0)),
            scroll,
            Space::with_height(Length::Fixed(4.0)),
            text("Esc closes · Clear all empties the cache")
                .size(10)
                .color(FG_MUTED),
        ]
        .padding(Padding {
            top: 14.0,
            right: 14.0,
            bottom: 8.0,
            left: 14.0,
        });

        let card: Element<'_, Message> = container(body)
            .width(Length::Fixed(WIDTH as f32))
            .height(Length::Fixed(HEIGHT as f32))
            .style(popover_surface)
            .into();

        // v3.0.4 — backdrop dismiss; bottom-right card.
        let dismiss = || {
            mouse_area(
                container(Space::with_width(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::Exit)
        };
        let bottom_strip = row![
            dismiss(),
            container(card).padding(Padding {
                top: 0.0,
                right: 4.0,
                bottom: 48.0,
                left: 0.0,
            }),
        ]
        .height(Length::Fixed((HEIGHT + 48) as f32));
        container(column![dismiss(), bottom_strip])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                shadow: Shadow::default(),
                text_color: None,
            })
            .into()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        iced::keyboard::on_key_press(|key, _| {
            use iced::keyboard::{key::Named, Key};
            if matches!(key, Key::Named(Named::Escape)) {
                Some(Message::Exit)
            } else {
                None
            }
        })
    }
}

pub fn run() -> iced_layershell::Result {
    <App as iced_layershell::Application>::run(Settings {
        id: Some("mde-popover-notifications".to_string()),
        fonts: crate::fonts::load_fallback_fonts(),
        layer_settings: LayerShellSettings {
            // v3.0.4 — fullscreen for backdrop dismiss.
            size: None,
            exclusive_zone: -1,
            anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
            margin: (0, 0, 0, 0),
            layer: Layer::Overlay,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            ..Default::default()
        },
        ..Default::default()
    })
}

/// BUS-2.3 — render one Bus message row with an "Ack" button on the right.
fn render_bus_row(msg: &BusPopoverMessage) -> Element<'_, Message> {
    let priority_color = match msg.priority.as_str() {
        "urgent" => BUS_URGENT_COLOR,
        "high"   => BUS_HIGH_COLOR,
        _        => BUS_DEFAULT_COLOR,
    };
    let topic_label = text(format!("[{}]", msg.topic))
        .size(10)
        .color(priority_color);
    let title_label = text(if msg.title.is_empty() { msg.topic.as_str() } else { msg.title.as_str() })
        .size(12)
        .color(FG_TEXT);
    let body_label = if msg.body.is_empty() {
        text("").size(11).color(FG_MUTED)
    } else {
        text(msg.body.chars().take(100).collect::<String>())
            .size(11)
            .color(FG_MUTED)
    };
    let ack_ulid = msg.ulid.clone();
    let ack_btn: Element<'_, Message> = iced::widget::Button::new(
        text("Ack").size(10).color(FG_MUTED),
    )
    .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
    .style(|_t: &Theme, status: iced::widget::button::Status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => Color { r: 0.18, g: 0.18, b: 0.20, a: 1.0 },
            _ => Color::TRANSPARENT,
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            text_color: FG_MUTED,
            border: Border {
                color: Color { a: 0.12, ..Color::WHITE },
                width: 1.0,
                radius: 3.0.into(),
            },
            shadow: Shadow::default(),
        }
    })
    .on_press(Message::AckBusMessage(ack_ulid))
    .into();

    let text_col = column![topic_label, title_label, body_label].spacing(1);
    let content_row = row![
        text_col,
        Space::with_width(Length::Fill),
        ack_btn,
    ]
    .align_y(iced::Alignment::Center)
    .spacing(6);

    container(content_row)
        .padding(Padding { top: 5.0, right: 8.0, bottom: 5.0, left: 8.0 })
        .style(row_surface)
        .width(Length::Fill)
        .into()
}

fn render_row(row_data: &NotificationRow) -> Element<'_, Message> {
    let title_prefix = if is_phone_origin(row_data) {
        "📱 ".to_string()
    } else if !row_data.read {
        "• ".to_string()
    } else {
        "  ".to_string()
    };
    let title = text(format!("{title_prefix}{}", row_data.title))
        .size(13)
        .color(if row_data.read { FG_MUTED } else { FG_TEXT });
    let body = if row_data.body.is_empty() {
        text("").size(11).color(FG_MUTED)
    } else {
        text(row_data.body.chars().take(120).collect::<String>())
            .size(11)
            .color(FG_MUTED)
    };
    container(column![title, body].spacing(2))
        .padding(Padding {
            top: 6.0,
            right: 10.0,
            bottom: 6.0,
            left: 10.0,
        })
        .style(row_surface)
        .width(Length::Fill)
        .into()
}

/// BUG-8.c — load + group rows for the current `GroupMode`,
/// filtering muted peers in peer-mode (mute is a peer concept;
/// app-mode users want the full firehose so they can still see
/// chatty apps even from muted peers).
fn load_groups_for(
    mode: GroupMode,
    muted_peers: &std::collections::HashSet<String>,
) -> Vec<(String, Vec<NotificationRow>)> {
    let path: PathBuf = notifications_cache_path();
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = parse_notifications(&raw);
    let visible_rows = visible(rows);
    match mode {
        GroupMode::Peer => group_and_sort(visible_rows)
            .into_iter()
            .filter(|(peer, _)| !muted_peers.contains(peer))
            .collect(),
        GroupMode::App => group_by_app(visible_rows),
    }
}

/// BUG-8.b — resolve the mute file path. Returns the canonical
/// `~/.config/mde/notification-mutes.toml`; falls back to
/// `$XDG_CONFIG_HOME/mde/...` if HOME isn't set.
fn mutes_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("mde").join("notification-mutes.toml"))
}

/// BUG-8.b — pure parser for the mute file. Returns the set of
/// peer names whose `[muted]."<peer>" = true` row is present.
#[must_use]
pub fn parse_mutes(raw: &str) -> std::collections::HashSet<String> {
    let value: toml::Value = match toml::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Default::default(),
    };
    let mut out = std::collections::HashSet::new();
    if let Some(tbl) = value.get("muted").and_then(|v| v.as_table()) {
        for (peer, on) in tbl {
            if on.as_bool() == Some(true) {
                out.insert(peer.clone());
            }
        }
    }
    out
}

/// BUG-8.b — serialise the muted-peers set to TOML.
#[must_use]
pub fn serialize_mutes(muted: &std::collections::HashSet<String>) -> String {
    let mut peers: Vec<&String> = muted.iter().collect();
    peers.sort();
    let mut out = String::from("# mde-popover-notifications mute state — BUG-8.b\n");
    out.push_str("[muted]\n");
    for p in peers {
        let escaped = p.replace('\\', "\\\\").replace('"', "\\\"");
        out.push_str(&format!("\"{escaped}\" = true\n"));
    }
    out
}

fn load_muted_peers() -> std::collections::HashSet<String> {
    let Some(path) = mutes_path() else {
        return Default::default();
    };
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Default::default(),
    };
    parse_mutes(&raw)
}

fn save_muted_peers(
    muted: &std::collections::HashSet<String>,
) -> std::io::Result<()> {
    let Some(path) = mutes_path() else {
        return Err(std::io::Error::other("no $HOME"));
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, serialize_mutes(muted))
}

fn popover_surface(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE_BG)),
        border: Border {
            color: Color {
                r: 0.957,
                g: 0.957,
                b: 0.957,
                a: 0.10,
            },
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Some(FG_TEXT),
        shadow: Shadow::default(),
    }
}

fn row_surface(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            r: 0.106,
            g: 0.106,
            b: 0.114,
            a: 1.0,
        })),
        border: Border {
            color: Color {
                r: ACCENT.r,
                g: ACCENT.g,
                b: ACCENT.b,
                a: 0.05,
            },
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: Some(FG_TEXT),
        shadow: Shadow::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_pinned_for_visual_consistency() {
        assert_eq!(WIDTH, 480);
        assert_eq!(HEIGHT, 600);
    }


    #[test]
    fn parse_mutes_decodes_known_shape() {
        let raw = r#"
            [muted]
            "pine.mesh" = true
            "birch.mesh" = true
            "oak.mesh" = false
        "#;
        let muted = parse_mutes(raw);
        assert_eq!(muted.len(), 2);
        assert!(muted.contains("pine.mesh"));
        assert!(muted.contains("birch.mesh"));
        assert!(!muted.contains("oak.mesh"));
    }

    #[test]
    fn parse_mutes_returns_empty_for_garbage() {
        assert!(parse_mutes("not toml").is_empty());
    }

    #[test]
    fn serialize_mutes_round_trips_through_parse() {
        let mut m: std::collections::HashSet<String> = Default::default();
        m.insert("pine.mesh".into());
        m.insert("birch.mesh".into());
        let raw = serialize_mutes(&m);
        let back = parse_mutes(&raw);
        assert_eq!(back, m);
    }

    #[test]
    fn serialize_mutes_handles_peers_with_quotes_in_name() {
        let mut m: std::collections::HashSet<String> = Default::default();
        m.insert(r#"odd"name"#.to_string());
        let raw = serialize_mutes(&m);
        let back = parse_mutes(&raw);
        assert_eq!(back, m);
    }

    // ── BUS-2.3 tests ─────────────────────────────────────────────

    #[test]
    fn parse_bus_message_decodes_high_priority_file() {
        let dir = tempfile::tempdir().unwrap();
        let ulid = "01JABCDEFGHJKMNPQRST";
        let json = r#"{"ulid":"01JABCDEFGHJKMNPQRST","topic":"fleet/announce","priority":"high","title":"Test title","body":"Test body","ts_unix_ms":1700000000000,"file_path":"fleet/announce/01JABCDEFGHJKMNPQRST.json"}"#;
        let path = dir.path().join(format!("{ulid}.json"));
        std::fs::write(&path, json).unwrap();
        let msg = parse_bus_message(&path, ulid, "fleet/announce").unwrap();
        assert_eq!(msg.ulid, ulid);
        assert_eq!(msg.priority, "high");
        assert_eq!(msg.title, "Test title");
        assert_eq!(msg.body, "Test body");
        assert_eq!(msg.topic, "fleet/announce");
    }

    #[test]
    fn parse_bus_message_filters_min_priority() {
        let dir = tempfile::tempdir().unwrap();
        let ulid = "01JBBBBBBBBBBBBBBBBB";
        let json = r#"{"ulid":"01JBBBBBBBBBBBBBBBBB","topic":"debug/info","priority":"min","title":"x","body":"y","ts_unix_ms":0,"file_path":""}"#;
        let path = dir.path().join(format!("{ulid}.json"));
        std::fs::write(&path, json).unwrap();
        assert!(parse_bus_message(&path, ulid, "debug/info").is_none());
    }

    #[test]
    fn parse_bus_message_returns_none_for_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("01JCCC.json");
        std::fs::write(&path, "not json at all").unwrap();
        assert!(parse_bus_message(&path, "01JCCC", "fleet/announce").is_none());
    }

    fn make_msg(ulid: &str, priority: &str) -> BusPopoverMessage {
        BusPopoverMessage {
            ulid: ulid.to_string(),
            topic: "fleet/announce".to_string(),
            priority: priority.to_string(),
            title: format!("{priority} message"),
            body: "body".to_string(),
        }
    }

    #[test]
    fn bucket_by_priority_groups_three_buckets() {
        let msgs = vec![
            make_msg("01ZAAA", "urgent"),
            make_msg("01ZBBB", "high"),
            make_msg("01ZCCC", "default"),
        ];
        let acked = std::collections::HashSet::new();
        let (urgent, high, default) = bucket_by_priority(&msgs, &acked);
        assert_eq!(urgent.len(), 1);
        assert_eq!(high.len(), 1);
        assert_eq!(default.len(), 1);
        assert_eq!(urgent[0].ulid, "01ZAAA");
        assert_eq!(high[0].ulid, "01ZBBB");
        assert_eq!(default[0].ulid, "01ZCCC");
    }

    #[test]
    fn bucket_by_priority_excludes_acked_ulids() {
        let msgs = vec![
            make_msg("01ZAAA", "urgent"),
            make_msg("01ZBBB", "high"),
        ];
        let mut acked = std::collections::HashSet::new();
        acked.insert("01ZAAA".to_string());
        let (urgent, high, _) = bucket_by_priority(&msgs, &acked);
        assert!(urgent.is_empty(), "acked urgent must be excluded");
        assert_eq!(high.len(), 1);
    }

    #[test]
    fn bucket_by_priority_newest_first_within_bucket() {
        let msgs = vec![
            make_msg("01ZAAA", "high"),  // older ULID
            make_msg("01ZZZZ", "high"),  // newer ULID
        ];
        let acked = std::collections::HashSet::new();
        let (_, high, _) = bucket_by_priority(&msgs, &acked);
        assert_eq!(high[0].ulid, "01ZZZZ", "newer ULID must sort first");
    }

    #[test]
    fn load_bus_messages_reads_nested_topic_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let topic_dir = dir.path().join("fleet").join("announce");
        std::fs::create_dir_all(&topic_dir).unwrap();
        let ulid = "01JABCDEFGHJKMNPQRST";
        let json = format!(r#"{{"ulid":"{ulid}","topic":"fleet/announce","priority":"default","title":"Mesh event","body":"peer joined","ts_unix_ms":0,"file_path":""}}"#);
        std::fs::write(topic_dir.join(format!("{ulid}.json")), &json).unwrap();
        // audit/ files must be skipped
        let audit_dir = dir.path().join("audit");
        std::fs::create_dir_all(&audit_dir).unwrap();
        std::fs::write(audit_dir.join("2026-05-29.jsonl"), "skip me\n").unwrap();

        let msgs = load_bus_messages(dir.path());
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].ulid, ulid);
        assert_eq!(msgs[0].topic, "fleet/announce");
    }
}
