//! BUS-2.8 — Do Not Disturb state machine + per-topic quiet hours.
//!
//! v6.x BUS-2.8 design lock: a single DND toggle gates ALL surfaces
//! (toast, tray, status-zone strip, theater takeover, wallpaper
//! stripe). When DND is active, only messages tagged with
//! `override=dnd` bypass — those messages still surface so genuine
//! emergencies (security incidents, critical alerts) can reach the
//! operator while everyday notifications stay quiet.
//!
//! Per-topic quiet hours layer on top of the DND toggle: each
//! topic config can carry a `quiet_after` / `quiet_until` window
//! of local-time seconds-of-day. Within that window, the topic
//! behaves as if DND was on (message goes to persistent file
//! storage + audit but is NOT routed to display surfaces).
//!
//! ## Files
//!
//! DND state syncs across the mesh via
//! `<XDG_DATA_HOME>/mde/bus/dnd.yaml` on the GFS-replicated
//! `mesh-home`. The schema is intentionally tiny so a flick of
//! the toggle on peer-A propagates to peer-B within the GFS
//! 1-second heal window.
//!
//! ## What ships here (BUS-2.8.data)
//!
//! This module is the v1 — DATA MODEL + DECISION LOGIC.
//! Serialization round-trip + the `is_suppressed` pure helper are
//! both unit-testable in isolation. The GFS sync + inotify watch
//! ship as a separate BUS-2.8.watcher follow-on once the data
//! schema is locked.

use serde::{Deserialize, Serialize};

/// Mesh-wide DND state. Single bool per the design lock —
/// per-topic mute is handled by the `subs.yaml` manifest (per
/// BUS-1.7), not by the DND toggle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DndState {
    /// `true` when DND is active; `false` when off.
    #[serde(default)]
    pub active: bool,
    /// Wall-clock instant the state was last toggled, in
    /// milliseconds since the Unix epoch. Used by the audit
    /// log to capture "DND on since 14:00 local."
    #[serde(default)]
    pub since_unix_ms: i64,
    /// Hostname of the peer that flipped the toggle. Used to
    /// surface "DND on by @<peer>" in the UI; mesh-wide sync
    /// means the source can differ from the local peer.
    #[serde(default)]
    pub set_by_peer: String,
}

impl Default for DndState {
    fn default() -> Self {
        Self {
            active: false,
            since_unix_ms: 0,
            set_by_peer: String::new(),
        }
    }
}

/// Per-topic quiet-hour window. Both fields are seconds-since-
/// midnight in the operator's local timezone (0..86_399).
/// `quiet_after` = window opens at this time; `quiet_until` =
/// window closes. When `quiet_after < quiet_until` the window is
/// same-day (09:00..17:00 = work-quiet); when `quiet_after >
/// quiet_until` the window wraps midnight (22:00..07:00 =
/// overnight-quiet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TopicQuietHours {
    /// Window-open boundary in seconds-of-day (0..86_400). When
    /// both fields are `None`, no quiet window is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_after: Option<u32>,
    /// Window-close boundary in seconds-of-day (0..86_400).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_until: Option<u32>,
}

/// Pure-fn — true if the given seconds-of-day falls inside the
/// quiet-hour window. Returns `false` when either bound is
/// `None` (no window configured) or when both bounds are equal
/// (zero-length window). Handles both same-day and overnight
/// (wrap-midnight) windows.
#[must_use]
pub fn is_quiet_hour(now_local_seconds: u32, hours: TopicQuietHours) -> bool {
    let (Some(after), Some(until)) = (hours.quiet_after, hours.quiet_until) else {
        return false;
    };
    if after == until {
        // Zero-length window — never quiet.
        return false;
    }
    if after < until {
        // Same-day window (09:00..17:00 = work-quiet).
        now_local_seconds >= after && now_local_seconds < until
    } else {
        // Overnight window (22:00..07:00). Quiet iff now is
        // after `quiet_after` OR before `quiet_until`.
        now_local_seconds >= after || now_local_seconds < until
    }
}

/// Pure-fn — true if the message should be SUPPRESSED (not
/// routed to display surfaces). The message still gets persisted
/// + audited regardless; suppression is a routing decision, not
/// a storage decision.
///
/// Rules (in priority order):
///   1. `override=dnd` tag → never suppressed (genuine
///      emergency bypass).
///   2. Global DND toggle active → suppressed.
///   3. Topic quiet-hour window active → suppressed.
///   4. Otherwise → not suppressed.
#[must_use]
pub fn is_suppressed(
    state: &DndState,
    topic_hours: TopicQuietHours,
    tags: &[&str],
    now_local_seconds: u32,
) -> bool {
    if tags.contains(&"override=dnd") {
        return false;
    }
    if state.active {
        return true;
    }
    is_quiet_hour(now_local_seconds, topic_hours)
}

/// Load the mesh-wide DND state from the GFS-replicated YAML
/// file at `<bus_root>/dnd.yaml`. Returns `DndState::default()`
/// (DND off) when the file is missing or unparseable — DND off
/// is the safe default so a corrupted file doesn't silently
/// suppress every notification.
#[must_use]
pub fn load_default(bus_root: &std::path::Path) -> DndState {
    let path = bus_root.join("dnd.yaml");
    let Ok(bytes) = std::fs::read(&path) else {
        return DndState::default();
    };
    serde_yaml::from_slice(&bytes).unwrap_or_default()
}

/// Atomic-write the DND state to `<bus_root>/dnd.yaml` via
/// temp-file + rename. Caller passes the full state (typically
/// from the operator's DND-toggle Workbench surface or a
/// `mde-bus dnd on/off` CLI verb that ships separately).
/// Returns `Ok(())` on success; `Err(io::Error)` on filesystem
/// failure.
pub fn save_default(bus_root: &std::path::Path, state: &DndState) -> std::io::Result<()> {
    std::fs::create_dir_all(bus_root)?;
    let serialized = serde_yaml::to_string(state)
        .map_err(|e| std::io::Error::other(format!("serialize dnd.yaml: {e}")))?;
    let final_path = bus_root.join("dnd.yaml");
    let tmp_path = bus_root.join("dnd.yaml.tmp");
    std::fs::write(&tmp_path, serialized)?;
    std::fs::rename(&tmp_path, &final_path)?;
    Ok(())
}

/// Convenience: parse an `HH:MM` (24-hour) string into a
/// seconds-of-day value. Returns `None` on malformed input
/// (missing colon, non-numeric, out-of-range hour or minute).
/// Used by the `dnd.yaml` migration path that accepts both raw
/// seconds + human-readable HH:MM strings.
#[must_use]
pub fn parse_hhmm(s: &str) -> Option<u32> {
    let (h_str, m_str) = s.split_once(':')?;
    let h: u32 = h_str.parse().ok()?;
    let m: u32 = m_str.parse().ok()?;
    if h >= 24 || m >= 60 {
        return None;
    }
    Some(h * 3600 + m * 60)
}

/// Tick interval for the DND-state watcher. 1 second balances
/// "operator-tolerant lag from peer-A toggle → peer-B observe"
/// (GFS heals dnd.yaml within ~1 s on the LAN) against polling
/// overhead.
pub const DEFAULT_WATCH_TICK: std::time::Duration =
    std::time::Duration::from_secs(1);

/// Outcome of one [`DndWatcher::tick_once`] cycle. Public so
/// tests can assert which branch fired without inspecting logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DndTickOutcome {
    /// File doesn't exist (pre-toggle state or operator deleted
    /// it). The cached state stays at the last known value
    /// (default DND off on first miss).
    FileMissing,
    /// File mtime hasn't advanced since the last poll. No re-read,
    /// no broadcast.
    Idle,
    /// File mtime advanced + content differs from cache. State
    /// re-published through the watch channel.
    Reloaded,
    /// File mtime advanced but content parsed identical (e.g.
    /// `touch dnd.yaml`). Treated as no-op.
    Unchanged,
    /// Re-read failed (permission, IO, etc.) or parse failed. The
    /// previous cached state is preserved — corrupted writes
    /// don't blow away the operator's last good DND value.
    ReadOrParseFailed,
}

/// Live watcher for `<bus_root>/dnd.yaml`. Polls mtime every
/// [`DEFAULT_WATCH_TICK`] (1 s); on advance re-reads + re-parses
/// and broadcasts the new state through a
/// `tokio::sync::watch::Sender`. Subscribers (the hook handler,
/// future BUS-2.x display surfaces) clone the Receiver via
/// [`Self::subscribe`].
///
/// The broadcast pattern eliminates the per-publish file re-read
/// in `handle_hook` — instead of `dnd::load_default(&bus_root)`
/// once per webhook fire, the handler reads the cached `current()`
/// in O(lock-free-borrow). Each operator toggle still propagates
/// across the mesh in ≤ 2 s (GFS heal + watch tick).
pub struct DndWatcher {
    file_path: std::path::PathBuf,
    tick_interval: std::time::Duration,
    tx: std::sync::Arc<tokio::sync::watch::Sender<DndState>>,
    rx: tokio::sync::watch::Receiver<DndState>,
    last_mtime: Option<std::time::SystemTime>,
}

impl DndWatcher {
    /// Construct a watcher pinned to `<bus_root>/dnd.yaml`. The
    /// initial state is loaded eagerly (missing/corrupted file →
    /// `DndState::default()` = DND off).
    #[must_use]
    pub fn new(bus_root: std::path::PathBuf) -> Self {
        let file_path = bus_root.join("dnd.yaml");
        let initial = load_default(&bus_root);
        let (tx, rx) = tokio::sync::watch::channel(initial);
        Self {
            file_path,
            tick_interval: DEFAULT_WATCH_TICK,
            tx: std::sync::Arc::new(tx),
            rx,
            last_mtime: None,
        }
    }

    /// Override the tick interval — used by tests that need a
    /// faster pulse.
    #[must_use]
    pub fn with_tick_interval(mut self, interval: std::time::Duration) -> Self {
        self.tick_interval = interval;
        self
    }

    /// Subscribe to state updates. Returns a fresh Receiver
    /// cloned off the watcher's Sender; the latest value is
    /// always immediately readable via `borrow()`.
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<DndState> {
        self.rx.clone()
    }

    /// Snapshot the current state. Cheaper than `subscribe()`
    /// when the caller only needs one read.
    #[must_use]
    pub fn current(&self) -> DndState {
        self.rx.borrow().clone()
    }

    /// Drive one tick of the watch loop. Public so tests can run
    /// it deterministically.
    pub fn tick_once(&mut self) -> DndTickOutcome {
        if !self.file_path.exists() {
            return DndTickOutcome::FileMissing;
        }
        let now = match std::fs::metadata(&self.file_path)
            .and_then(|m| m.modified())
        {
            Ok(t) => t,
            Err(_) => return DndTickOutcome::Idle,
        };
        let advanced = self.last_mtime.is_none_or(|last| now > last);
        self.last_mtime = Some(now);
        if !advanced {
            return DndTickOutcome::Idle;
        }
        let bytes = match std::fs::read(&self.file_path) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(
                    target: "mde_bus::dnd",
                    error = %e,
                    path = %self.file_path.display(),
                    "dnd.yaml re-read failed"
                );
                return DndTickOutcome::ReadOrParseFailed;
            }
        };
        let parsed: DndState = match serde_yaml::from_slice(&bytes) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    target: "mde_bus::dnd",
                    error = %e,
                    "dnd.yaml parse failed — keeping previous state"
                );
                return DndTickOutcome::ReadOrParseFailed;
            }
        };
        let changed = *self.tx.borrow() != parsed;
        if changed {
            let _ = self.tx.send_replace(parsed);
            tracing::info!(
                target: "mde_bus::dnd",
                path = %self.file_path.display(),
                "dnd state reloaded"
            );
            DndTickOutcome::Reloaded
        } else {
            DndTickOutcome::Unchanged
        }
    }

    /// Long-running async loop. Calls [`Self::tick_once`] every
    /// `tick_interval` until `shutdown.changed()` resolves.
    pub async fn run(
        &mut self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        loop {
            let _ = self.tick_once();
            tokio::select! {
                _ = shutdown.changed() => break,
                () = tokio::time::sleep(self.tick_interval) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_dnd_off() {
        let s = DndState::default();
        assert!(!s.active);
        assert_eq!(s.since_unix_ms, 0);
        assert!(s.set_by_peer.is_empty());
    }

    #[test]
    fn dnd_state_roundtrips_yaml() {
        let s = DndState {
            active: true,
            since_unix_ms: 1_700_000_000_000,
            set_by_peer: "fedora".to_string(),
        };
        let yaml = serde_yaml::to_string(&s).unwrap();
        let back: DndState = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn topic_quiet_hours_default_no_window() {
        let h = TopicQuietHours::default();
        assert!(h.quiet_after.is_none());
        assert!(h.quiet_until.is_none());
        assert!(!is_quiet_hour(12 * 3600, h));
    }

    #[test]
    fn quiet_hour_same_day_window() {
        // 09:00..17:00 work-quiet.
        let h = TopicQuietHours {
            quiet_after: Some(9 * 3600),
            quiet_until: Some(17 * 3600),
        };
        assert!(!is_quiet_hour(8 * 3600, h));    // 08:00 — before window
        assert!(is_quiet_hour(9 * 3600, h));     // 09:00 — boundary in
        assert!(is_quiet_hour(12 * 3600, h));    // 12:00 — middle
        assert!(!is_quiet_hour(17 * 3600, h));   // 17:00 — boundary out
        assert!(!is_quiet_hour(20 * 3600, h));   // 20:00 — after window
    }

    #[test]
    fn quiet_hour_overnight_window() {
        // 22:00..07:00 overnight-quiet.
        let h = TopicQuietHours {
            quiet_after: Some(22 * 3600),
            quiet_until: Some(7 * 3600),
        };
        assert!(is_quiet_hour(23 * 3600, h));    // 23:00 — after `after`
        assert!(is_quiet_hour(0, h));            // 00:00 — wrap midnight
        assert!(is_quiet_hour(6 * 3600, h));     // 06:00 — before `until`
        assert!(!is_quiet_hour(7 * 3600, h));    // 07:00 — boundary out
        assert!(!is_quiet_hour(12 * 3600, h));   // 12:00 — daytime
        assert!(!is_quiet_hour(21 * 3600 + 59 * 60, h)); // 21:59 — just before `after`
    }

    #[test]
    fn quiet_hour_zero_length_window_never_fires() {
        let h = TopicQuietHours {
            quiet_after: Some(12 * 3600),
            quiet_until: Some(12 * 3600),
        };
        for hour in 0..24 {
            assert!(!is_quiet_hour(hour * 3600, h));
        }
    }

    #[test]
    fn quiet_hour_one_sided_window_never_fires() {
        // Either bound None → no window.
        let only_after = TopicQuietHours {
            quiet_after: Some(9 * 3600),
            quiet_until: None,
        };
        assert!(!is_quiet_hour(12 * 3600, only_after));
        let only_until = TopicQuietHours {
            quiet_after: None,
            quiet_until: Some(17 * 3600),
        };
        assert!(!is_quiet_hour(12 * 3600, only_until));
    }

    #[test]
    fn override_dnd_tag_bypasses_global_toggle() {
        let state = DndState {
            active: true,
            since_unix_ms: 1_000,
            set_by_peer: "fedora".to_string(),
        };
        let hours = TopicQuietHours::default();
        let tags_with_override = ["priority=urgent", "override=dnd"];
        let tags_without = ["priority=urgent"];
        assert!(!is_suppressed(&state, hours, &tags_with_override, 12 * 3600));
        assert!(is_suppressed(&state, hours, &tags_without, 12 * 3600));
    }

    #[test]
    fn override_dnd_tag_bypasses_quiet_hours() {
        let state = DndState::default();
        let hours = TopicQuietHours {
            quiet_after: Some(9 * 3600),
            quiet_until: Some(17 * 3600),
        };
        let tags_with_override = ["override=dnd"];
        let tags_without: [&str; 0] = [];
        // Inside quiet hour, override bypasses; without override
        // the quiet window suppresses.
        assert!(!is_suppressed(&state, hours, &tags_with_override, 12 * 3600));
        assert!(is_suppressed(&state, hours, &tags_without, 12 * 3600));
    }

    #[test]
    fn dnd_off_outside_quiet_hours_is_not_suppressed() {
        let state = DndState::default();
        let hours = TopicQuietHours {
            quiet_after: Some(9 * 3600),
            quiet_until: Some(17 * 3600),
        };
        // 20:00 — DND off AND outside the quiet window → delivered.
        assert!(!is_suppressed(&state, hours, &[], 20 * 3600));
    }

    #[test]
    fn parse_hhmm_round_trip() {
        assert_eq!(parse_hhmm("09:00"), Some(9 * 3600));
        assert_eq!(parse_hhmm("17:00"), Some(17 * 3600));
        assert_eq!(parse_hhmm("00:00"), Some(0));
        assert_eq!(parse_hhmm("23:59"), Some(23 * 3600 + 59 * 60));
        assert_eq!(parse_hhmm("12:30"), Some(12 * 3600 + 30 * 60));
    }

    #[test]
    fn load_default_missing_file_returns_default() {
        let tmp = std::env::temp_dir().join(format!("mde-bus-dnd-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        // No dnd.yaml in tmp — should return default (DND off).
        let s = load_default(&tmp);
        assert_eq!(s, DndState::default());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn save_default_round_trip() {
        let tmp = std::env::temp_dir().join(format!("mde-bus-dnd-roundtrip-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let original = DndState {
            active: true,
            since_unix_ms: 1_700_000_000_000,
            set_by_peer: "fedora".to_string(),
        };
        save_default(&tmp, &original).unwrap();
        let loaded = load_default(&tmp);
        assert_eq!(original, loaded);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_default_corrupted_yaml_returns_default() {
        let tmp = std::env::temp_dir().join(format!("mde-bus-dnd-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("dnd.yaml"), "this is not yaml: {[}{").unwrap();
        let s = load_default(&tmp);
        // DND off is the safe default — a corrupted file must NOT
        // silently suppress every notification.
        assert!(!s.active);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn watcher_starts_with_default_when_file_missing() {
        let tmp = std::env::temp_dir().join(format!("mde-bus-dnd-watch-init-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let watcher = DndWatcher::new(tmp.clone());
        // File doesn't exist → initial state = default (DND off).
        assert_eq!(watcher.current(), DndState::default());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn watcher_starts_with_existing_file_state() {
        let tmp = std::env::temp_dir().join(format!("mde-bus-dnd-watch-existing-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let existing = DndState {
            active: true,
            since_unix_ms: 1_700_000_000_000,
            set_by_peer: "fedora".to_string(),
        };
        save_default(&tmp, &existing).unwrap();
        let watcher = DndWatcher::new(tmp.clone());
        assert_eq!(watcher.current(), existing);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn watcher_tick_file_missing() {
        let tmp = std::env::temp_dir().join(format!("mde-bus-dnd-watch-missing-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let mut watcher = DndWatcher::new(tmp.clone());
        assert_eq!(watcher.tick_once(), DndTickOutcome::FileMissing);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn watcher_tick_reloads_on_mtime_advance() {
        let tmp = std::env::temp_dir().join(format!("mde-bus-dnd-watch-reload-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let initial = DndState::default();
        save_default(&tmp, &initial).unwrap();
        let mut watcher = DndWatcher::new(tmp.clone());
        // First tick after creation — file exists, mtime advances
        // from None → file's mtime. Content is identical to
        // initial → Unchanged.
        let first = watcher.tick_once();
        assert!(
            matches!(first, DndTickOutcome::Unchanged | DndTickOutcome::Reloaded),
            "first tick should be Unchanged or Reloaded, got {first:?}"
        );
        // Sleep briefly so the next save_default produces a
        // strictly-later mtime; some filesystems have 1-second
        // mtime granularity.
        std::thread::sleep(std::time::Duration::from_millis(1200));
        let flipped = DndState {
            active: true,
            since_unix_ms: 1_700_000_000_000,
            set_by_peer: "fedora".to_string(),
        };
        save_default(&tmp, &flipped).unwrap();
        assert_eq!(watcher.tick_once(), DndTickOutcome::Reloaded);
        assert_eq!(watcher.current(), flipped);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn watcher_subscribe_emits_clone_of_current_state() {
        let tmp = std::env::temp_dir().join(format!("mde-bus-dnd-watch-sub-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let watcher = DndWatcher::new(tmp.clone());
        let rx = watcher.subscribe();
        assert_eq!(*rx.borrow(), DndState::default());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn parse_hhmm_rejects_malformed() {
        assert!(parse_hhmm("").is_none());
        assert!(parse_hhmm("9").is_none());
        assert!(parse_hhmm("09").is_none());
        assert!(parse_hhmm("09:").is_none());
        assert!(parse_hhmm(":00").is_none());
        assert!(parse_hhmm("24:00").is_none()); // hour out of range
        assert!(parse_hhmm("09:60").is_none()); // minute out of range
        assert!(parse_hhmm("ab:cd").is_none()); // non-numeric
    }
}
