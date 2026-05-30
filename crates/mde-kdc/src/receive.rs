//! MESHFS-15.1 / GF-5.1 / GF-15.1 — KDC2 inbound file-receive handler.
//!
//! Handles `kdeconnect.share.request` packets where the phone ships an
//! actual binary file (not just a URL). Two distinct actions:
//!
//!   1. **Drop-folder creation** [`ensure_phone_drop_folder`] —
//!      idempotently creates `~/Documents/From-<phone-name>/` at pairing
//!      time (GF-15.1) and on first file receive (GF-5.1). Under LizardFS
//!      mesh-storage, `~/Documents/` is a replicated export — the drop
//!      folder inherits replication automatically on first sync.
//!
//!   2. **Binary pull** [`ingest_file_share`] — opens a new pinned-TLS
//!      connection to `peer_addr:transfer_info.port`, reads exactly
//!      `share.payload_size` bytes, and writes them to
//!      `<drop_dir>/<filename>`. SHA-256 integrity check runs if
//!      `share.payload_hash` is non-empty; partial files are removed on
//!      failure.

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use mde_kdc_proto::plugins::share::{ShareBody, ShareKind};
use mde_kdc_proto::wire::PayloadTransferInfo;

// ─────────────────────────────────────────────────────────────────────────────
// Drop-folder helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build the canonical drop-folder path for `phone_name` under `base_dir`.
/// Pure path arithmetic — no I/O.
fn drop_folder_path(base_dir: &Path, phone_name: &str) -> PathBuf {
    base_dir
        .join("Documents")
        .join(format!("From-{}", sanitize_phone_name(phone_name)))
}

/// Create `~/Documents/From-<phone_name>/` idempotently.
///
/// Called at pairing time (GF-15.1) AND on every inbound file receive
/// (GF-5.1). `create_dir_all` is a no-op when the directory already
/// exists, so both call sites are safe to call this unconditionally.
pub fn ensure_phone_drop_folder(phone_name: &str) -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME not set".to_owned())?;
    let dir = drop_folder_path(&home, phone_name);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create_dir_all {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Strip characters that are unsafe in a directory-name component.
/// Keeps alphanumerics, spaces, hyphens, underscores, and dots —
/// the character set Android device names commonly use.
pub(crate) fn sanitize_phone_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || matches!(*c, ' ' | '-' | '_' | '.'))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// File-name sanitization + collision avoidance
// ─────────────────────────────────────────────────────────────────────────────

/// Sanitize an incoming filename: map path separators, NUL, and control
/// characters to `_`. Preserves everything else (including the extension).
/// Falls back to `"received"` for empty names.
pub(crate) fn sanitize_file_name(name: &str) -> String {
    if name.is_empty() {
        return "received".to_owned();
    }
    name.chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == '\0' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Return `dir/name`, bumping to `dir/<stem>.1<ext>`,
/// `dir/<stem>.2<ext>`, … when the target already exists.
pub(crate) fn unique_dest_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name);
    let ext = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    for n in 1u32.. {
        let c = dir.join(format!("{stem}.{n}{ext}"));
        if !c.exists() {
            return c;
        }
    }
    unreachable!("u32 exhausted")
}

// ─────────────────────────────────────────────────────────────────────────────
// Binary pull
// ─────────────────────────────────────────────────────────────────────────────

/// Outcome of a successful [`ingest_file_share`] call.
#[derive(Debug)]
pub struct ReceivedFile {
    /// Absolute path of the saved file.
    pub path: PathBuf,
    /// Bytes written.
    pub bytes: u64,
}

/// Pull an inbound file from the phone and write it to the drop folder.
///
/// Opens a fresh pinned-TLS connection to `peer_addr:transfer_info.port`
/// using `pinned_fingerprint` (the value stored in the pairing record), reads
/// `share.payload_size` bytes, and writes them to
/// `<drop_dir>/<safe_filename>`. SHA-256 integrity check runs if
/// `share.payload_hash` is non-empty; partial files are removed on mismatch.
///
/// Calls `ensure_phone_drop_folder` first so the drop directory always
/// exists when this returns `Ok`.
pub async fn ingest_file_share(
    share: &ShareBody,
    phone_name: &str,
    peer_addr: IpAddr,
    transfer_info: &PayloadTransferInfo,
    pinned_fingerprint: Option<String>,
) -> Result<ReceivedFile, String> {
    if share.kind() != ShareKind::File {
        return Err(format!(
            "ingest_file_share: expected File share, got {:?}",
            share.kind()
        ));
    }

    let drop_dir = ensure_phone_drop_folder(phone_name)?;
    let safe_name = sanitize_file_name(&share.filename);
    let dest = unique_dest_path(&drop_dir, &safe_name);

    let sock_addr = std::net::SocketAddr::new(peer_addr, transfer_info.port);
    // The peer's TLS cert CN is the device-id, but for the SNI field we use
    // the sanitized phone name (matching what KDC stock clients send in their
    // CN). The pinned-fingerprint verifier ignores the SNI anyway — only the
    // cert fingerprint matters for trust decisions.
    let server_name = sanitize_phone_name(phone_name);
    let mut stream = crate::tls::connect_pinned_tls(sock_addr, &server_name, pinned_fingerprint)
        .await
        .map_err(|e| format!("TLS connect {sock_addr}: {e}"))?;

    pull_payload_to_path(&mut stream, share.payload_size, &dest, &share.payload_hash).await?;

    Ok(ReceivedFile {
        path: dest,
        bytes: share.payload_size,
    })
}

/// Read exactly `size` bytes from `stream` into `dest`.
///
/// SHA-256-checks the written bytes when `expected_hex` is non-empty;
/// removes the partial file on EOF-before-size or hash mismatch.
async fn pull_payload_to_path<R>(
    stream: &mut R,
    size: u64,
    dest: &Path,
    expected_hex: &str,
) -> Result<(), String>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("create {}: {e}", dest.display()))?;

    let check_hash = !expected_hex.is_empty();
    let mut hasher = Sha256::new();
    let mut remaining = size;
    let mut buf = vec![0u8; 65536];

    while remaining > 0 {
        let want = (remaining as usize).min(buf.len());
        let got = stream
            .read(&mut buf[..want])
            .await
            .map_err(|e| format!("read payload: {e}"))?;
        if got == 0 {
            let _ = tokio::fs::remove_file(dest).await;
            return Err(format!("unexpected EOF with {remaining} bytes remaining"));
        }
        file.write_all(&buf[..got])
            .await
            .map_err(|e| format!("write {}: {e}", dest.display()))?;
        if check_hash {
            hasher.update(&buf[..got]);
        }
        remaining -= got as u64;
    }

    file.flush()
        .await
        .map_err(|e| format!("flush {}: {e}", dest.display()))?;
    drop(file);

    if check_hash {
        let digest = hasher.finalize();
        let hex: String = digest.iter().fold(String::new(), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        });
        if hex != expected_hex {
            let _ = tokio::fs::remove_file(dest).await;
            return Err(format!(
                "sha256 mismatch: expected {expected_hex} got {hex}"
            ));
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ── Drop-folder path helpers ─────────────────────────────────────────────

    #[test]
    fn drop_folder_path_structure() {
        let base = Path::new("/home/alice");
        let p = drop_folder_path(base, "Pixel 9 Pro");
        assert_eq!(p, Path::new("/home/alice/Documents/From-Pixel 9 Pro"));
    }

    #[test]
    fn drop_folder_path_sanitizes_name() {
        let base = Path::new("/home/alice");
        let p = drop_folder_path(base, "Alice's Phone");
        // apostrophe is stripped by sanitize_phone_name
        assert_eq!(p, Path::new("/home/alice/Documents/From-Alices Phone"));
    }

    // ── sanitize_phone_name ──────────────────────────────────────────────────

    #[test]
    fn sanitize_phone_name_strips_path_separators_and_specials() {
        assert_eq!(sanitize_phone_name("../evil"), "..evil");
        assert_eq!(sanitize_phone_name("Alice's Phone"), "Alices Phone");
        assert_eq!(sanitize_phone_name("My-Phone.5"), "My-Phone.5");
        assert_eq!(sanitize_phone_name("Pixel 9 Pro"), "Pixel 9 Pro");
    }

    #[test]
    fn sanitize_phone_name_empty_stays_empty() {
        assert_eq!(sanitize_phone_name(""), "");
    }

    // ── sanitize_file_name ───────────────────────────────────────────────────

    #[test]
    fn sanitize_file_name_strips_path_traversal() {
        assert_eq!(sanitize_file_name("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(sanitize_file_name("report.pdf"), "report.pdf");
    }

    #[test]
    fn sanitize_file_name_empty_becomes_received() {
        assert_eq!(sanitize_file_name(""), "received");
    }

    #[test]
    fn sanitize_file_name_strips_nul_and_control_chars() {
        assert_eq!(sanitize_file_name("fi\0le"), "fi_le");
        assert_eq!(sanitize_file_name("ab\x01cd"), "ab_cd");
    }

    // ── unique_dest_path ─────────────────────────────────────────────────────

    #[test]
    fn unique_dest_path_returns_bare_when_not_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let p = unique_dest_path(tmp.path(), "foo.txt");
        assert_eq!(p, tmp.path().join("foo.txt"));
    }

    #[test]
    fn unique_dest_path_bumps_on_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let p1 = tmp.path().join("foo.txt");
        fs::write(&p1, b"x").unwrap();
        let p2 = unique_dest_path(tmp.path(), "foo.txt");
        assert_eq!(p2, tmp.path().join("foo.1.txt"));
        fs::write(&p2, b"x").unwrap();
        let p3 = unique_dest_path(tmp.path(), "foo.txt");
        assert_eq!(p3, tmp.path().join("foo.2.txt"));
    }

    #[test]
    fn unique_dest_path_no_extension_bumps_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let p1 = tmp.path().join("Makefile");
        fs::write(&p1, b"x").unwrap();
        let p2 = unique_dest_path(tmp.path(), "Makefile");
        assert_eq!(p2, tmp.path().join("Makefile.1"));
    }

    // ── ingest_file_share ────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn ingest_file_share_rejects_url_kind() {
        // Early return before any HOME lookup or network I/O.
        let body = ShareBody {
            url: "https://example.com".to_string(),
            ..Default::default()
        };
        let result = ingest_file_share(
            &body,
            "My Phone",
            "127.0.0.1".parse().unwrap(),
            &PayloadTransferInfo { port: 1740 },
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected File share"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ingest_file_share_rejects_empty_kind() {
        let body = ShareBody::default();
        let result = ingest_file_share(
            &body,
            "My Phone",
            "127.0.0.1".parse().unwrap(),
            &PayloadTransferInfo { port: 1740 },
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected File share"));
    }

    // ── pull_payload_to_path ─────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn pull_payload_writes_and_passes_sha256() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out.bin");
        let payload = b"hello receive".as_slice();
        let mut h = Sha256::new();
        h.update(payload);
        let hex: String = h
            .finalize()
            .iter()
            .fold(String::new(), |mut s, b| {
                use std::fmt::Write as _;
                let _ = write!(s, "{b:02x}");
                s
            });

        let mut cursor = std::io::Cursor::new(payload.to_vec());
        pull_payload_to_path(&mut cursor, payload.len() as u64, &dest, &hex)
            .await
            .unwrap();

        assert_eq!(fs::read(&dest).unwrap(), payload);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pull_payload_removes_file_on_hash_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("bad.bin");
        let payload = b"some content".to_vec();
        let mut cursor = std::io::Cursor::new(payload.clone());
        let result =
            pull_payload_to_path(&mut cursor, payload.len() as u64, &dest, "deadbeef").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("sha256 mismatch"));
        assert!(!dest.exists(), "partial file must be removed on hash failure");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pull_payload_skips_hash_when_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("nohash.bin");
        let payload = b"data".to_vec();
        let mut cursor = std::io::Cursor::new(payload.clone());
        pull_payload_to_path(&mut cursor, payload.len() as u64, &dest, "")
            .await
            .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), payload);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pull_payload_removes_file_on_early_eof() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("short.bin");
        let payload = b"too short".to_vec();
        let actual_len = payload.len() as u64;
        let mut cursor = std::io::Cursor::new(payload);
        // Claim more bytes than the stream contains.
        let result = pull_payload_to_path(&mut cursor, actual_len + 10, &dest, "").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unexpected EOF"));
        assert!(!dest.exists(), "partial file must be removed on early EOF");
    }
}
