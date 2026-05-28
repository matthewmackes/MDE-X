//! HYP-14 (v6.5, Portal-46 retarget) — per-window mark state.
//!
//! Hyprland has no native window-mark primitive (sway did). This
//! worker owns the mark store in mackesd + exposes it over the
//! Mackes Bus so Portal (HYP-15 pills), the border tinter (HYP-22)
//! and the elevation shadow worker (HYP-21) can subscribe to mark
//! deltas without a compositor-side C++ store — per the 2026-05-27
//! simplification re-lock (design doc §10.1).
//!
//! ## Two event sources, one store
//!
//! 1. **Hyprland `EventStream`** (`openwindow` / `closewindow` /
//!    `activewindow`): tracks window lifecycle. On `openwindow` the
//!    worker auto-populates marks from the window's class via the
//!    compile-time taxonomy ([`super::auto_mark::taxonomy_for_app_id`])
//!    + the owning tag manifest's `marks_default`. On `closewindow`
//!    the window's marks are dropped.
//! 2. **Bus action poll** (`action/marks/{add,remove,list,match}`):
//!    mackesd's first Bus action-responder. The worker polls the
//!    persist layer for new action messages (the same persisted
//!    request/reply path `mde_bus::rpc::await_reply` reads from —
//!    there is no live-subscription requirement) + writes the result
//!    to `reply/<request-ulid>`.
//!
//! Every add/remove also publishes a delta on `event/marks/<addr>`
//! so subscribers repaint. State persists to a GFS-replicated
//! snapshot (`~/.local/share/mde/marks/<peer>.toml`) on a 60 s tick
//! + on shutdown, and is replayed (matched by class + title) on
//! restart so a mackesd bounce doesn't lose marks for live windows.

#![cfg(feature = "async-services")]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Duration;

use futures_util::StreamExt as _;
use hyprland::event_listener::{Event, EventStream};
use mde_bus::hooks::Priority;
use mde_bus::persist::Persist;
use mde_bus::rpc::reply_topic;
use serde::{Deserialize, Serialize};

use super::auto_mark::taxonomy_for_app_id;
use super::{ShutdownToken, Worker};

const RECONNECT_BACKOFF: Duration = Duration::from_secs(3);
/// How often the Bus action topics are polled for new requests.
const ACTION_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Snapshot cadence (acceptance: 60 s tick + on shutdown).
const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(60);
/// The four `action/marks/<verb>` topics the responder serves.
const ACTION_VERBS: [&str; 4] = ["add", "remove", "list", "match"];

// ── State ───────────────────────────────────────────────────────────────

/// Per-window mark store. Keyed by Hyprland window address string
/// (`0x…`). `class` + `title` are retained alongside the marks so a
/// restart can re-match a live window to its snapshotted marks.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MarksStore {
    /// addr → window identity (for snapshot replay matching).
    identity: HashMap<String, WindowIdentity>,
    /// addr → marks (sorted, deduped).
    marks: HashMap<String, Vec<String>>,
}

/// The class + title a window reported, used to re-match snapshotted
/// marks to a live window after a mackesd restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowIdentity {
    /// Window class (`app_id` analog).
    pub class: String,
    /// Window title at the time the window was last seen.
    pub title: String,
}

impl MarksStore {
    /// Add `mark` to `addr` (idempotent). Returns `true` when the
    /// mark set actually changed (so the caller knows to publish a
    /// delta).
    pub fn add_mark(&mut self, addr: &str, mark: &str) -> bool {
        if mark.is_empty() {
            return false;
        }
        let set = self.marks.entry(addr.to_string()).or_default();
        if set.iter().any(|m| m == mark) {
            return false;
        }
        set.push(mark.to_string());
        set.sort();
        true
    }

    /// Remove `mark` from `addr`. Returns `true` when something was
    /// removed.
    pub fn remove_mark(&mut self, addr: &str, mark: &str) -> bool {
        let Some(set) = self.marks.get_mut(addr) else {
            return false;
        };
        let before = set.len();
        set.retain(|m| m != mark);
        let changed = set.len() != before;
        if set.is_empty() {
            self.marks.remove(addr);
        }
        changed
    }

    /// List the marks on `addr` (empty when none / unknown).
    #[must_use]
    pub fn list_marks(&self, addr: &str) -> Vec<String> {
        self.marks.get(addr).cloned().unwrap_or_default()
    }

    /// Every address carrying `mark`, sorted for determinism.
    #[must_use]
    pub fn match_marks(&self, mark: &str) -> Vec<String> {
        let mut hits: Vec<String> = self
            .marks
            .iter()
            .filter(|(_, ms)| ms.iter().any(|m| m == mark))
            .map(|(addr, _)| addr.clone())
            .collect();
        hits.sort();
        hits
    }

    /// Record a window's identity (class + title) for snapshot
    /// replay matching, and drop a closed window entirely.
    pub fn note_window(&mut self, addr: &str, class: &str, title: &str) {
        self.identity.insert(
            addr.to_string(),
            WindowIdentity {
                class: class.to_string(),
                title: title.to_string(),
            },
        );
    }

    /// Forget a closed window — drops its identity + marks.
    pub fn drop_window(&mut self, addr: &str) {
        self.identity.remove(addr);
        self.marks.remove(addr);
    }
}

// ── Bus action request / reply shapes ───────────────────────────────────

/// JSON body of an `action/marks/{add,remove}` request.
#[derive(Debug, Deserialize)]
struct MarkMutateRequest {
    addr: String,
    mark: String,
}

/// JSON body of an `action/marks/list` request.
#[derive(Debug, Deserialize)]
struct MarkListRequest {
    addr: String,
}

/// JSON body of an `action/marks/match` request.
#[derive(Debug, Deserialize)]
struct MarkMatchRequest {
    mark: String,
}

/// JSON reply for every verb — a status + an optional address /
/// mark list, so a single shape serves all four.
#[derive(Debug, Serialize)]
struct MarkReply {
    ok: bool,
    /// `add` / `remove`: whether the store changed. Other verbs: true.
    changed: bool,
    /// `list`: the marks on the queried addr. Empty otherwise.
    marks: Vec<String>,
    /// `match`: the matching addrs. Empty otherwise.
    addrs: Vec<String>,
}

/// `event/marks/<addr>` delta body.
#[derive(Debug, Serialize)]
struct MarkDelta<'a> {
    addr: &'a str,
    op: &'a str,
    mark: &'a str,
    marks: Vec<String>,
}

// ── Snapshot ────────────────────────────────────────────────────────────

/// GFS-replicated snapshot of the whole mark store.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarksSnapshot {
    /// One entry per window with marks.
    #[serde(default)]
    pub windows: BTreeMap<String, SnapshotEntry>,
}

/// One window's snapshotted identity + marks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotEntry {
    /// Window class (replay match key).
    pub class: String,
    /// Window title (replay match key).
    pub title: String,
    /// Marks to restore.
    pub marks: Vec<String>,
}

impl MarksStore {
    /// Serialize the live store into a snapshot (only windows that
    /// actually carry marks are persisted).
    #[must_use]
    pub fn to_snapshot(&self) -> MarksSnapshot {
        let mut snap = MarksSnapshot::default();
        for (addr, marks) in &self.marks {
            if marks.is_empty() {
                continue;
            }
            let id = self.identity.get(addr).cloned().unwrap_or_default();
            snap.windows.insert(
                addr.clone(),
                SnapshotEntry {
                    class: id.class,
                    title: id.title,
                    marks: marks.clone(),
                },
            );
        }
        snap
    }

    /// Rebuild a store from a snapshot. Used on restart before the
    /// first live event so subscribers see continuity. Addresses
    /// from the snapshot are provisional until the matching live
    /// window re-announces (HYP-15 re-keys by class+title).
    #[must_use]
    pub fn from_snapshot(snap: &MarksSnapshot) -> Self {
        let mut store = Self::default();
        for (addr, entry) in &snap.windows {
            store.identity.insert(
                addr.clone(),
                WindowIdentity {
                    class: entry.class.clone(),
                    title: entry.title.clone(),
                },
            );
            if !entry.marks.is_empty() {
                let mut marks = entry.marks.clone();
                marks.sort();
                marks.dedup();
                store.marks.insert(addr.clone(), marks);
            }
        }
        store
    }
}

impl Default for WindowIdentity {
    fn default() -> Self {
        Self {
            class: String::new(),
            title: String::new(),
        }
    }
}

/// Resolve `~/.local/share/mde/marks/<peer>.toml`. Peer name is the
/// hostname (the snapshot is per-peer so GFS replication doesn't
/// collide). Returns `None` when neither the data dir nor a hostname
/// resolves.
#[must_use]
pub fn default_snapshot_path() -> Option<PathBuf> {
    let data = dirs::data_dir()?;
    let host = hostname_string()?;
    Some(data.join("mde").join("marks").join(format!("{host}.toml")))
}

fn hostname_string() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok().filter(|s| !s.is_empty()))
}

/// Read the persist Bus root the same way the `mde-bus` CLI does:
/// `<XDG_DATA_HOME>/mde/bus`.
fn default_bus_root() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("mde").join("bus"))
}

// ── Pure verb dispatch (testable without persist / Hyprland) ────────────

/// Outcome of dispatching one Bus action against the store: the JSON
/// reply body + (for mutations) the delta to publish.
#[derive(Debug, PartialEq, Eq)]
pub struct DispatchOutcome {
    /// JSON reply to write to `reply/<ulid>`.
    pub reply_json: String,
    /// `Some((addr, op, mark, marks))` when a delta should publish.
    pub delta: Option<(String, String, String, Vec<String>)>,
}

/// Dispatch one action verb against `store`, mutating it for
/// add/remove. `verb` is the topic tail (`add` / `remove` / `list` /
/// `match`); `body` is the request JSON. Unknown verbs / malformed
/// bodies produce an `ok:false` reply + no delta.
pub fn dispatch_action(store: &mut MarksStore, verb: &str, body: &str) -> DispatchOutcome {
    let fail = || DispatchOutcome {
        reply_json: serde_json::to_string(&MarkReply {
            ok: false,
            changed: false,
            marks: Vec::new(),
            addrs: Vec::new(),
        })
        .unwrap_or_else(|_| "{\"ok\":false}".to_string()),
        delta: None,
    };

    match verb {
        "add" | "remove" => {
            let Ok(req) = serde_json::from_str::<MarkMutateRequest>(body) else {
                return fail();
            };
            let changed = if verb == "add" {
                store.add_mark(&req.addr, &req.mark)
            } else {
                store.remove_mark(&req.addr, &req.mark)
            };
            let reply = MarkReply {
                ok: true,
                changed,
                marks: store.list_marks(&req.addr),
                addrs: Vec::new(),
            };
            let delta = if changed {
                Some((
                    req.addr.clone(),
                    verb.to_string(),
                    req.mark.clone(),
                    store.list_marks(&req.addr),
                ))
            } else {
                None
            };
            DispatchOutcome {
                reply_json: serde_json::to_string(&reply)
                    .unwrap_or_else(|_| "{\"ok\":true}".to_string()),
                delta,
            }
        }
        "list" => {
            let Ok(req) = serde_json::from_str::<MarkListRequest>(body) else {
                return fail();
            };
            let reply = MarkReply {
                ok: true,
                changed: false,
                marks: store.list_marks(&req.addr),
                addrs: Vec::new(),
            };
            DispatchOutcome {
                reply_json: serde_json::to_string(&reply)
                    .unwrap_or_else(|_| "{\"ok\":true}".to_string()),
                delta: None,
            }
        }
        "match" => {
            let Ok(req) = serde_json::from_str::<MarkMatchRequest>(body) else {
                return fail();
            };
            let reply = MarkReply {
                ok: true,
                changed: false,
                marks: Vec::new(),
                addrs: store.match_marks(&req.mark),
            };
            DispatchOutcome {
                reply_json: serde_json::to_string(&reply)
                    .unwrap_or_else(|_| "{\"ok\":true}".to_string()),
                delta: None,
            }
        }
        _ => fail(),
    }
}

/// Seed marks for a freshly-opened window from its class: the
/// compile-time taxonomy bucket (editor/web/shell/mail/chat) plus
/// every `marks_default` entry of any tag manifest whose `apps[]`
/// lists the class. Pure — the caller applies the result.
#[must_use]
pub fn auto_marks_for_class(
    class: &str,
    manifests: Option<&[crate::config::TagManifest]>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(tax) = taxonomy_for_app_id(class) {
        out.push(tax.to_string());
    }
    if let Some(ms) = manifests {
        for m in ms.iter().filter(|m| m.apps.iter().any(|a| a == class)) {
            for mark in m.marks_default.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if !out.iter().any(|o| o == mark) {
                    out.push(mark.to_string());
                }
            }
        }
    }
    out
}

// ── Worker ──────────────────────────────────────────────────────────────

/// Empty-state worker; the store + cursors live on the stack inside
/// `run` so a reconnect rebuilds cleanly.
pub struct MarksStateWorker;

impl MarksStateWorker {
    /// Construct a fresh worker.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for MarksStateWorker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Worker for MarksStateWorker {
    fn name(&self) -> &'static str {
        "marks_state"
    }

    async fn run(&mut self, mut shutdown: ShutdownToken) -> anyhow::Result<()> {
        // Open the persist layer once; reused across reconnect cycles.
        // Without a bus root we can't serve the action surface, so the
        // worker idles (returns Ok) rather than spinning.
        let Some(bus_root) = default_bus_root() else {
            tracing::debug!("marks_state: no bus root resolvable; worker idle");
            return Ok(());
        };
        let persist = match Persist::open(bus_root) {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!(error = %e, "marks_state: persist open failed; worker idle");
                return Ok(());
            }
        };

        // Restore the snapshot before the first live event so
        // subscribers see mark continuity across a mackesd bounce.
        let mut store = load_snapshot().unwrap_or_default();
        // Per-action-topic cursor (last ULID processed).
        let mut cursors: HashMap<String, String> = HashMap::new();

        loop {
            if shutdown.is_shutdown() {
                let _ = save_snapshot(&store);
                return Ok(());
            }
            let mut events = EventStream::new();
            let mut snap_tick = tokio::time::interval(SNAPSHOT_INTERVAL);
            snap_tick.tick().await; // consume the immediate first tick
            let mut poll_tick = tokio::time::interval(ACTION_POLL_INTERVAL);

            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.wait() => {
                        let _ = save_snapshot(&store);
                        return Ok(());
                    }
                    next = events.next() => {
                        match next {
                            Some(Ok(ev)) => handle_event(&persist, &mut store, &ev),
                            Some(Err(e)) => {
                                tracing::debug!(error = %e, "marks_state event stream errored; reconnecting");
                                break;
                            }
                            None => {
                                tracing::debug!("marks_state event stream ended; reconnecting");
                                break;
                            }
                        }
                    }
                    _ = poll_tick.tick() => {
                        poll_actions(&persist, &mut store, &mut cursors);
                    }
                    _ = snap_tick.tick() => {
                        let _ = save_snapshot(&store);
                    }
                }
            }
            sleep_or_shutdown(RECONNECT_BACKOFF, &mut shutdown).await;
        }
    }
}

async fn sleep_or_shutdown(dur: Duration, shutdown: &mut ShutdownToken) {
    tokio::select! {
        _ = shutdown.wait() => {}
        _ = tokio::time::sleep(dur) => {}
    }
}

/// React to one Hyprland event: track window lifecycle + auto-mark
/// new windows from their class taxonomy + tag-manifest marks_default.
fn handle_event(persist: &Persist, store: &mut MarksStore, ev: &Event) {
    match ev {
        Event::WindowOpened(w) => {
            let addr = w.window_address.to_string();
            store.note_window(&addr, &w.window_class, &w.window_title);
            let manifests = crate::config::default_manifests_dir()
                .and_then(|d| crate::config::load_tag_manifests(&d).ok());
            for mark in auto_marks_for_class(&w.window_class, manifests.as_deref()) {
                if store.add_mark(&addr, &mark) {
                    publish_delta(persist, &addr, "add", &mark, store.list_marks(&addr));
                }
            }
        }
        Event::WindowClosed(addr) => {
            store.drop_window(&addr.to_string());
        }
        Event::ActiveWindowChanged(Some(w)) => {
            // Keep the identity record fresh (title changes etc.) so
            // the snapshot replay match stays accurate.
            store.note_window(&w.address.to_string(), &w.class, &w.title);
        }
        _ => {}
    }
}

/// Poll every `action/marks/<verb>` topic for requests newer than
/// the last cursor, dispatch each, write the reply, publish deltas.
fn poll_actions(persist: &Persist, store: &mut MarksStore, cursors: &mut HashMap<String, String>) {
    for verb in ACTION_VERBS {
        let topic = format!("action/marks/{verb}");
        let since = cursors.get(&topic).map(String::as_str);
        let msgs = match persist.list_since(&topic, since) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!(%topic, error = %e, "marks_state action poll failed");
                continue;
            }
        };
        for msg in msgs {
            cursors.insert(topic.clone(), msg.ulid.clone());
            let body = msg.body.as_deref().unwrap_or("");
            let outcome = dispatch_action(store, verb, body);
            if let Err(e) = persist.write(
                &reply_topic(&msg.ulid),
                Priority::Default,
                None,
                Some(&outcome.reply_json),
            ) {
                tracing::warn!(ulid = %msg.ulid, error = %e, "marks_state reply write failed");
            }
            if let Some((addr, op, mark, marks)) = outcome.delta {
                publish_delta(persist, &addr, &op, &mark, marks);
            }
        }
    }
}

/// Publish an `event/marks/<addr>` delta. Best-effort — a persist
/// write failure is logged, not fatal.
fn publish_delta(persist: &Persist, addr: &str, op: &str, mark: &str, marks: Vec<String>) {
    let delta = MarkDelta { addr, op, mark, marks };
    let Ok(body) = serde_json::to_string(&delta) else {
        return;
    };
    let topic = format!("event/marks/{addr}");
    if let Err(e) = persist.write(&topic, Priority::Default, None, Some(&body)) {
        tracing::debug!(%topic, error = %e, "marks_state delta publish failed");
    }
}

/// Load the on-disk snapshot into a store. Missing / unreadable /
/// malformed snapshot → `None` (first-boot or corrupt; start empty).
fn load_snapshot() -> Option<MarksStore> {
    let path = default_snapshot_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let snap: MarksSnapshot = toml::from_str(&raw).ok()?;
    Some(MarksStore::from_snapshot(&snap))
}

/// Persist the store to the snapshot path (atomic temp+rename).
fn save_snapshot(store: &MarksStore) -> std::io::Result<()> {
    let Some(path) = default_snapshot_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let snap = store.to_snapshot();
    let body = toml::to_string(&snap)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    let mut tmp = path.clone();
    tmp.set_extension("toml.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_mark_is_idempotent_and_sorted() {
        let mut s = MarksStore::default();
        assert!(s.add_mark("0x1", "web"));
        assert!(!s.add_mark("0x1", "web")); // dup → no change
        assert!(s.add_mark("0x1", "dev"));
        assert_eq!(s.list_marks("0x1"), vec!["dev", "web"]); // sorted
    }

    #[test]
    fn remove_mark_reports_change_and_prunes_empty() {
        let mut s = MarksStore::default();
        s.add_mark("0x1", "web");
        assert!(s.remove_mark("0x1", "web"));
        assert!(!s.remove_mark("0x1", "web")); // already gone
        assert!(s.list_marks("0x1").is_empty());
        assert!(!s.remove_mark("0xdead", "web")); // unknown addr
    }

    #[test]
    fn match_marks_finds_every_addr_sorted() {
        let mut s = MarksStore::default();
        s.add_mark("0x2", "web");
        s.add_mark("0x1", "web");
        s.add_mark("0x3", "dev");
        assert_eq!(s.match_marks("web"), vec!["0x1", "0x2"]);
        assert_eq!(s.match_marks("dev"), vec!["0x3"]);
        assert!(s.match_marks("none").is_empty());
    }

    #[test]
    fn drop_window_clears_marks_and_identity() {
        let mut s = MarksStore::default();
        s.note_window("0x1", "firefox", "Mozilla");
        s.add_mark("0x1", "web");
        s.drop_window("0x1");
        assert!(s.list_marks("0x1").is_empty());
        assert!(s.match_marks("web").is_empty());
    }

    #[test]
    fn snapshot_round_trip_preserves_marks_and_identity() {
        let mut s = MarksStore::default();
        s.note_window("0x1", "firefox", "Mozilla Firefox");
        s.add_mark("0x1", "web");
        s.add_mark("0x1", "priority");
        let snap = s.to_snapshot();
        let restored = MarksStore::from_snapshot(&snap);
        assert_eq!(restored.list_marks("0x1"), vec!["priority", "web"]);
        // Re-serialize → identical snapshot.
        assert_eq!(restored.to_snapshot(), snap);
    }

    #[test]
    fn snapshot_skips_windows_without_marks() {
        let mut s = MarksStore::default();
        s.note_window("0x1", "foot", "shell"); // identity but no marks
        let snap = s.to_snapshot();
        assert!(snap.windows.is_empty());
    }

    #[test]
    fn auto_marks_combines_taxonomy_and_manifest() {
        use crate::config::TagManifest;
        let manifests = vec![TagManifest {
            name: "voip".into(),
            apps: vec!["firefox".into()],
            marks_default: "priority,call".into(),
            ..TagManifest::default()
        }];
        // firefox is taxonomy `web` + manifest marks priority,call.
        let marks = auto_marks_for_class("firefox", Some(&manifests));
        assert!(marks.contains(&"web".to_string()));
        assert!(marks.contains(&"priority".to_string()));
        assert!(marks.contains(&"call".to_string()));
    }

    #[test]
    fn auto_marks_unknown_class_no_manifest_is_empty() {
        assert!(auto_marks_for_class("some-obscure-app", None).is_empty());
    }

    #[test]
    fn dispatch_add_returns_changed_and_delta() {
        let mut s = MarksStore::default();
        let out = dispatch_action(&mut s, "add", r#"{"addr":"0x1","mark":"web"}"#);
        assert!(out.reply_json.contains("\"ok\":true"));
        assert!(out.reply_json.contains("\"changed\":true"));
        let (addr, op, mark, marks) = out.delta.expect("delta on change");
        assert_eq!((addr.as_str(), op.as_str(), mark.as_str()), ("0x1", "add", "web"));
        assert_eq!(marks, vec!["web"]);
    }

    #[test]
    fn dispatch_add_dup_has_no_delta() {
        let mut s = MarksStore::default();
        s.add_mark("0x1", "web");
        let out = dispatch_action(&mut s, "add", r#"{"addr":"0x1","mark":"web"}"#);
        assert!(out.delta.is_none()); // no change → no delta
    }

    #[test]
    fn dispatch_list_and_match() {
        let mut s = MarksStore::default();
        s.add_mark("0x1", "web");
        let list = dispatch_action(&mut s, "list", r#"{"addr":"0x1"}"#);
        assert!(list.reply_json.contains("web"));
        let m = dispatch_action(&mut s, "match", r#"{"mark":"web"}"#);
        assert!(m.reply_json.contains("0x1"));
    }

    #[test]
    fn dispatch_malformed_body_fails_cleanly() {
        let mut s = MarksStore::default();
        let out = dispatch_action(&mut s, "add", "not json");
        assert!(out.reply_json.contains("\"ok\":false"));
        assert!(out.delta.is_none());
    }

    #[test]
    fn dispatch_unknown_verb_fails() {
        let mut s = MarksStore::default();
        let out = dispatch_action(&mut s, "frobnicate", "{}");
        assert!(out.reply_json.contains("\"ok\":false"));
    }
}
