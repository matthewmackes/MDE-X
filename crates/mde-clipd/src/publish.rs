//! BUS-5.2 — clipboard event publisher.
//!
//! Writes clipboard payloads to the `clipboard/sync` Bus topic using the
//! authoritative file-tree format (`<bus_root>/clipboard/sync/<ulid>.json`).
//! The per-peer SQLite index is maintained by the mackesd bus subsystem;
//! `mde_bus::persist::detect_divergence` reconciles any gap between the
//! file tree and the index on the next mackesd startup.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Context as _;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Payloads ≤ 64 KB go inline as base64; larger are stored as blob refs.
pub const INLINE_THRESHOLD: usize = 64 * 1024;

/// Bus topic for clipboard synchronisation (BUS-5.2 lock).
pub const CLIPBOARD_TOPIC: &str = "clipboard/sync";

// ── Message types ──────────────────────────────────────────────────────────

/// The clipboard payload: either the raw bytes (base64-encoded for transport)
/// or a path reference to a local blob file (for large payloads).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipboardPayload {
    /// Raw bytes encoded as standard base64, ≤ [`INLINE_THRESHOLD`] bytes
    /// unencoded. Fits in the bus message body field.
    Inline { data_b64: String },
    /// Blob written to `<data_home>/mde/clipboard/blobs/`; body holds a
    /// reference. BUS-5.3 adds lifecycle/GC for orphaned blobs.
    BlobRef {
        path: String,
        size_bytes: usize,
        ext: String,
    },
}

/// The message body published to `clipboard/sync`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardSyncMsg {
    /// Hostname of the peer that copied this content.
    pub publisher_peer: String,
    /// All MIME types advertised by the clipboard source.
    pub mime_types: Vec<String>,
    /// The MIME type that was actually read and is contained in `payload`.
    pub selected_mime: String,
    /// The clipboard content, inline or by reference.
    pub payload: ClipboardPayload,
    /// RFC 3339 timestamp of the publish.
    pub ts_iso: String,
}

/// Outer envelope matching `mde_bus::persist::StoredMessage` on disk.
/// Written directly to the bus topic tree so mackesd's index can reconcile it.
#[derive(Debug, Serialize, Deserialize)]
struct BusEnvelope {
    ulid: String,
    topic: String,
    priority: String,
    title: Option<String>,
    body: Option<String>,
    ts_unix_ms: i64,
    file_path: String,
}

// ── Helper functions ───────────────────────────────────────────────────────

/// Guess a file extension from a MIME type (for blob filenames).
fn ext_for_mime(mime: &str) -> &'static str {
    // Strip charset/parameter suffix (e.g. "text/plain;charset=utf-8").
    let base = mime.split(';').next().unwrap_or(mime).trim();
    match base {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "text/plain" => "txt",
        "text/html" => "html",
        "application/json" => "json",
        "application/pdf" => "pdf",
        _ => "bin",
    }
}

/// Returns `<data_home>/mde/clipboard/blobs/`.
pub fn blob_dir(data_home: &Path) -> PathBuf {
    data_home.join("mde").join("clipboard").join("blobs")
}

/// Best-effort peer identifier: `$HOSTNAME` → `/proc/sys/kernel/hostname` →
/// `"local"`. Matches the fallback chain used by `mde_bus::persist`.
pub fn local_peer_id() -> String {
    if let Ok(v) = std::env::var("HOSTNAME") {
        let t = v.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }
    if let Ok(body) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
        let t = body.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }
    "local".to_string()
}

// ── Publisher entry point ──────────────────────────────────────────────────

/// Publish a clipboard event to the `clipboard/sync` bus topic.
///
/// If `data.len() ≤ INLINE_THRESHOLD`, the payload is included inline as
/// base64. Otherwise the raw bytes are written to
/// `<data_home>/mde/clipboard/blobs/<ulid>.<ext>` and only a reference is
/// published (BUS-5.3 adds lifecycle/GC).
///
/// The JSON file is written atomically (tmp + rename) to match the format
/// of `mde_bus::persist::StoredMessage` so mackesd's `detect_divergence`
/// can back-fill the SQLite index without data loss.
///
/// # Errors
///
/// Returns an error when directory creation, file write, or JSON encoding
/// fails.
pub fn publish_clipboard(
    bus_root: &Path,
    data_home: &Path,
    peer_id: &str,
    mime_types: &[String],
    selected_mime: &str,
    data: &[u8],
) -> anyhow::Result<()> {
    let ts_unix_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let ts_iso = chrono::Utc::now().to_rfc3339();

    let payload = if data.len() <= INLINE_THRESHOLD {
        ClipboardPayload::Inline {
            data_b64: base64::engine::general_purpose::STANDARD.encode(data),
        }
    } else {
        let blobs = blob_dir(data_home);
        std::fs::create_dir_all(&blobs)
            .with_context(|| format!("mkdir {}", blobs.display()))?;
        let ext = ext_for_mime(selected_mime);
        let blob_ulid = ulid::Ulid::new().to_string();
        let blob_path = blobs.join(format!("{blob_ulid}.{ext}"));
        std::fs::write(&blob_path, data)
            .with_context(|| format!("write blob {}", blob_path.display()))?;
        ClipboardPayload::BlobRef {
            path: blob_path.to_string_lossy().into_owned(),
            size_bytes: data.len(),
            ext: ext.to_string(),
        }
    };

    let clip_msg = ClipboardSyncMsg {
        publisher_peer: peer_id.to_string(),
        mime_types: mime_types.to_vec(),
        selected_mime: selected_mime.to_string(),
        payload,
        ts_iso,
    };
    let body_json = serde_json::to_string(&clip_msg).context("serialize ClipboardSyncMsg")?;

    // Write atomically to the bus topic tree (tmp + rename).
    let ulid = ulid::Ulid::new().to_string();
    let topic_dir = bus_root.join(CLIPBOARD_TOPIC);
    std::fs::create_dir_all(&topic_dir)
        .with_context(|| format!("mkdir {}", topic_dir.display()))?;

    let file_name = format!("{ulid}.json");
    let abs_path = topic_dir.join(&file_name);
    let rel_path = format!("{CLIPBOARD_TOPIC}/{file_name}");

    let envelope = BusEnvelope {
        ulid: ulid.clone(),
        topic: CLIPBOARD_TOPIC.to_string(),
        priority: "default".to_string(),
        title: None,
        body: Some(body_json),
        ts_unix_ms,
        file_path: rel_path,
    };

    let json = serde_json::to_string_pretty(&envelope).context("serialize BusEnvelope")?;
    let tmp = abs_path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())
        .with_context(|| format!("write tmp {}", tmp.display()))?;
    std::fs::rename(&tmp, &abs_path)
        .with_context(|| format!("rename to {}", abs_path.display()))?;

    info!(
        ulid = %ulid,
        mime = selected_mime,
        size = data.len(),
        "clipboard: published to clipboard/sync"
    );
    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_threshold_is_64_kb() {
        assert_eq!(INLINE_THRESHOLD, 65_536);
    }

    #[test]
    fn ext_for_known_mimes() {
        assert_eq!(ext_for_mime("image/png"), "png");
        assert_eq!(ext_for_mime("image/jpeg"), "jpg");
        assert_eq!(ext_for_mime("text/plain"), "txt");
        assert_eq!(ext_for_mime("text/plain;charset=utf-8"), "txt");
        assert_eq!(ext_for_mime("application/json"), "json");
        assert_eq!(ext_for_mime("application/octet-stream"), "bin");
    }

    #[test]
    fn inline_payload_round_trip() {
        let data = b"hello, world";
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        let p = ClipboardPayload::Inline {
            data_b64: encoded.clone(),
        };
        let j = serde_json::to_string(&p).unwrap();
        let p2: ClipboardPayload = serde_json::from_str(&j).unwrap();
        assert_eq!(p, p2);
        if let ClipboardPayload::Inline { data_b64 } = p2 {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(data_b64)
                .unwrap();
            assert_eq!(decoded, data);
        } else {
            panic!("expected Inline");
        }
    }

    #[test]
    fn blob_ref_round_trip() {
        let p = ClipboardPayload::BlobRef {
            path: "/tmp/test.png".to_string(),
            size_bytes: 1_048_576,
            ext: "png".to_string(),
        };
        let j = serde_json::to_string(&p).unwrap();
        let p2: ClipboardPayload = serde_json::from_str(&j).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn publish_inline_writes_file_and_correct_format() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        let data_home = tmp.path().to_path_buf();

        publish_clipboard(
            &bus_root,
            &data_home,
            "peer1",
            &["text/plain".to_string()],
            "text/plain",
            b"hello",
        )
        .unwrap();

        let topic_dir = bus_root.join("clipboard/sync");
        let entries: Vec<_> = std::fs::read_dir(&topic_dir).unwrap().collect();
        assert_eq!(entries.len(), 1, "expected exactly one file");

        let content =
            std::fs::read_to_string(entries.into_iter().next().unwrap().unwrap().path()).unwrap();
        let env: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(env["topic"], "clipboard/sync");
        assert_eq!(env["priority"], "default");
        assert!(env["title"].is_null());

        let body_str = env["body"].as_str().unwrap();
        let msg: ClipboardSyncMsg = serde_json::from_str(body_str).unwrap();
        assert_eq!(msg.publisher_peer, "peer1");
        assert_eq!(msg.selected_mime, "text/plain");

        let ClipboardPayload::Inline { data_b64 } = msg.payload else {
            panic!("expected Inline payload for small data");
        };
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data_b64)
            .unwrap();
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn publish_large_data_writes_blob_ref() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        let data_home = tmp.path().to_path_buf();
        let big_data = vec![0u8; INLINE_THRESHOLD + 1];

        publish_clipboard(
            &bus_root,
            &data_home,
            "peer1",
            &["image/png".to_string()],
            "image/png",
            &big_data,
        )
        .unwrap();

        // Blob file should exist under data_home/mde/clipboard/blobs/
        let blobs = blob_dir(&data_home);
        let blob_entries: Vec<_> = std::fs::read_dir(&blobs).unwrap().collect();
        assert_eq!(blob_entries.len(), 1, "expected one blob file");
        let blob_name = blob_entries
            .into_iter()
            .next()
            .unwrap()
            .unwrap()
            .file_name();
        assert!(
            blob_name.to_string_lossy().ends_with(".png"),
            "blob should have .png extension, got {blob_name:?}"
        );

        // Bus message should contain a BlobRef
        let topic_dir = bus_root.join("clipboard/sync");
        let msg_entries: Vec<_> = std::fs::read_dir(&topic_dir).unwrap().collect();
        let content =
            std::fs::read_to_string(msg_entries.into_iter().next().unwrap().unwrap().path())
                .unwrap();
        let env: serde_json::Value = serde_json::from_str(&content).unwrap();
        let body_str = env["body"].as_str().unwrap();
        let msg: ClipboardSyncMsg = serde_json::from_str(body_str).unwrap();
        assert!(
            matches!(msg.payload, ClipboardPayload::BlobRef { .. }),
            "expected BlobRef for large payload"
        );
    }

    #[test]
    fn atomic_write_no_tmp_leftover() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        let data_home = tmp.path().to_path_buf();
        publish_clipboard(
            &bus_root,
            &data_home,
            "p",
            &["text/plain".to_string()],
            "text/plain",
            b"x",
        )
        .unwrap();

        let topic_dir = bus_root.join("clipboard/sync");
        for entry in std::fs::read_dir(&topic_dir).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(
                !name.to_string_lossy().ends_with(".tmp"),
                "found leftover .tmp file: {name:?}"
            );
        }
    }

    #[test]
    fn two_publishes_produce_two_files_with_distinct_ulids() {
        let tmp = tempfile::tempdir().unwrap();
        let bus_root = tmp.path().join("bus");
        let data_home = tmp.path().to_path_buf();

        publish_clipboard(&bus_root, &data_home, "p", &["text/plain".to_string()], "text/plain", b"a").unwrap();
        publish_clipboard(&bus_root, &data_home, "p", &["text/plain".to_string()], "text/plain", b"b").unwrap();

        let topic_dir = bus_root.join("clipboard/sync");
        let entries: Vec<_> = std::fs::read_dir(&topic_dir).unwrap().collect();
        assert_eq!(entries.len(), 2, "expected two files");

        let mut ulids: Vec<String> = entries
            .into_iter()
            .map(|e| {
                let env_str = std::fs::read_to_string(e.unwrap().path()).unwrap();
                let env: serde_json::Value = serde_json::from_str(&env_str).unwrap();
                env["ulid"].as_str().unwrap().to_string()
            })
            .collect();
        ulids.sort();
        ulids.dedup();
        assert_eq!(ulids.len(), 2, "ULIDs must be distinct");
    }
}
