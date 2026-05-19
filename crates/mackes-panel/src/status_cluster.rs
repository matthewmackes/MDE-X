//! Top-bar right-side status cluster — six read-only indicators that
//! each show an icon paired with a numeric value (Q-locked
//! 2026-05-18; click-target re-locked 2026-05-19).
//!
//! Layout: icon-left, number-right, ~4 px gap inside each pair, ~6 px
//! between items. The cluster refreshes every 2 s (matches the dock).
//! Clicking an item opens the Mackes Workbench focused on the panel
//! the slug maps to (`mackes --focus <slug>`); the Python side owns
//! the slug → panel-key translation. The drawer is no longer reachable
//! from this cluster — it stays bound to Super+M / the drawer applet.
//! On probe failure the value renders as an em-dash and the item dims
//! via `.mackes-status-degraded`; the tooltip names the reason.
//!
//! Numeric mapping per Q-lock:
//!
//! | Slot          | Source                                            | Unit       |
//! |---------------|---------------------------------------------------|------------|
//! | Mesh          | `tailscale status --json` (peers with Online:true)| count      |
//! | Clipboard     | `~/.cache/mackes/clipboard.json`                  | item count |
//! | Volume        | `pactl get-sink-volume @DEFAULT_SINK@`            | percent    |
//! | Battery       | `/sys/class/power_supply/BAT*/capacity`           | percent    |
//! | Notifications | `~/.cache/mackes/notifications.json` unread       | count      |
//! | User          | `who -q` `# users=N`                              | count      |

use std::path::PathBuf;
use std::process::Command;

use gtk::glib;
use gtk::prelude::*;

use crate::icons;

const STATUS_ICON_PX: i32 = 18;

/// Single status indicator: its widget cluster (left-to-right) is
/// `[icon][number]` wrapped in a relief-less `gtk::Button`.
#[derive(Clone, Copy, Debug)]
struct StatusItem {
    slug: &'static str,
    icon_name: &'static str,
    title: &'static str,
}

const ITEMS: &[StatusItem] = &[
    StatusItem {
        slug: "mesh",
        icon_name: "network-wireless-symbolic",
        title: "Mesh",
    },
    StatusItem {
        slug: "clipboard",
        icon_name: "edit-paste-symbolic",
        title: "Clipboard",
    },
    StatusItem {
        slug: "volume",
        icon_name: "audio-volume-high-symbolic",
        title: "Volume",
    },
    StatusItem {
        slug: "battery",
        icon_name: "battery-symbolic",
        title: "Battery",
    },
    StatusItem {
        slug: "notifications",
        icon_name: "mail-unread-symbolic",
        title: "Notifications",
    },
    StatusItem {
        slug: "user",
        icon_name: "system-users-symbolic",
        title: "User",
    },
];

/// Per-item live readout. `Ok(n)` renders as the integer (any width);
/// `Err(reason)` renders as an em-dash with `reason` suffixed into the
/// tooltip so the user can tell whether the underlying probe died (e.g.
/// "tailscale not running") versus a legitimate zero.
type Reading = Result<u32, String>;

#[derive(Clone)]
struct ItemWidgets {
    button: gtk::Button,
    label: gtk::Label,
    slug: &'static str,
    title: &'static str,
}

fn probe(slug: &str) -> Reading {
    match slug {
        "mesh" => probe_mesh(),
        "clipboard" => probe_clipboard(),
        "volume" => probe_volume(),
        "battery" => probe_battery(),
        "notifications" => probe_notifications(),
        "user" => probe_user(),
        other => Err(format!("unknown slug: {other}")),
    }
}

fn probe_mesh() -> Reading {
    let result = Command::new("tailscale")
        .args(["status", "--json"])
        .output();
    let bytes = match result {
        Ok(o) if o.status.success() => o.stdout,
        Ok(_) => return Err("tailscale not running".into()),
        Err(_) => return Err("tailscale not installed".into()),
    };
    // Cheap dep-free parse: the JSON has one `"Online": true|false` per
    // peer plus one for Self. Count the trues and subtract Self.
    let s = String::from_utf8_lossy(&bytes);
    let trues = s.matches("\"Online\":true").count() + s.matches("\"Online\": true").count();
    let online_peers = u32::try_from(trues.saturating_sub(1)).unwrap_or(u32::MAX);
    Ok(online_peers)
}

fn probe_clipboard() -> Reading {
    let path = cache_dir().join("mackes").join("clipboard.json");
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("clipboard.json unreadable: {e}")),
    };
    // Item count = number of top-level entries. Each carries a "text":
    // or "uri": field; counting those tokens is good enough without
    // pulling in serde for one indicator.
    let s = String::from_utf8_lossy(&bytes);
    let n = s.matches("\"text\"").count() + s.matches("\"uri\"").count();
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

fn probe_volume() -> Reading {
    let result = Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output();
    let bytes = match result {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Err("pactl unavailable".into()),
    };
    let s = String::from_utf8_lossy(&bytes);
    // pactl emits e.g.: `Volume: front-left: 49151 /  75% / -7.50 dB, ...`
    // Take the first integer immediately followed by `%`.
    let mut digits = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if ch == '%' && !digits.is_empty() {
            return digits
                .parse::<u32>()
                .map_err(|_| "unparseable %".to_owned());
        } else {
            digits.clear();
        }
    }
    Err("no % token".into())
}

fn probe_battery() -> Reading {
    let dir = std::path::Path::new("/sys/class/power_supply");
    let entries = std::fs::read_dir(dir).map_err(|_| "no /sys/class/power_supply".to_owned())?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("BAT") {
            let cap = entry.path().join("capacity");
            if let Ok(s) = std::fs::read_to_string(&cap) {
                if let Ok(n) = s.trim().parse::<u32>() {
                    return Ok(n);
                }
            }
        }
    }
    Err("no battery (desktop?)".into())
}

fn probe_notifications() -> Reading {
    let path = cache_dir().join("mackes").join("notifications.json");
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("notifications.json unreadable: {e}")),
    };
    let s = String::from_utf8_lossy(&bytes);
    let n = s.matches("\"read\":false").count() + s.matches("\"read\": false").count();
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

fn probe_user() -> Reading {
    let result = Command::new("who").arg("-q").output();
    let bytes = match result {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Err("who unavailable".into()),
    };
    let s = String::from_utf8_lossy(&bytes);
    for line in s.lines() {
        if let Some(rest) = line.trim().strip_prefix("# users=") {
            if let Ok(n) = rest.parse::<u32>() {
                return Ok(n);
            }
        }
    }
    Err("unparseable who -q".into())
}

fn cache_dir() -> PathBuf {
    if let Ok(s) = std::env::var("XDG_CACHE_HOME") {
        if !s.is_empty() {
            return PathBuf::from(s);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".cache");
    }
    PathBuf::from("/tmp")
}

fn apply_reading(w: &ItemWidgets, reading: &Reading) {
    let ctx = w.button.style_context();
    match reading {
        Ok(n) => {
            w.label.set_text(&n.to_string());
            let phrase = accessible_phrase(w.slug, *n);
            w.button.set_tooltip_text(Some(&phrase));
            // AT-SPI: screen readers announce this exact phrase. Without
            // it the button announces as "button" + the icon name, which
            // is useless. "Mesh: 3 online peers" reads sensibly.
            if let Some(atk) = w.button.accessible() {
                atk.set_name(&phrase);
            }
            ctx.remove_class("mackes-status-degraded");
        }
        Err(reason) => {
            w.label.set_text("—");
            let phrase = format!("{}: unavailable ({reason})", w.title);
            w.button.set_tooltip_text(Some(&phrase));
            if let Some(atk) = w.button.accessible() {
                atk.set_name(&phrase);
            }
            ctx.add_class("mackes-status-degraded");
        }
    }
}

/// Human-readable status phrase for AT-SPI announcements + tooltip.
/// Mesh/clipboard/notifications/user use plural-aware nouns; volume +
/// battery use percent.
fn accessible_phrase(slug: &str, n: u32) -> String {
    match slug {
        "mesh" => match n {
            0 => "Mesh: no peers online".into(),
            1 => "Mesh: 1 peer online".into(),
            _ => format!("Mesh: {n} peers online"),
        },
        "clipboard" => match n {
            0 => "Clipboard: empty".into(),
            1 => "Clipboard: 1 item".into(),
            _ => format!("Clipboard: {n} items"),
        },
        "volume" => format!("Volume: {n} percent"),
        "battery" => format!("Battery: {n} percent"),
        "notifications" => match n {
            0 => "Notifications: none unread".into(),
            1 => "Notifications: 1 unread".into(),
            _ => format!("Notifications: {n} unread"),
        },
        "user" => match n {
            0 => "User: no active sessions".into(),
            1 => "User: 1 session".into(),
            _ => format!("User: {n} sessions"),
        },
        _ => format!("Status: {n}"),
    }
}

fn refresh_all(widgets: &[ItemWidgets]) {
    for w in widgets {
        let reading = probe(w.slug);
        apply_reading(w, &reading);
    }
}

/// Build the cluster widget. Returns a `gtk::Box` ready to drop into the
/// top bar's right slot. The widget owns its own 2 s refresh timer.
#[must_use]
pub fn build() -> gtk::Box {
    let cluster = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    cluster.set_widget_name("mackes-status-cluster");

    let mut widgets: Vec<ItemWidgets> = Vec::with_capacity(ITEMS.len());

    for item in ITEMS {
        let button = gtk::Button::new();
        button.set_widget_name(&format!("mackes-status-{}", item.slug));
        button.set_relief(gtk::ReliefStyle::None);
        button.set_focus_on_click(false);

        let pair = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let image = gtk::Image::new();
        if let Some(pb) = icons::load(item.icon_name, STATUS_ICON_PX) {
            image.set_from_pixbuf(Some(&pb));
        }
        let label = gtk::Label::new(Some("—"));
        label.set_widget_name("mackes-status-value");
        pair.pack_start(&image, false, false, 0);
        pair.pack_start(&label, false, false, 0);
        button.add(&pair);

        let slug = item.slug;
        let button_for_handler = button.clone();
        button.connect_clicked(move |_| {
            // 1.1.0 (Q21 lock): the clipboard slug opens an in-process
            // Rust popover (live mesh history, click-to-paste) rather
            // than the Workbench. Every other slug continues with the
            // 1.0.8 behavior — `mackes --focus <slug>` opens the
            // Workbench focused on that panel.
            if slug == "clipboard" {
                let popover = crate::clipboard_manager::build(
                    button_for_handler.upcast_ref::<gtk::Widget>(),
                );
                popover.popup();
                return;
            }
            if let Err(e) = Command::new("mackes")
                .args(["--focus", slug])
                .spawn()
            {
                eprintln!("mackes-panel: workbench launch failed ({slug}): {e}");
            }
        });

        cluster.pack_start(&button, false, false, 0);
        widgets.push(ItemWidgets {
            button,
            label,
            slug: item.slug,
            title: item.title,
        });
    }

    refresh_all(&widgets);
    glib::timeout_add_seconds_local(2, move || {
        refresh_all(&widgets);
        glib::ControlFlow::Continue
    });

    cluster
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_dir_prefers_xdg() {
        let _g = crate::test_env::env_lock();
        std::env::set_var("XDG_CACHE_HOME", "/tmp/xdg-cache-test");
        assert_eq!(cache_dir(), PathBuf::from("/tmp/xdg-cache-test"));
        std::env::remove_var("XDG_CACHE_HOME");
    }

    #[test]
    fn probe_battery_when_no_sysfs_errs_cleanly() {
        // We can't mock /sys at unit-test scope, but reading a battery
        // on a CI VM with no BAT0 should land in the Err branch.
        let r = probe_battery();
        if let Ok(pct) = r {
            assert!(pct <= 100, "battery should be 0..=100, got {pct}");
        }
    }

    #[test]
    fn accessible_phrase_mesh_pluralizes_correctly() {
        assert_eq!(accessible_phrase("mesh", 0), "Mesh: no peers online");
        assert_eq!(accessible_phrase("mesh", 1), "Mesh: 1 peer online");
        assert_eq!(accessible_phrase("mesh", 5), "Mesh: 5 peers online");
    }

    #[test]
    fn accessible_phrase_clipboard_pluralizes_correctly() {
        assert_eq!(accessible_phrase("clipboard", 0), "Clipboard: empty");
        assert_eq!(accessible_phrase("clipboard", 1), "Clipboard: 1 item");
        assert_eq!(accessible_phrase("clipboard", 7), "Clipboard: 7 items");
    }

    #[test]
    fn accessible_phrase_notifications_pluralizes_correctly() {
        assert_eq!(
            accessible_phrase("notifications", 0),
            "Notifications: none unread"
        );
        assert_eq!(
            accessible_phrase("notifications", 1),
            "Notifications: 1 unread"
        );
        assert_eq!(
            accessible_phrase("notifications", 12),
            "Notifications: 12 unread"
        );
    }

    #[test]
    fn accessible_phrase_user_pluralizes_correctly() {
        assert_eq!(accessible_phrase("user", 0), "User: no active sessions");
        assert_eq!(accessible_phrase("user", 1), "User: 1 session");
        assert_eq!(accessible_phrase("user", 3), "User: 3 sessions");
    }

    #[test]
    fn accessible_phrase_volume_and_battery_render_percent() {
        assert_eq!(accessible_phrase("volume", 75), "Volume: 75 percent");
        assert_eq!(accessible_phrase("battery", 30), "Battery: 30 percent");
        assert_eq!(accessible_phrase("battery", 0), "Battery: 0 percent");
        assert_eq!(accessible_phrase("battery", 100), "Battery: 100 percent");
    }

    #[test]
    fn accessible_phrase_unknown_slug_falls_back_to_status() {
        assert_eq!(accessible_phrase("invented", 4), "Status: 4");
    }

    #[test]
    fn cache_dir_falls_back_to_home_then_tmp() {
        let _g = crate::test_env::env_lock();
        std::env::remove_var("XDG_CACHE_HOME");
        std::env::set_var("HOME", "/var/empty/home");
        let dir = cache_dir();
        assert_eq!(dir, PathBuf::from("/var/empty/home").join(".cache"));
        std::env::remove_var("HOME");
        let dir = cache_dir();
        assert_eq!(dir, PathBuf::from("/tmp"));
    }

    #[test]
    fn cluster_build_has_one_button_per_known_item() {
        let _g = crate::test_env::env_lock();
        if !crate::test_env::try_init_gtk_serialized() {
            return;
        }
        let cluster = build();
        let children = cluster.children();
        assert_eq!(
            children.len(),
            ITEMS.len(),
            "build() must emit one widget per ITEMS entry"
        );
        for child in &children {
            assert!(
                child.is::<gtk::Button>(),
                "every cluster child must be a Button"
            );
        }
    }

    #[test]
    fn cluster_build_buttons_have_widget_names_matching_slugs() {
        let _g = crate::test_env::env_lock();
        if !crate::test_env::try_init_gtk_serialized() {
            return;
        }
        let cluster = build();
        let children = cluster.children();
        for (i, child) in children.iter().enumerate() {
            let name = child.widget_name();
            let expected_suffix = ITEMS[i].slug;
            assert!(
                name.contains(expected_suffix),
                "widget {i} name {name:?} must contain slug {expected_suffix:?}"
            );
        }
    }
}
