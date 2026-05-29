//! BUS-5.8 — Simultaneous-copy conflict resolution.
//!
//! When multiple peers publish to `clipboard/sync` within the same poll
//! cycle (one-second window), `resolve_batch` merges or selects the
//! winning payload:
//!
//! - All-text (`text/*`) payloads → concatenate in ascending ULID order
//!   with `\n--- @<peer> ---\n` separators between them.
//! - Non-text or mixed payloads → last-write-wins by ULID (highest ULID
//!   is the most-recent write, wins outright).
//!
//! ULID strings are lexicographically sortable in timestamp order, so no
//! clock injection is needed — the tests are deterministic by ULID value.

/// A decoded inbound clipboard message ready for conflict resolution.
pub struct DecodedMsg {
    /// ULID of the bus message. Lexicographic order equals time order.
    pub ulid: String,
    /// `publisher_peer` field from the bus envelope (not the local peer).
    pub peer: String,
    /// `selected_mime` from the `ClipboardSyncMsg`.
    pub mime: String,
    /// Decoded payload bytes.
    pub data: Vec<u8>,
}

/// Resolve a batch of simultaneous inbound clipboard messages into a
/// single `(payload, mime)` to apply to the local clipboard.
///
/// Returns `None` if `msgs` is empty.
pub fn resolve_batch(mut msgs: Vec<DecodedMsg>) -> Option<(Vec<u8>, String)> {
    match msgs.len() {
        0 => return None,
        1 => {
            let m = msgs.remove(0);
            return Some((m.data, m.mime));
        }
        _ => {}
    }

    // Stable sort ascending by ULID — consistent for merge and LWW.
    msgs.sort_by(|a, b| a.ulid.cmp(&b.ulid));

    let all_text = msgs.iter().all(|m| m.mime.starts_with("text/"));

    if all_text {
        let mut merged: Vec<u8> = Vec::new();
        for (i, msg) in msgs.iter().enumerate() {
            if i > 0 {
                // Separator identifies the peer whose content follows.
                merged.extend_from_slice(b"\n--- @");
                merged.extend_from_slice(msg.peer.as_bytes());
                merged.extend_from_slice(b" ---\n");
            }
            merged.extend_from_slice(&msg.data);
        }
        let mime = msgs[0].mime.clone();
        Some((merged, mime))
    } else {
        // LWW: highest ULID (last in ascending sort) wins.
        let winner = msgs.swap_remove(msgs.len() - 1);
        Some((winner.data, winner.mime))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(ulid: &str, peer: &str, mime: &str, data: &[u8]) -> DecodedMsg {
        DecodedMsg {
            ulid: ulid.to_string(),
            peer: peer.to_string(),
            mime: mime.to_string(),
            data: data.to_vec(),
        }
    }

    #[test]
    fn empty_batch_returns_none() {
        assert!(resolve_batch(vec![]).is_none());
    }

    #[test]
    fn single_text_returns_unchanged() {
        let (data, mime) = resolve_batch(vec![msg("A", "peer-b", "text/plain", b"hello")]).unwrap();
        assert_eq!(data, b"hello");
        assert_eq!(mime, "text/plain");
    }

    #[test]
    fn single_non_text_returns_unchanged() {
        let (data, mime) =
            resolve_batch(vec![msg("A", "peer-b", "image/png", b"\x89PNG")]).unwrap();
        assert_eq!(data, b"\x89PNG");
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn two_text_from_different_peers_merges() {
        let msgs = vec![
            msg("B", "peer-b", "text/plain", b"from B"),
            msg("A", "peer-a", "text/plain", b"from A"),
        ];
        // "A" < "B" → peer-a first, peer-b second.
        let (data, mime) = resolve_batch(msgs).unwrap();
        assert_eq!(mime, "text/plain");
        let text = String::from_utf8(data).unwrap();
        assert_eq!(text, "from A\n--- @peer-b ---\nfrom B");
    }

    #[test]
    fn merge_preserves_ulid_order_ascending() {
        let msgs = vec![
            msg("C", "peer-c", "text/plain", b"third"),
            msg("A", "peer-a", "text/plain", b"first"),
            msg("B", "peer-b", "text/plain", b"second"),
        ];
        let (data, _) = resolve_batch(msgs).unwrap();
        let text = String::from_utf8(data).unwrap();
        assert_eq!(text, "first\n--- @peer-b ---\nsecond\n--- @peer-c ---\nthird");
    }

    #[test]
    fn non_text_batch_uses_lww_highest_ulid() {
        let msgs = vec![
            msg("A", "peer-a", "image/png", b"old"),
            msg("B", "peer-b", "image/png", b"new"),
        ];
        // "B" > "A" → peer-b wins.
        let (data, mime) = resolve_batch(msgs).unwrap();
        assert_eq!(data, b"new");
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn mixed_text_and_non_text_uses_lww() {
        let msgs = vec![
            msg("A", "peer-a", "text/plain", b"text"),
            msg("B", "peer-b", "image/png", b"image"),
        ];
        // Mixed → LWW; "B" > "A" → image wins.
        let (data, mime) = resolve_batch(msgs).unwrap();
        assert_eq!(data, b"image");
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn separator_format_exact_bytes() {
        let msgs = vec![
            msg("A", "host1", "text/plain", b"X"),
            msg("B", "host2", "text/plain", b"Y"),
        ];
        let (data, _) = resolve_batch(msgs).unwrap();
        assert_eq!(data, b"X\n--- @host2 ---\nY");
    }
}
