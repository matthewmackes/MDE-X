//! Notification-bell pill — top-bar-right applet that
//! shows the unread-count badge.
//!
//! Phase E1.2.5: reads `~/.cache/mackes/notifications.json`
//! (the same file the v1.x notification_bell.rs +
//! notification_center.rs sync via QNM-Shared) and counts
//! unread rows. Renders the bell glyph + count badge string.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use mde_applet_api::{AppletId, AppletSlot, HostMessage};
use serde::Deserialize;

/// One notification row in the JSON cache file. The v1.x
/// schema (mackes/notifications.py) has more fields; we
/// only read the ones the bell needs.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NotificationRow {
    /// Read-state flag. Default `false` (unread) on
    /// missing-field for the fresh-notification case.
    #[serde(default)]
    pub read: bool,
    /// Optional dismissed flag — dismissed notifications
    /// count as read for the bell badge.
    #[serde(default)]
    pub dismissed: bool,
}

/// Build the canonical manifest the panel host picks up.
#[must_use]
pub fn manifest() -> mde_applet_api::AppletManifest {
    mde_applet_api::AppletManifest {
        id: AppletId::from_static("notification-bell"),
        binary: "mde-applet-notification-bell".into(),
        slot: AppletSlot::TopBarRight,
        summary: "Notification unread-count pill".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// `~/.cache/mackes/notifications.json` — the QNM-Shared-
/// replicated notification log.
#[must_use]
pub fn notifications_cache_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".cache/mackes/notifications.json")
}

/// Parse the notification-cache JSON into a Vec of rows.
/// Returns an empty Vec on any failure (file missing,
/// malformed JSON, etc.).
#[must_use]
pub fn parse_notifications(raw: &str) -> Vec<NotificationRow> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Count unread, non-dismissed rows.
#[must_use]
pub fn count_unread(rows: &[NotificationRow]) -> u32 {
    let n = rows.iter().filter(|r| !r.read && !r.dismissed).count();
    // Cap at u32::MAX defensively — the panel renders "99+"
    // for any count over 99 anyway.
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Format the unread-count badge string the bell renders.
/// `0` → empty (the badge hides); `1..=99` → the integer;
/// `100+` → `"99+"` matching the v1.x cap.
#[must_use]
pub fn format_badge(count: u32) -> String {
    if count == 0 {
        String::new()
    } else if count <= 99 {
        count.to_string()
    } else {
        "99+".to_string()
    }
}

/// Decide whether a host-pushed message means the bell
/// should re-render. Shutdown short-circuits to false.
#[must_use]
pub fn handle_host(msg: &HostMessage) -> bool {
    !matches!(msg, HostMessage::Shutdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_lands_in_top_bar_right_slot() {
        let m = manifest();
        assert_eq!(m.id.as_str(), "notification-bell");
        assert_eq!(m.slot, AppletSlot::TopBarRight);
    }

    #[test]
    fn parse_notifications_returns_empty_on_garbage() {
        assert!(parse_notifications("").is_empty());
        assert!(parse_notifications("not json").is_empty());
        assert!(parse_notifications("{}").is_empty()); // not a list
    }

    #[test]
    fn parse_notifications_extracts_read_and_dismissed_flags() {
        let raw = r#"[
            {"read": false, "dismissed": false},
            {"read": true,  "dismissed": false},
            {"read": false, "dismissed": true}
        ]"#;
        let rows = parse_notifications(raw);
        assert_eq!(rows.len(), 3);
        assert!(!rows[0].read && !rows[0].dismissed);
        assert!(rows[1].read);
        assert!(rows[2].dismissed);
    }

    #[test]
    fn count_unread_excludes_read_and_dismissed() {
        let rows = vec![
            NotificationRow {
                read: false,
                dismissed: false,
            },
            NotificationRow {
                read: false,
                dismissed: true,
            },
            NotificationRow {
                read: true,
                dismissed: false,
            },
            NotificationRow {
                read: false,
                dismissed: false,
            },
        ];
        assert_eq!(count_unread(&rows), 2);
    }

    #[test]
    fn format_badge_zero_is_empty() {
        assert_eq!(format_badge(0), "");
    }

    #[test]
    fn format_badge_within_99_is_integer() {
        assert_eq!(format_badge(1), "1");
        assert_eq!(format_badge(42), "42");
        assert_eq!(format_badge(99), "99");
    }

    #[test]
    fn format_badge_over_99_caps_at_99_plus() {
        assert_eq!(format_badge(100), "99+");
        assert_eq!(format_badge(1_000_000), "99+");
    }

    #[test]
    fn handle_host_short_circuits_shutdown() {
        assert!(!handle_host(&HostMessage::Shutdown));
        assert!(handle_host(&HostMessage::Visibility { active: true }));
    }
}
