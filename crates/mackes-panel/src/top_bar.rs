//! Top-bar widget construction.
//!
//! Phase 1.5–1.7: fills the left / center / right slots with the
//! initial visual widgets:
//!
//! - Left:   Mackes button (`apple_menu_button`)
//! - Center: HH:MM clock with a 60 s timer (`clock`)
//! - Right:  Six-glyph status cluster (`status_cluster`)
//!
//! Each widget is a stub with the right shape; behavior (menu dropdown,
//! drawer open, etc.) lands in later phases per `docs/PROJECT_WORKLIST.md`.

use std::process::Command;

use gdk_pixbuf::Pixbuf;
use gtk::prelude::*;

use crate::{apple_menu, desktop_files, icons};

/// Glyph size shown in the 20 px top bar. 14 px lets the icon breathe
/// against the height without clipping baseline math.
const TOP_BAR_ICON_PX: i32 = 14;

/// Glyph used as the Mackes-menu button. Q23 hinted at a Carbon mark;
/// we use `applications-system-symbolic` as a stand-in until the real
/// brand glyph lands.
const MACKES_BUTTON_ICON: &str = "applications-system-symbolic";

/// Right-side status cluster, in render order (left-to-right). Per Q8.
const STATUS_ITEMS: &[(&str, &str)] = &[
    ("mesh", "network-wireless-symbolic"),
    ("clipboard", "edit-paste-symbolic"),
    ("volume", "audio-volume-high-symbolic"),
    ("battery", "battery-symbolic"),
    ("notifications", "mail-unread-symbolic"),
    ("user", "system-users-symbolic"),
];

/// Build the Mackes-menu button. Click → drops a `gtk::Menu` populated
/// by `apple_menu::build` of the live `.desktop` scan, plus the canonical
/// system-action items (Sleep / Restart / Shut Down / Lock / Sign Out).
#[must_use]
pub fn apple_menu_button() -> gtk::Button {
    let button = gtk::Button::new();
    button.set_widget_name("mackes-apple-menu-button");
    button.set_relief(gtk::ReliefStyle::None);
    button.set_focus_on_click(false);

    if let Some(pb) = icons::load(MACKES_BUTTON_ICON, TOP_BAR_ICON_PX) {
        button.set_image(Some(&gtk::Image::from_pixbuf(Some(&pb))));
        button.set_always_show_image(true);
    } else {
        // No Carbon theme available (dev tree); use a tiny text glyph
        // so the slot is at least visible.
        button.set_label("M");
    }

    let button_for_handler = button.clone();
    button.connect_clicked(move |_| {
        let menu = build_apple_menu();
        menu.show_all();
        menu.popup_at_widget(
            &button_for_handler,
            gdk::Gravity::SouthWest,
            gdk::Gravity::NorthWest,
            None,
        );
    });
    button
}

/// Construct the full Apple-menu `gtk::Menu`. Composition matches the
/// design lock's Q24 ordering:
///   About / ─ / Settings / Software Update / ─ / Recent → / Applications →
///   / ─ / Force Quit / ─ / Sleep / Restart / Shut Down / ─ / Lock / Sign Out
fn build_apple_menu() -> gtk::Menu {
    let menu = gtk::Menu::new();
    menu.set_widget_name("mackes-apple-menu");

    add_item(&menu, "About Mackes", || {
        launch("mackes", &["--about"]);
    });
    menu.append(&gtk::SeparatorMenuItem::new());
    add_item(&menu, "Settings…", || {
        launch("mackes", &[]);
    });
    add_item(&menu, "Software Update…", || {
        launch("mackes", &["--update"]);
    });
    menu.append(&gtk::SeparatorMenuItem::new());
    menu.append(&build_applications_submenu());
    menu.append(&gtk::SeparatorMenuItem::new());
    add_item(&menu, "Force Quit…", || {
        eprintln!("mackes-panel: force-quit (stub)");
    });
    menu.append(&gtk::SeparatorMenuItem::new());
    add_item(&menu, "Sleep", || run_loginctl("suspend"));
    add_item(&menu, "Restart…", || run_loginctl("reboot"));
    add_item(&menu, "Shut Down…", || run_loginctl("poweroff"));
    menu.append(&gtk::SeparatorMenuItem::new());
    add_item(&menu, "Lock Screen", || run_loginctl("lock-session"));
    add_item(&menu, "Sign Out…", || {
        launch("xfce4-session-logout", &["--logout"]);
    });

    menu
}

fn build_applications_submenu() -> gtk::MenuItem {
    let parent = gtk::MenuItem::with_label("Applications");
    let submenu = gtk::Menu::new();
    submenu.set_widget_name("mackes-apple-menu-applications");

    for cat in apple_menu::build(&desktop_files::scan()) {
        let cat_item = gtk::MenuItem::with_label(cat.label);
        let cat_submenu = gtk::Menu::new();
        for entry in cat.entries {
            let item = gtk::MenuItem::with_label(&entry.name);
            let exec = entry.exec.clone();
            let terminal = entry.terminal;
            item.connect_activate(move |_| {
                launch_exec(&exec, terminal);
            });
            cat_submenu.append(&item);
        }
        cat_item.set_submenu(Some(&cat_submenu));
        submenu.append(&cat_item);
    }

    parent.set_submenu(Some(&submenu));
    parent
}

fn add_item<F>(menu: &gtk::Menu, label: &str, on_activate: F)
where
    F: Fn() + 'static,
{
    let item = gtk::MenuItem::with_label(label);
    item.connect_activate(move |_| on_activate());
    menu.append(&item);
}

/// `loginctl <verb>` (no `PolicyKit` prompt for `lock-session` and
/// `suspend`; `reboot`/`poweroff` will prompt via `PolicyKit`).
fn run_loginctl(verb: &str) {
    if let Err(e) = Command::new("loginctl").arg(verb).spawn() {
        eprintln!("mackes-panel: loginctl {verb} failed: {e}");
    }
}

fn launch(program: &str, args: &[&str]) {
    if let Err(e) = Command::new(program).args(args).spawn() {
        eprintln!("mackes-panel: launching {program} failed: {e}");
    }
}

/// Spawn an `Exec=` line from a `.desktop` file. We strip the
/// freedesktop field codes (`%U`, `%F`, etc.) and hand the rest to
/// `/bin/sh -c` so quoting works. Terminal apps get prefixed with the
/// user's preferred terminal.
///
/// Shared with `dock::AppModule` (Phase 5.1), so re-exported as `pub`.
pub fn launch_exec(exec: &str, terminal: bool) {
    let stripped = strip_field_codes(exec);
    let cmd = if terminal {
        format!("xfce4-terminal -e {stripped}")
    } else {
        stripped
    };
    if let Err(e) = Command::new("/bin/sh").arg("-c").arg(&cmd).spawn() {
        eprintln!("mackes-panel: spawn failed: {cmd}: {e}");
    }
}

fn strip_field_codes(exec: &str) -> String {
    exec.split_whitespace()
        .filter(|tok| !is_field_code(tok))
        .collect::<Vec<&str>>()
        .join(" ")
}

fn is_field_code(token: &str) -> bool {
    matches!(
        token,
        "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%i" | "%c" | "%k" | "%v" | "%m"
    )
}

/// Build the center clock widget. The label updates every 60 s and on
/// startup. Format is "HH:MM" — 24-hour, monospace via Red Hat Mono
/// (loaded by the global token CSS).
///
/// `gtk::Label` is a reference-counted `GObject` handle, so cloning it
/// for the timer closure is just a refcount bump (no `Rc<RefCell<…>>`
/// needed).
#[must_use]
pub fn clock() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_widget_name("mackes-top-clock");
    label.set_text(&current_hhmm());

    // First tick scheduled for the next minute boundary; afterwards
    // every 60 s. This keeps the clock visually synchronised with the
    // wall clock instead of drifting based on startup time.
    let initial_delay_s = seconds_until_next_minute();
    let label_for_timer = label.clone();
    glib::timeout_add_seconds_local(initial_delay_s, move || {
        label_for_timer.set_text(&current_hhmm());
        let label_recurring = label_for_timer.clone();
        glib::timeout_add_seconds_local(60, move || {
            label_recurring.set_text(&current_hhmm());
            glib::ControlFlow::Continue
        });
        glib::ControlFlow::Break
    });

    label
}

fn current_hhmm() -> String {
    let now = glib::DateTime::now_local().expect("system clock");
    now.format("%H:%M")
        .map_or_else(|_| "--:--".to_owned(), |g| g.as_str().to_owned())
}

/// Seconds remaining until the next clock minute. `glib::timeout_add_seconds`
/// takes whole seconds, so we floor — losing at most a few hundred
/// milliseconds of accuracy on the first tick.
fn seconds_until_next_minute() -> u32 {
    let now = glib::DateTime::now_local().expect("system clock");
    let secs = now.second();
    if secs >= 60 {
        1
    } else {
        let remaining = 60 - secs;
        u32::try_from(remaining).unwrap_or(1)
    }
}

/// Build the right-side status cluster — six Carbon glyphs side by side.
/// Click anywhere in the cluster opens the Notification Drawer (Q28),
/// stubbed for now.
#[must_use]
pub fn status_cluster() -> gtk::Box {
    let cluster = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    cluster.set_widget_name("mackes-status-cluster");

    for (slug, icon_name) in STATUS_ITEMS {
        cluster.pack_start(&status_item(slug, icon_name), false, false, 0);
    }

    cluster
}

fn status_item(slug: &str, icon_name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_widget_name(&format!("mackes-status-{slug}"));
    button.set_relief(gtk::ReliefStyle::None);
    button.set_focus_on_click(false);

    let pb: Option<Pixbuf> = icons::load(icon_name, TOP_BAR_ICON_PX);
    if let Some(pb) = pb {
        button.set_image(Some(&gtk::Image::from_pixbuf(Some(&pb))));
        button.set_always_show_image(true);
    } else {
        // Dev fallback so the slot remains discoverable.
        button.set_label(&slug.chars().next().unwrap_or('?').to_string());
    }

    let slug_owned = slug.to_owned();
    button.connect_clicked(move |_| {
        // Per Q28 every status-cluster click opens the v2.2.0
        // Notification Drawer. Until Phase 4.3 ports the drawer into
        // mackes-panel itself, we invoke the existing Python drawer
        // via `mackes --drawer`. The drawer focuses its own section
        // based on the slug (`--drawer-focus <slug>`).
        if let Err(e) = Command::new("mackes")
            .args(["--drawer", "--drawer-focus", &slug_owned])
            .spawn()
        {
            eprintln!("mackes-panel: drawer launch failed ({slug_owned}): {e}");
        }
    });

    button
}
