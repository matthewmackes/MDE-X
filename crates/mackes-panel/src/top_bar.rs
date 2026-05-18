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

use gdk_pixbuf::Pixbuf;
use gtk::prelude::*;

use crate::icons;

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

/// Build the Mackes-menu button. Click handler is currently a stub —
/// Phase 3 wires the Apple-menu dropdown.
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

    button.connect_clicked(|_| {
        // Stub: Phase 3 replaces this with the dropdown trigger.
        eprintln!("mackes-panel: apple menu clicked (stub)");
    });
    button
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
        // Phase 4.2: replace with `Drawer::open()` signal.
        eprintln!("mackes-panel: status item {slug_owned} clicked (stub)");
    });

    button
}
