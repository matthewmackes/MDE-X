//! BUS-5.9 — KDC2 clipboard bridge: phone ↔ mesh bus topic.
//!
//! Two functions complete the bidirectional bridge:
//!
//! * [`phone_to_bus`] — called by the KDC2 host when
//!   `ClipboardPlugin::take_received()` drains an inbound phone
//!   clipboard event. Formats the text content as a Mackes Bus
//!   message (same wire shape as `mde-clipd`'s `publish.rs`) and
//!   writes it atomically to `<bus_root>/clipboard/sync/<ulid>.json`.
//!   The clipboard popover (BUS-5.6) and every mesh subscriber
//!   (BUS-5.4) can immediately read it as if `mde-clipd` had
//!   published it.
//!
//! * [`new_bus_entries_since`] — called by the KDC2 host on each
//!   poll tick. Reads `clipboard/sync/*.json`, skips messages from
//!   the local peer (echo prevention), returns text content of
//!   entries whose ULID is strictly greater than `cursor`. Updates
//!   `cursor` to the highest seen ULID. The host feeds each returned
//!   string into `ClipboardPlugin::push_clipboard()`, which queues a
//!   `kdeconnect.clipboard` packet for transmission to the paired
//!   phone.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

// ── Wire types (must match mde-clipd/src/publish.rs exactly) ─────────────

#[derive(Debug, Serialize, Deserialize)]
struct BusEnvelope {
    ulid: String,
    topic: String,
    priority: String,
    title: Option<String>,
    body: String,
    ts_unix_ms: i64,
    file_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BusSyncMsg {
    publisher_peer: String,
    mime_types: Vec<String>,
    selected_mime: String,
    payload: BusPayload,
    ts_iso: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BusPayload {
    Inline { data_b64: String },
    BlobRef { path: String },
}

// ── phone_to_bus ──────────────────────────────────────────────────────────

/// Publish `content` received from a KDE Connect phone to the Mackes
/// Bus `clipboard/sync` topic.
///
/// The written file is immediately readable by:
/// - `mde-popover clipboard` (BUS-5.6 Super+V popover)
/// - `mde-clipd` subscriber thread (BUS-5.4 mesh sync)
///
/// # Errors
///
/// Returns an error if the topic directory cannot be created or the
/// atomic write (temp → rename) fails.
pub fn phone_to_bus(
    content: &str,
    phone_device_id: &str,
    bus_root: &Path,
    _data_home: &Path,
) -> std::io::Result<()> {
    let ulid = Ulid::new().to_string();
    let ts_iso = chrono::Utc::now().to_rfc3339();
    let ts_unix_ms = chrono::Utc::now().timestamp_millis();

    let data_b64 = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
    let sync_msg = BusSyncMsg {
        publisher_peer: phone_device_id.to_owned(),
        mime_types: vec!["text/plain".into()],
        selected_mime: "text/plain".into(),
        payload: BusPayload::Inline { data_b64 },
        ts_iso,
    };
    let body = serde_json::to_string(&sync_msg).map_err(std::io::Error::other)?;

    let topic_dir = bus_root.join("clipboard/sync");
    std::fs::create_dir_all(&topic_dir)?;

    let file_name = format!("{ulid}.json");
    let file_path_str = format!("clipboard/sync/{file_name}");
    let envelope = BusEnvelope {
        ulid: ulid.clone(),
        topic: "clipboard/sync".into(),
        priority: "normal".into(),
        title: None,
        body,
        ts_unix_ms,
        file_path: file_path_str,
    };

    let json = serde_json::to_string_pretty(&envelope).map_err(std::io::Error::other)?;
    let dest = topic_dir.join(&file_name);
    let tmp = dest.with_extension("tmp");
    std::fs::write(&tmp, json.as_bytes())?;
    std::fs::rename(&tmp, &dest)?;
    Ok(())
}

// ── new_bus_entries_since ─────────────────────────────────────────────────

/// Scan `<bus_root>/clipboard/sync/` for text entries with ULID
/// strictly greater than `*cursor` from a peer other than
/// `local_peer_id`.
///
/// On return, `*cursor` is advanced to the highest ULID seen across
/// ALL entries (including local-peer entries, to avoid replaying them
/// on the next call). Returns the text content strings ready to push
/// to the paired phone.
///
/// Caller passes an empty string as the initial `cursor` value; on
/// subsequent calls the cursor is the ULID returned by the previous
/// invocation.
pub fn new_bus_entries_since(
    bus_root: &Path,
    local_peer_id: &str,
    cursor: &mut String,
) -> Vec<String> {
    let topic_dir = bus_root.join("clipboard/sync");
    let rd = match std::fs::read_dir(&topic_dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    // Collect all JSON paths, sort lexicographically (= ULID time order).
    let mut paths: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map_or(false, |x| x == "json"))
        .collect();
    paths.sort();

    // Build a sorted set of ULIDs we see this pass (for cursor advance).
    let mut seen_ulids: BTreeSet<String> = BTreeSet::new();
    let mut results: Vec<String> = Vec::new();

    for path in &paths {
        let Ok(raw) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(env) = serde_json::from_str::<BusEnvelope>(&raw) else {
            continue;
        };
        let ulid_str = env.ulid.clone();
        seen_ulids.insert(ulid_str.clone());

        // Skip entries at or before the cursor (already processed).
        if !cursor.is_empty() && ulid_str.as_str() <= cursor.as_str() {
            continue;
        }

        // Parse inner body.
        let Ok(msg) = serde_json::from_str::<BusSyncMsg>(&env.body) else {
            continue;
        };

        // Skip own-peer entries (echo prevention).
        if msg.publisher_peer == local_peer_id {
            continue;
        }

        // Only text/plain — binary payloads not bridgeable via KDC2 clipboard.
        if msg.selected_mime != "text/plain" {
            continue;
        }

        let text = match &msg.payload {
            BusPayload::Inline { data_b64 } => {
                base64::engine::general_purpose::STANDARD
                    .decode(data_b64)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                    .unwrap_or_default()
            }
            BusPayload::BlobRef { path } => {
                std::fs::read_to_string(path).unwrap_or_default()
            }
        };

        if !text.is_empty() {
            results.push(text);
        }
    }

    // Advance cursor to the highest ULID seen.
    if let Some(highest) = seen_ulids.into_iter().next_back() {
        if highest.as_str() > cursor.as_str() {
            *cursor = highest;
        }
    }

    results
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bus_msg(
        bus_root: &Path,
        ulid: &str,
        peer_id: &str,
        content: &str,
    ) {
        let data_b64 = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
        let msg = BusSyncMsg {
            publisher_peer: peer_id.to_owned(),
            mime_types: vec!["text/plain".into()],
            selected_mime: "text/plain".into(),
            payload: BusPayload::Inline { data_b64 },
            ts_iso: "2026-05-30T00:00:00Z".into(),
        };
        let body = serde_json::to_string(&msg).unwrap();
        let envelope = BusEnvelope {
            ulid: ulid.to_owned(),
            topic: "clipboard/sync".into(),
            priority: "normal".into(),
            title: None,
            body,
            ts_unix_ms: 0,
            file_path: format!("clipboard/sync/{ulid}.json"),
        };
        let topic_dir = bus_root.join("clipboard/sync");
        std::fs::create_dir_all(&topic_dir).unwrap();
        std::fs::write(
            topic_dir.join(format!("{ulid}.json")),
            serde_json::to_string_pretty(&envelope).unwrap(),
        )
        .unwrap();
    }

    // ── phone_to_bus ──────────────────────────────────────────────────────

    #[test]
    fn phone_to_bus_creates_json_file() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        let data_home = tmp.path().to_path_buf();

        phone_to_bus("hello from phone", "pixel-6a", &bus_root, &data_home).unwrap();

        let topic_dir = bus_root.join("clipboard/sync");
        let files: Vec<_> = std::fs::read_dir(&topic_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1, "exactly one bus file created");
    }

    #[test]
    fn phone_to_bus_file_contains_correct_content() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        let data_home = tmp.path().to_path_buf();

        phone_to_bus("sync me", "my-pixel", &bus_root, &data_home).unwrap();

        let topic_dir = bus_root.join("clipboard/sync");
        let path = std::fs::read_dir(&topic_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let raw = std::fs::read_to_string(&path).unwrap();
        let env: BusEnvelope = serde_json::from_str(&raw).unwrap();
        assert_eq!(env.topic, "clipboard/sync");
        let msg: BusSyncMsg = serde_json::from_str(&env.body).unwrap();
        assert_eq!(msg.publisher_peer, "my-pixel");
        assert_eq!(msg.selected_mime, "text/plain");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(match &msg.payload {
                BusPayload::Inline { data_b64 } => data_b64.as_bytes(),
                _ => panic!("expected inline"),
            })
            .unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), "sync me");
    }

    #[test]
    fn phone_to_bus_no_temp_file_left() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        let data_home = tmp.path().to_path_buf();

        phone_to_bus("atomic", "phone", &bus_root, &data_home).unwrap();

        let topic_dir = bus_root.join("clipboard/sync");
        let leftovers: Vec<_> = std::fs::read_dir(&topic_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map_or(false, |x| x == "tmp")
            })
            .collect();
        assert!(leftovers.is_empty(), "no .tmp files left after atomic write");
    }

    // ── new_bus_entries_since ─────────────────────────────────────────────

    #[test]
    fn entries_from_other_peer_returned() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        make_bus_msg(&bus_root, "01AAAAAAAAAAAAAAAAAAAAAAAAA", "peer-02", "hello mesh");

        let mut cursor = String::new();
        let results = new_bus_entries_since(&bus_root, "local-peer", &mut cursor);
        assert_eq!(results, vec!["hello mesh"]);
        assert_eq!(cursor, "01AAAAAAAAAAAAAAAAAAAAAAAAA");
    }

    #[test]
    fn own_peer_entries_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        make_bus_msg(&bus_root, "01BBBBBBBBBBBBBBBBBBBBBBBBB", "local-peer", "own copy");

        let mut cursor = String::new();
        let results = new_bus_entries_since(&bus_root, "local-peer", &mut cursor);
        assert!(results.is_empty(), "own-peer entries must be skipped");
        // Cursor still advances past the seen ULID.
        assert_eq!(cursor, "01BBBBBBBBBBBBBBBBBBBBBBBBB");
    }

    #[test]
    fn cursor_prevents_replay() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        make_bus_msg(&bus_root, "01CCCCCCCCCCCCCCCCCCCCCCCCC", "peer-02", "first");

        let mut cursor = String::new();
        let first = new_bus_entries_since(&bus_root, "local", &mut cursor);
        assert_eq!(first.len(), 1);

        // Second call — no new messages.
        let second = new_bus_entries_since(&bus_root, "local", &mut cursor);
        assert!(second.is_empty(), "replay must be suppressed by cursor");
    }

    #[test]
    fn new_entries_after_cursor_are_returned() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        make_bus_msg(&bus_root, "01DDDDDDDDDDDDDDDDDDDDDDDDD", "peer-02", "old");

        let mut cursor = String::new();
        let _ = new_bus_entries_since(&bus_root, "local", &mut cursor);

        // Add a newer message.
        make_bus_msg(&bus_root, "01EEEEEEEEEEEEEEEEEEEEEEEEE", "peer-02", "new");
        let new_results = new_bus_entries_since(&bus_root, "local", &mut cursor);
        assert_eq!(new_results, vec!["new"]);
    }

    #[test]
    fn missing_bus_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        let mut cursor = String::new();
        let results = new_bus_entries_since(&bus_root, "local", &mut cursor);
        assert!(results.is_empty());
        assert!(cursor.is_empty(), "cursor unchanged when dir missing");
    }

    #[test]
    fn multiple_peers_all_included() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        make_bus_msg(&bus_root, "01FFFFFFFFFFFFFFFFFFFFFFF0A", "peer-02", "from p2");
        make_bus_msg(&bus_root, "01FFFFFFFFFFFFFFFFFFFFFFF0B", "peer-03", "from p3");

        let mut cursor = String::new();
        let results = new_bus_entries_since(&bus_root, "local", &mut cursor);
        assert_eq!(results.len(), 2);
        assert!(results.contains(&"from p2".to_string()));
        assert!(results.contains(&"from p3".to_string()));
    }

    // ── plugin integration (emulated KDC2 client) ─────────────────────────

    #[test]
    fn phone_clipboard_written_to_bus_and_readable() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        let data_home = tmp.path().to_path_buf();

        // Simulate phone sending clipboard → host calls phone_to_bus.
        phone_to_bus("phone text", "pixel-7a", &bus_root, &data_home).unwrap();

        // Another peer (or same machine) reads back via new_bus_entries_since.
        let mut cursor = String::new();
        let entries = new_bus_entries_since(&bus_root, "local-mde-peer", &mut cursor);
        assert_eq!(entries, vec!["phone text"]);
    }

    #[test]
    fn plugin_push_then_take_outbound_forms_valid_packet() {
        use mde_kdc_proto::plugins::clipboard::{from_packet_body, ClipboardBody, ClipboardPlugin};

        let mut plugin = ClipboardPlugin::new();
        plugin.push_clipboard("mesh clip".to_string());
        let packets = plugin.take_outbound();
        assert_eq!(packets.len(), 1);
        let body: ClipboardBody = from_packet_body(&packets[0]).unwrap();
        assert_eq!(body.content, "mesh clip");
    }
}
