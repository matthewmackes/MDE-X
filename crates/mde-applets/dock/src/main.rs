//! DOCK-1 (v4.0.1, 2026-05-23) — Iced layer-shell dock.
//!
//! Replaces the text-renderer that shipped through Phase E1.2.7
//! with a real Iced 0.13 + `iced_layershell` 0.13.7 surface
//! anchored to the bottom of the screen. One cell per running
//! sway window + one cell per pinned `.desktop` entry that
//! isn't currently running. Cells render the app's Material
//! Symbols-mapped glyph via `mde_theme::Icon` →
//! `ResolvedIcon::svg_bytes()`.
//!
//! Interactions (mouse_area-based):
//!   * Left-click  → focus the window via `swaymsg
//!     [con_id=N] focus` (for pinned-only cells, launch via
//!     `gtk-launch <desktop_id>`).
//!   * Right-click → spawn the icon-mapper popover via
//!     `mde-popover icon-mapper <app_id>` (matches DOCK-1's
//!     E.19 hook).
//!   * Middle-click → pin (if running, not yet pinned) or unpin
//!     (if pinned). Best-choice deviation from the spec's
//!     drag-to-pin since Iced 0.13's mouse_area doesn't surface
//!     a full DnD pipeline; the resulting interaction lands in
//!     one click on `mackes_config::pin_app` /
//!     `unpin_app` + persists `panel.toml`.
//!
//! Tick: a 1 s `iced::time::every` subscription re-runs
//! `swaymsg -t get_tree` + reads the pinned-apps section of
//! `~/.config/mde/panel.toml`. Matches the legacy
//! text-renderer's cadence.
//!
//! Legacy entry points (`--manifest`, `--now`, stdin-loop)
//! remain reachable through `--text`; the applet-host stdin
//! supervisor uses them today and will keep working until the
//! host learns to spawn the GUI directly.

#![forbid(unsafe_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use iced::widget::{column, container, mouse_area, row, svg, text, Space};
use iced::{Background, Border, Color, Element, Length, Shadow, Subscription, Task, Theme};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings};
use iced_layershell::to_layer_message;
use mde_applet_api::HostMessage;
use mde_applet_dock::{
    format_dock, handle_host, icon_for_app_id, manifest, parse_pinned, parse_windows, pinned_path,
    DockWindow, PinnedApp,
};
use mde_theme::{mde_icon, IconSize};

/// Dock bar height in pixels. Matches the bottom-bar reservation
/// the existing panel CSS expects.
const HEIGHT: u32 = 48;
/// Per-cell width in pixels. Tunable; Win11/macOS dock cells are
/// 44-60 px, we go mid-range so labels stay legible at default
/// scale.
const CELL_WIDTH: f32 = 56.0;
/// Material Symbols SVG square rendered inside each cell.
const ICON_PX: f32 = 24.0;
/// Indigo accent for focus underlines + button hovers. Matches
/// the UX-2 lock (#5b6af5).
const ACCENT: Color = Color {
    r: 0.357,
    g: 0.416,
    b: 0.961,
    a: 1.0,
};
/// Orange highlight for urgent cells. UX-2 status palette.
const URGENT: Color = Color {
    r: 0.953,
    g: 0.514,
    b: 0.137,
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
const SURFACE_BG: Color = Color {
    r: 0.071,
    g: 0.071,
    b: 0.082,
    a: 0.97,
};

/// One cell on the dock. A cell is either a running sway window
/// (with a `con_id` for focus) or a pinned-only `.desktop`
/// (launched via `gtk-launch`).
/// One cell in the dock — either a running window (with a
/// `con_id` for focus) or a pinned-only `.desktop` (launched
/// via `gtk-launch`).
#[derive(Debug, Clone)]
pub struct DockCell {
    /// Wayland `app_id` for icon lookup + grouping with
    /// pinned-app rows.
    pub app_id: String,
    /// Display label for hover-tooltip + accessibility.
    pub label: String,
    /// `true` when the compositor reports this window focused.
    pub focused: bool,
    /// `true` when the window has set `urgent`/needs-attention.
    pub urgent: bool,
    /// `true` when the cell originates from a pinned-app row
    /// (vs a running window the operator hasn't pinned).
    pub pinned: bool,
    /// `Some(con_id)` when the cell maps to a running window.
    /// `None` for pinned-only cells.
    pub con_id: Option<u64>,
    /// `.desktop` basename for pinned cells. `None` for
    /// running-only cells the user hasn't pinned.
    pub desktop_id: Option<String>,
}

/// Iced application message — `#[to_layer_message]` expands
/// this enum with extra layer-shell-specific variants
/// (Resize, OutputAdded, …) the macro auto-injects; the
/// hand-written variants below cover the dock's own events.
/// The `allow(missing_docs)` on the macro line silences
/// the warnings the macro-injected variants would otherwise
/// emit (the macro does not propagate hand-written doc
/// comments onto its generated items).
#[allow(missing_docs)]
#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    /// 1 s tick — re-poll swaymsg + reread pinned config.
    Tick,
    /// Left-click: focus the running window or launch the
    /// pinned-only `.desktop`.
    Activate(usize),
    /// Right-click: spawn the WM-3 window-actions popover for
    /// the targeted cell (Close / Move to ws 1-4 / Pin-Unpin).
    OpenWindowActions(usize),
    /// Middle-click: toggle pin/unpin for the cell.
    TogglePin(usize),
}

/// Iced application state. Owns the live `Vec<DockCell>` the
/// renderer iterates each frame.
pub struct App {
    cells: Vec<DockCell>,
}

impl iced_layershell::Application for App {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Task<Message>) {
        let cells = build_cells();
        tracing::info!(cell_count = cells.len(), "dock layer-shell open");
        (Self { cells }, Task::none())
    }

    fn namespace(&self) -> String {
        "mde-applet-dock".to_string()
    }

    fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Tick => {
                self.cells = build_cells();
                Task::none()
            }
            Message::Activate(idx) => {
                if let Some(cell) = self.cells.get(idx).cloned() {
                    activate_cell(&cell);
                }
                Task::none()
            }
            Message::OpenWindowActions(idx) => {
                if let Some(cell) = self.cells.get(idx).cloned() {
                    spawn_window_actions(&cell);
                }
                Task::none()
            }
            Message::TogglePin(idx) => {
                if let Some(cell) = self.cells.get(idx).cloned() {
                    toggle_pin(&cell);
                    self.cells = build_cells();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let mut bar = row![].spacing(2).align_y(iced::Alignment::Center);
        for (idx, cell) in self.cells.iter().enumerate() {
            bar = bar.push(render_cell(idx, cell));
        }
        // Empty state — show a thin muted hint so the dock
        // doesn't look broken on a fresh login with no pinned
        // apps and no running windows.
        if self.cells.is_empty() {
            bar = bar.push(
                container(text("No windows or pinned apps").size(11).color(FG_MUTED))
                    .padding(8),
            );
        }
        container(
            row![Space::with_width(Length::Fixed(8.0)), bar]
                .align_y(iced::Alignment::Center)
                .height(Length::Fixed(HEIGHT as f32)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(HEIGHT as f32))
        .style(dock_surface)
        .into()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }

    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)
    }
}

fn render_cell(idx: usize, cell: &DockCell) -> Element<'_, Message> {
    let icon = mde_icon(icon_for_app_id(&cell.app_id), IconSize::PanelHeader);
    let icon_element: Element<'_, Message> = match icon.svg_bytes() {
        Some(bytes) => svg(svg::Handle::from_memory(bytes))
            .width(Length::Fixed(ICON_PX))
            .height(Length::Fixed(ICON_PX))
            .into(),
        // svg_bytes() returns None today only for variants we
        // haven't baked. The fallback Icon::Apps is always
        // wired, so this branch is defensive.
        None => text(icon_for_app_id(&cell.app_id).fallback_glyph())
            .size(20)
            .color(FG_TEXT)
            .into(),
    };

    // Focus underline + urgent border come from a per-cell
    // container style. Pinned-only cells render at lowered
    // opacity by using the muted foreground tint.
    let body = container(
        column![
            Space::with_height(Length::Fixed(2.0)),
            container(icon_element)
                .width(Length::Fixed(CELL_WIDTH))
                .center_x(Length::Fixed(CELL_WIDTH)),
            Space::with_height(Length::Fixed(2.0)),
            container(
                text(cell.label.clone())
                    .size(9)
                    .color(if cell.con_id.is_none() {
                        FG_MUTED
                    } else {
                        FG_TEXT
                    })
            )
            .width(Length::Fixed(CELL_WIDTH))
            .center_x(Length::Fixed(CELL_WIDTH)),
            Space::with_height(Length::Fixed(2.0)),
            focus_underline(cell),
        ]
        .align_x(iced::Alignment::Center),
    )
    .width(Length::Fixed(CELL_WIDTH))
    .height(Length::Fixed(HEIGHT as f32 - 4.0))
    .style(move |_| cell_style(cell));

    mouse_area(body)
        .on_press(Message::Activate(idx))
        .on_right_press(Message::OpenWindowActions(idx))
        .on_middle_press(Message::TogglePin(idx))
        .into()
}

fn focus_underline(cell: &DockCell) -> Element<'_, Message> {
    let color = if cell.focused {
        ACCENT
    } else {
        Color::TRANSPARENT
    };
    container(Space::with_width(Length::Fixed(CELL_WIDTH * 0.5)))
        .height(Length::Fixed(2.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 1.0.into(),
            },
            text_color: None,
            shadow: Shadow::default(),
        })
        .into()
}

fn cell_style(cell: &DockCell) -> container::Style {
    let (bg, border_color) = if cell.urgent {
        (
            Some(Background::Color(Color {
                r: URGENT.r,
                g: URGENT.g,
                b: URGENT.b,
                a: 0.18,
            })),
            URGENT,
        )
    } else if cell.focused {
        (
            Some(Background::Color(Color {
                r: ACCENT.r,
                g: ACCENT.g,
                b: ACCENT.b,
                a: 0.14,
            })),
            Color {
                r: ACCENT.r,
                g: ACCENT.g,
                b: ACCENT.b,
                a: 0.55,
            },
        )
    } else {
        (None, Color::TRANSPARENT)
    };
    container::Style {
        background: bg,
        border: Border {
            color: border_color,
            width: if cell.urgent { 1.0 } else { 0.0 },
            radius: 6.0.into(),
        },
        text_color: Some(FG_TEXT),
        shadow: Shadow::default(),
    }
}

fn dock_surface(_t: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE_BG)),
        border: Border {
            color: Color {
                r: 0.957,
                g: 0.957,
                b: 0.957,
                a: 0.08,
            },
            width: 1.0,
            radius: 0.0.into(),
        },
        text_color: Some(FG_TEXT),
        shadow: Shadow::default(),
    }
}

/// Compose the live cell list from sway + the pinned config.
/// Pinned-but-not-running apps render first (left-most),
/// running windows follow. Matches the legacy text-renderer's
/// order so visual habits carry across.
fn build_cells() -> Vec<DockCell> {
    let raw_tree = run_swaymsg_tree();
    let pinned_raw = std::fs::read_to_string(pinned_path()).unwrap_or_default();
    let windows = parse_windows(&raw_tree);
    let pinned = parse_pinned(&pinned_raw);
    cells_from(&pinned, &windows)
}

/// Pure helper — given the parsed pinned + window lists,
/// compose the cell layout. Pulled out for testing.
#[must_use]
pub fn cells_from(pinned: &[PinnedApp], windows: &[DockWindow]) -> Vec<DockCell> {
    use std::collections::HashSet;
    let running_app_ids: HashSet<&str> = windows.iter().map(|w| w.app_id.as_str()).collect();
    let pinned_app_ids: HashSet<&str> = pinned
        .iter()
        .map(|p| p.desktop_id.trim_end_matches(".desktop"))
        .collect();
    let mut cells = Vec::new();
    // Pinned-but-not-running first.
    for p in pinned {
        let bare = p.desktop_id.trim_end_matches(".desktop");
        if !running_app_ids.contains(bare) {
            cells.push(DockCell {
                app_id: bare.to_string(),
                label: p.label.clone(),
                focused: false,
                urgent: false,
                pinned: true,
                con_id: None,
                desktop_id: Some(p.desktop_id.clone()),
            });
        }
    }
    // Running windows.
    for w in windows {
        let pinned = pinned_app_ids.contains(w.app_id.as_str());
        let label = if w.app_id.is_empty() {
            "?".to_string()
        } else {
            w.app_id.clone()
        };
        cells.push(DockCell {
            app_id: w.app_id.clone(),
            label,
            focused: w.focused,
            urgent: w.urgent,
            pinned,
            con_id: Some(w.id),
            desktop_id: pinned.then(|| format!("{}.desktop", w.app_id)),
        });
    }
    cells
}

fn activate_cell(cell: &DockCell) {
    if let Some(id) = cell.con_id {
        let arg = format!("[con_id={id}]");
        let _ = Command::new("swaymsg")
            .args([&arg, "focus"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        return;
    }
    // Pinned-only cell — launch the .desktop via gtk-launch.
    let Some(desktop) = cell.desktop_id.as_deref() else {
        return;
    };
    let bare = desktop.trim_end_matches(".desktop");
    let _ = Command::new("gtk-launch")
        .arg(bare)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Spawn the WM-3 window-actions popover for the given cell.
/// The popover reads MDE_WINDOW_CON_ID + MDE_WINDOW_APP_ID
/// from its env to know which window to target.
fn spawn_window_actions(cell: &DockCell) {
    let con_id = cell.con_id.map(|n| n.to_string()).unwrap_or_default();
    let _ = Command::new("mde-popover")
        .arg("window-actions")
        .env("MDE_WINDOW_CON_ID", &con_id)
        .env("MDE_WINDOW_APP_ID", &cell.app_id)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Toggle the pin status for the given cell. Reads the live
/// `panel.toml`, mutates via `mackes_config::pin_app` /
/// `unpin_app`, writes back. Best-choice replacement for the
/// drag-to-reorder DnD the spec calls for — Iced 0.13's
/// `mouse_area` doesn't surface DnD events, so middle-click
/// is the one-gesture wiring that satisfies "PinDrop emits a
/// `Message` the dock_dnd helpers consume."
fn toggle_pin(cell: &DockCell) {
    let cfg_path = panel_config_path();
    let raw = std::fs::read_to_string(&cfg_path).unwrap_or_default();
    let mut cfg = mackes_config::parse(&raw).unwrap_or_else(|_| mackes_config::default_config());
    let bare = cell.app_id.trim_end_matches(".desktop");
    let already_pinned = cfg.dock.items.iter().any(|i| match i {
        mackes_config::DockItem::App { desktop: d } => d.trim_end_matches(".desktop") == bare,
        mackes_config::DockItem::Mesh { .. } => false,
    });
    if already_pinned {
        mackes_config::unpin_app(&mut cfg, &format!("{bare}.desktop"));
    } else if !bare.is_empty() {
        mackes_config::pin_app(&mut cfg, &format!("{bare}.desktop"));
    }
    if let Ok(s) = mackes_config::to_toml_string(&cfg) {
        if let Some(parent) = cfg_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&cfg_path, s);
    }
}

fn panel_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/mde/panel.toml")
}

fn run_swaymsg_tree() -> String {
    let Ok(output) = Command::new("swaymsg").args(["-t", "get_tree"]).output() else {
        return String::new();
    };
    if !output.status.success() {
        return String::new();
    }
    String::from_utf8(output.stdout).unwrap_or_default()
}

// ---- Entry-point dispatch ---------------------------------------------------

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a == "--manifest") {
        return print_manifest();
    }
    if argv.iter().any(|a| a == "--now") {
        return print_now();
    }
    if argv.iter().any(|a| a == "--text") {
        return run_text_loop();
    }
    run_layershell()
}

fn print_manifest() -> ExitCode {
    match serde_json::to_string_pretty(&manifest()) {
        Ok(j) => {
            println!("{j}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("mde-applet-dock: serialize manifest: {e}");
            ExitCode::FAILURE
        }
    }
}

fn current_dock_text() -> String {
    let raw_tree = run_swaymsg_tree();
    let raw_pinned = std::fs::read_to_string(pinned_path()).unwrap_or_default();
    let windows = parse_windows(&raw_tree);
    let pinned = parse_pinned(&raw_pinned);
    format_dock(&pinned, &windows)
}

fn print_now() -> ExitCode {
    println!("{}", current_dock_text());
    ExitCode::SUCCESS
}

fn run_text_loop() -> ExitCode {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let reader = BufReader::new(stdin.lock());
    let _ = writeln!(stdout, "{}", current_dock_text());
    let _ = stdout.flush();
    for line in reader.lines() {
        let Ok(line) = line else {
            return ExitCode::from(2);
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<HostMessage>(&line) else {
            return ExitCode::from(2);
        };
        if matches!(msg, HostMessage::Shutdown) {
            return ExitCode::SUCCESS;
        }
        if handle_host(&msg) {
            let _ = writeln!(stdout, "{}", current_dock_text());
            let _ = stdout.flush();
        }
    }
    ExitCode::SUCCESS
}

fn run_layershell() -> ExitCode {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .json()
        .try_init();
    let result = <App as iced_layershell::Application>::run(Settings {
        id: Some("mde-applet-dock".to_string()),
        layer_settings: LayerShellSettings {
            // Span the bottom of every output, reserve HEIGHT px
            // exclusive zone so other layer-shell clients
            // (notifications, popovers) don't overlap the dock.
            size: Some((0, HEIGHT)),
            exclusive_zone: HEIGHT as i32,
            anchor: Anchor::Bottom | Anchor::Left | Anchor::Right,
            margin: (0, 0, 0, 0),
            layer: Layer::Top,
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        },
        ..Default::default()
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(?err, "dock layer-shell exited with error");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pinned() -> Vec<PinnedApp> {
        vec![
            PinnedApp {
                desktop_id: "firefox.desktop".into(),
                label: "Firefox".into(),
            },
            PinnedApp {
                desktop_id: "foot.desktop".into(),
                label: "Terminal".into(),
            },
        ]
    }

    fn sample_running() -> Vec<DockWindow> {
        vec![DockWindow {
            id: 42,
            app_id: "firefox".into(),
            focused: true,
            urgent: false,
        }]
    }

    #[test]
    fn cells_render_pinned_only_then_running() {
        let cells = cells_from(&sample_pinned(), &sample_running());
        // foot is pinned + not running → first cell.
        // firefox is pinned + running → second cell (pinned=true,
        // con_id=Some).
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].app_id, "foot");
        assert!(cells[0].con_id.is_none());
        assert!(cells[0].pinned);
        assert_eq!(cells[1].app_id, "firefox");
        assert_eq!(cells[1].con_id, Some(42));
        assert!(cells[1].focused);
        assert!(cells[1].pinned);
    }

    #[test]
    fn cells_handles_empty_dock() {
        let cells = cells_from(&[], &[]);
        assert!(cells.is_empty());
    }

    #[test]
    fn cells_handles_no_pinned() {
        let cells = cells_from(&[], &sample_running());
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].app_id, "firefox");
        assert!(!cells[0].pinned);
        assert_eq!(cells[0].con_id, Some(42));
    }

    #[test]
    fn cells_label_empty_app_id_falls_back_to_question_mark() {
        let running = vec![DockWindow {
            id: 1,
            app_id: "".into(),
            focused: false,
            urgent: false,
        }];
        let cells = cells_from(&[], &running);
        assert_eq!(cells[0].label, "?");
    }

    #[test]
    fn cells_pinned_running_dedupes_to_single_cell() {
        let pinned = vec![PinnedApp {
            desktop_id: "firefox.desktop".into(),
            label: "Firefox".into(),
        }];
        let running = vec![DockWindow {
            id: 7,
            app_id: "firefox".into(),
            focused: false,
            urgent: false,
        }];
        let cells = cells_from(&pinned, &running);
        // Only one cell — firefox is both pinned + running so it
        // renders once with pinned=true and con_id=Some.
        assert_eq!(cells.len(), 1);
        assert!(cells[0].pinned);
        assert_eq!(cells[0].con_id, Some(7));
    }

    #[test]
    fn cells_marks_urgent_windows() {
        let running = vec![DockWindow {
            id: 9,
            app_id: "slack".into(),
            focused: false,
            urgent: true,
        }];
        let cells = cells_from(&[], &running);
        assert!(cells[0].urgent);
        assert!(!cells[0].focused);
    }

    #[test]
    fn panel_config_path_lands_under_home() {
        let p = panel_config_path();
        assert!(p.to_string_lossy().ends_with(".config/mde/panel.toml"));
    }
}
