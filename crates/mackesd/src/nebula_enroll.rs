//! NF-3.6.a (v2.5) — peer-enrollment helper.
//!
//! The Rust side of the v2.5 `mesh:<id>@<ip>:<port>#<bearer>`
//! join-token flow. Consumed by:
//!
//!   * the extended `mackesd enroll --token` CLI (bin/mackesd.rs)
//!   * the future `dev.mackes.MDE.Nebula.Enroll` D-Bus method
//!     (NF-3.6 — convenience shim over this module)
//!   * the wizard's Apply page (NF-14.4 / NF-14.5 — shells out to
//!     the CLI)
//!
//! Architecture: peer-side enrollment is QNM-Shared-mediated.
//! The peer writes a [`PendingEnrollment`] file at
//! `QNM-Shared/<self-id>/mackesd/pending-enroll.json`. The
//! lighthouse's `mackesd ca sign-csr <peer-id>` (NF-3.6.a CLI
//! helper) reads it, signs the cert, writes the
//! `nebula-bundle.json` back to QNM-Shared/<self-id>/mackesd/.
//! `nebula_supervisor` (NF-3.4) is already watching for that
//! bundle and materializes /etc/nebula/ when it appears.
//!
//! This module ships peer-side: publish the CSR + poll for the
//! signed bundle. Lighthouse-side signing is a separate concern
//! (see [`sign_pending_csr`]). The two flows share the [`JoinToken`]
//! parser + [`PendingEnrollment`] wire shape so they stay in lock-
//! step on every wire-format change.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::ca::bundle::{bundle_path, read_bundle};
use crate::enrollment::{build_identity, EnrolledIdentity};

/// QR-friendly upper bound on the wire-form join token. Matches
/// the Python helper's `JOIN_TOKEN_MAX_LEN` lock at
/// `mackes/wizard/pages/mesh_passcode.py`.
pub const JOIN_TOKEN_MAX_LEN: usize = 120;

/// Filename for the per-peer pending-enrollment CSR the
/// lighthouse looks for. Lives alongside `heartbeat.json` +
/// `nebula-bundle.json` in `QNM-Shared/<peer-id>/mackesd/`.
pub const PENDING_ENROLL_FILENAME: &str = "pending-enroll.json";

/// Default poll cadence — how often the peer-side waiter checks
/// for the signed bundle.
pub const ENROLL_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Default wait budget — peer-side waiter gives up after this
/// long and surfaces an informative timeout error pointing the
/// operator at the manual `mackesd ca sign-csr` recovery step.
pub const ENROLL_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Parsed wire-form of `mesh:<id>@<ip>:<port>#<bearer>`. Lock-step
/// with the Python `JoinToken` dataclass in
/// `mackes/wizard/pages/mesh_passcode.py` so the wizard + the
/// CLI consume the same shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinToken {
    /// URL-safe mesh identifier (no `@` / `:` / `#` / `/`).
    pub mesh_id: String,
    /// Lighthouse IPv4 address (overlay or public — both work).
    pub lighthouse: String,
    /// Lighthouse port (1..=65535).
    pub port: u16,
    /// Base32/URL-safe bearer the lighthouse validates against
    /// its pending-enroll allow-list.
    pub bearer: String,
}

impl JoinToken {
    /// Round-trip back to the wire form. Symmetric with
    /// [`parse_join_token`].
    #[must_use]
    pub fn encode(&self) -> String {
        format!(
            "mesh:{}@{}:{}#{}",
            self.mesh_id, self.lighthouse, self.port, self.bearer
        )
    }
}

/// Parse a wire-form join token. Returns `None` on any failure
/// (wrong shape, port out of range, non-IPv4 lighthouse, etc.).
/// Mirrors the Python `parse_join_token` rejection rules.
///
/// # Errors
///
/// Returns `None` rather than `Result` so the CLI / wizard /
/// D-Bus surface can render the same "invalid join token" copy
/// without branching on subtypes. Operators who need
/// fine-grained diagnostic messages should validate by
/// inspecting the wire shape directly.
#[must_use]
pub fn parse_join_token(raw: &str) -> Option<JoinToken> {
    if raw.is_empty() || raw.len() > JOIN_TOKEN_MAX_LEN {
        return None;
    }
    let stripped = raw.strip_prefix("mesh:")?;
    // mesh:<mesh_id>@<lighthouse>:<port>#<bearer>
    let (mesh_id, rest) = stripped.split_once('@')?;
    if mesh_id.is_empty() || !is_mesh_id_url_safe(mesh_id) {
        return None;
    }
    let (lighthouse_port, bearer) = rest.split_once('#')?;
    if bearer.is_empty() || !is_bearer_url_safe(bearer) {
        return None;
    }
    let (lighthouse, port_str) = lighthouse_port.rsplit_once(':')?;
    if lighthouse.is_empty() {
        return None;
    }
    let port: u16 = port_str.parse().ok()?;
    if port == 0 {
        return None;
    }
    if !is_ipv4(lighthouse) {
        return None;
    }
    Some(JoinToken {
        mesh_id: mesh_id.to_string(),
        lighthouse: lighthouse.to_string(),
        port,
        bearer: bearer.to_string(),
    })
}

fn is_mesh_id_url_safe(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn is_bearer_url_safe(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '=' | '-'))
}

fn is_ipv4(s: &str) -> bool {
    s.parse::<std::net::Ipv4Addr>().is_ok()
}

/// Wire shape of the per-peer pending-enrollment CSR the peer
/// publishes to QNM-Shared. The lighthouse reads it, validates
/// the bearer, signs a cert, writes the signed bundle back. JSON
/// for self-contained sneakernet replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingEnrollment {
    /// Token the peer presented. Carries the bearer the
    /// lighthouse cross-checks against its issued-but-unredeemed
    /// list.
    pub token: JoinToken,
    /// Peer's stable node id (e.g. `peer:anvil`). The lighthouse
    /// uses this as the row identifier in `nebula_peer_certs`.
    pub node_id: String,
    /// Hostname at enrollment time, for the display column on the
    /// roster.
    pub display_name: String,
    /// Hardware fingerprint — drives the idempotent re-enroll
    /// path. Lighthouse matches this against existing rows to
    /// refresh credentials in place when the same physical box
    /// re-enrolls.
    pub hw_fingerprint: String,
    /// Hex-encoded Ed25519 public key the peer just generated.
    /// The lighthouse signs a Nebula cert binding this key to the
    /// allocated overlay IP.
    pub public_key_hex: String,
    /// Unix-epoch seconds when the CSR was written. Used by the
    /// lighthouse to expire stale CSRs.
    pub created_at: i64,
}

/// Compute the per-peer pending-enrollment path under a
/// QNM-Shared root. Mirrors the `bundle_path` convention.
#[must_use]
pub fn pending_enroll_path(qnm_root: &Path, peer_id: &str) -> PathBuf {
    qnm_root
        .join(peer_id)
        .join("mackesd")
        .join(PENDING_ENROLL_FILENAME)
}

/// Outcome of a successful peer-side enrollment.
#[derive(Debug, Clone)]
pub struct EnrollOutcome {
    /// Overlay IP allocated by the lighthouse.
    pub overlay_ip: String,
    /// Lighthouse-side mesh-id confirmed by the bundle.
    pub mesh_id: String,
    /// Wall-clock time from CSR-publish to bundle-arrival.
    pub waited: Duration,
}

/// Errors a peer-side enrollment can hit. Each variant carries
/// the human-readable copy the CLI surfaces verbatim — keep them
/// actionable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnrollError {
    /// The wire-form token didn't parse. Includes the offending
    /// input length so operators can spot truncation.
    InvalidToken {
        /// Length of the rejected raw input.
        raw_len: usize,
    },
    /// Could not write the pending-enroll CSR (filesystem error).
    PublishFailed {
        /// Underlying error message.
        reason: String,
    },
    /// The lighthouse didn't sign within the wait budget. Carries
    /// the elapsed seconds so the CLI message can quote it.
    Timeout {
        /// Wall-clock seconds the waiter spent.
        elapsed_s: u64,
    },
    /// Bundle appeared but didn't parse — the lighthouse may have
    /// written a corrupt or version-mismatched file.
    BundleCorrupt {
        /// Underlying error message.
        reason: String,
    },
}

impl std::fmt::Display for EnrollError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidToken { raw_len } => write!(
                f,
                "invalid join token (length {raw_len}). \
                 Expected mesh:<id>@<ipv4>:<port>#<bearer>, max {JOIN_TOKEN_MAX_LEN} chars.",
            ),
            Self::PublishFailed { reason } => write!(
                f,
                "could not publish pending-enroll CSR: {reason}. \
                 Check QNM-Shared is mounted + writable for the mackes user.",
            ),
            Self::Timeout { elapsed_s } => write!(
                f,
                "waited {elapsed_s} s for the lighthouse to sign — \
                 no bundle appeared. Run `mackesd ca sign-csr \
                 <your-node-id>` on the lighthouse and retry.",
            ),
            Self::BundleCorrupt { reason } => write!(
                f,
                "bundle arrived but didn't parse: {reason}. \
                 The lighthouse may have written an incompatible \
                 version — confirm both sides are on the same MDE \
                 release.",
            ),
        }
    }
}

impl std::error::Error for EnrollError {}

/// Publish the pending-enroll CSR to QNM-Shared. Writes the file
/// atomically (temp + rename). Idempotent — re-running overwrites
/// the previous CSR (lighthouse always reads the latest version).
///
/// # Errors
///
/// Surfaces filesystem errors as [`EnrollError::PublishFailed`].
pub fn publish_enrollment_request(
    qnm_root: &Path,
    node_id: &str,
    pending: &PendingEnrollment,
) -> Result<PathBuf, EnrollError> {
    let path = pending_enroll_path(qnm_root, node_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| EnrollError::PublishFailed { reason: e.to_string() })?;
    }
    let body = serde_json::to_vec_pretty(pending)
        .map_err(|e| EnrollError::PublishFailed { reason: e.to_string() })?;
    // Atomic write: temp file + rename so a lighthouse polling
    // mid-write never reads a half-formed CSR.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &body)
        .map_err(|e| EnrollError::PublishFailed { reason: e.to_string() })?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| EnrollError::PublishFailed { reason: e.to_string() })?;
    Ok(path)
}

/// Wait for the lighthouse-signed bundle to appear in QNM-Shared.
/// Polls every [`ENROLL_POLL_INTERVAL`] until the bundle exists +
/// parses, or [`ENROLL_WAIT_TIMEOUT`] elapses. Returns the parsed
/// bundle on success.
///
/// # Errors
///
/// - [`EnrollError::Timeout`] when no bundle appears in the
///   budget.
/// - [`EnrollError::BundleCorrupt`] when a bundle appears but
///   doesn't parse.
pub fn wait_for_signed_bundle(
    qnm_root: &Path,
    node_id: &str,
    poll_interval: Duration,
    timeout: Duration,
) -> Result<(crate::ca::bundle::NebulaBundle, Duration), EnrollError> {
    let path = bundle_path(qnm_root, node_id);
    let started = Instant::now();
    loop {
        if path.exists() {
            match read_bundle(&path) {
                Ok(bundle) => return Ok((bundle, started.elapsed())),
                Err(e) => {
                    return Err(EnrollError::BundleCorrupt {
                        reason: e.to_string(),
                    });
                }
            }
        }
        if started.elapsed() >= timeout {
            return Err(EnrollError::Timeout {
                elapsed_s: started.elapsed().as_secs(),
            });
        }
        std::thread::sleep(poll_interval);
    }
}

/// End-to-end peer-side enrollment from a raw join-token string.
/// Generates a fresh identity, writes the CSR, waits for the
/// lighthouse to sign. Returns [`EnrollOutcome`] on success.
///
/// On a peer that IS the lighthouse (the first peer in a new
/// mesh), the caller is expected to run `mackesd ca mint`
/// separately + skip this enroll flow entirely — the lighthouse
/// signs its own cert via the mint path.
///
/// # Errors
///
/// Per [`EnrollError`].
pub fn enroll_with_token(
    qnm_root: &Path,
    node_id: &str,
    display_name: &str,
    raw_token: &str,
) -> Result<EnrollOutcome, EnrollError> {
    let token = parse_join_token(raw_token).ok_or(EnrollError::InvalidToken {
        raw_len: raw_token.len(),
    })?;
    let identity = build_identity();
    let pending = build_pending(&identity, node_id, display_name, token);
    publish_enrollment_request(qnm_root, node_id, &pending)?;
    let (bundle, waited) =
        wait_for_signed_bundle(qnm_root, node_id, ENROLL_POLL_INTERVAL, ENROLL_WAIT_TIMEOUT)?;
    Ok(EnrollOutcome {
        overlay_ip: bundle.overlay_ip,
        mesh_id: bundle.mesh_id,
        waited,
    })
}

/// Pure helper — build the PendingEnrollment payload from a
/// freshly-minted identity + the parsed token. Split out so tests
/// can exercise the shape without spinning the filesystem.
#[must_use]
pub fn build_pending(
    identity: &EnrolledIdentity,
    node_id: &str,
    display_name: &str,
    token: JoinToken,
) -> PendingEnrollment {
    let public_key_hex = hex_bytes(identity.key.verifying_key().as_bytes());
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    PendingEnrollment {
        token,
        node_id: node_id.to_string(),
        display_name: display_name.to_string(),
        hw_fingerprint: identity.hw_fingerprint.clone(),
        public_key_hex,
        created_at,
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ---- parse_join_token coverage --------------------------

    #[test]
    fn parse_round_trips_a_canonical_token() {
        let raw = "mesh:mesh-001@10.0.0.5:4242#dGVzdC1iZWFyZXItYWJjZGVm";
        let tok = parse_join_token(raw).expect("decoded");
        assert_eq!(tok.mesh_id, "mesh-001");
        assert_eq!(tok.lighthouse, "10.0.0.5");
        assert_eq!(tok.port, 4242);
        assert_eq!(tok.bearer, "dGVzdC1iZWFyZXItYWJjZGVm");
        assert_eq!(tok.encode(), raw);
    }

    #[test]
    fn parse_rejects_empty_and_oversized() {
        assert!(parse_join_token("").is_none());
        let long = format!(
            "mesh:m@10.0.0.5:4242#{}",
            "a".repeat(JOIN_TOKEN_MAX_LEN + 10)
        );
        assert!(parse_join_token(&long).is_none());
    }

    #[test]
    fn parse_rejects_wrong_scheme() {
        assert!(parse_join_token("https://example.com").is_none());
        assert!(parse_join_token("not-a-token").is_none());
        assert!(parse_join_token("MESH:m@10.0.0.5:4242#b").is_none());
    }

    #[test]
    fn parse_rejects_invalid_port() {
        // 0 and out-of-range both reject.
        assert!(parse_join_token("mesh:m@10.0.0.5:0#b").is_none());
        assert!(parse_join_token("mesh:m@10.0.0.5:99999#b").is_none());
        assert!(parse_join_token("mesh:m@10.0.0.5:abc#b").is_none());
    }

    #[test]
    fn parse_rejects_non_ipv4_lighthouse() {
        // IPv6 + hostname rejected per the v2.5 IPv4-only lock.
        assert!(parse_join_token("mesh:m@fe80::1:4242#b").is_none());
        assert!(parse_join_token("mesh:m@example.com:4242#b").is_none());
    }

    #[test]
    fn parse_rejects_empty_components() {
        assert!(parse_join_token("mesh:@10.0.0.5:4242#b").is_none());
        assert!(parse_join_token("mesh:m@10.0.0.5:4242#").is_none());
        assert!(parse_join_token("mesh:m@:4242#b").is_none());
    }

    #[test]
    fn parse_rejects_unsafe_mesh_id() {
        // @ / : / # / / are reserved separators — must reject.
        assert!(parse_join_token("mesh:bad@id@10.0.0.5:4242#b").is_none());
        // / not allowed in the URL-safe set per the Python lock.
        assert!(parse_join_token("mesh:bad/id@10.0.0.5:4242#b").is_none());
    }

    #[test]
    fn parse_accepts_url_safe_mesh_id() {
        for mesh_id in ["m", "mesh-001", "mesh_001", "mesh.001", "Mesh-A1.b_2"] {
            let raw = format!("mesh:{mesh_id}@10.0.0.5:4242#bearer");
            assert!(parse_join_token(&raw).is_some(), "{mesh_id}");
        }
    }

    // ---- error message ergonomics ---------------------------

    #[test]
    fn invalid_token_error_quotes_length() {
        let err = EnrollError::InvalidToken { raw_len: 99 };
        let s = err.to_string();
        assert!(s.contains("invalid join token"));
        assert!(s.contains("length 99"));
        assert!(s.contains("mesh:"));
    }

    #[test]
    fn timeout_error_quotes_elapsed_and_recovery_hint() {
        let err = EnrollError::Timeout { elapsed_s: 30 };
        let s = err.to_string();
        assert!(s.contains("waited 30 s"));
        assert!(s.contains("mackesd ca sign-csr"));
    }

    #[test]
    fn publish_failed_error_quotes_reason() {
        let err = EnrollError::PublishFailed {
            reason: "permission denied".into(),
        };
        let s = err.to_string();
        assert!(s.contains("permission denied"));
        assert!(s.contains("QNM-Shared"));
    }

    #[test]
    fn bundle_corrupt_error_quotes_reason() {
        let err = EnrollError::BundleCorrupt {
            reason: "missing field `mesh_id`".into(),
        };
        let s = err.to_string();
        assert!(s.contains("missing field"));
        assert!(s.contains("MDE release"));
    }

    // ---- publish + path conventions -------------------------

    #[test]
    fn pending_enroll_path_mirrors_bundle_path_convention() {
        let root = Path::new("/qnm");
        let p = pending_enroll_path(root, "peer:anvil");
        assert_eq!(
            p,
            PathBuf::from("/qnm/peer:anvil/mackesd/pending-enroll.json")
        );
    }

    #[test]
    fn publish_writes_atomically_and_creates_parent() {
        let tmp = tempdir().expect("tempdir");
        let identity = build_identity();
        let token = parse_join_token("mesh:m@10.0.0.5:4242#bearer").unwrap();
        let pending =
            build_pending(&identity, "peer:anvil", "anvil", token);
        let written = publish_enrollment_request(tmp.path(), "peer:anvil", &pending)
            .expect("publish");
        assert!(written.exists());
        let on_disk: PendingEnrollment =
            serde_json::from_slice(&std::fs::read(&written).unwrap()).unwrap();
        assert_eq!(on_disk.node_id, "peer:anvil");
        assert_eq!(on_disk.display_name, "anvil");
        assert_eq!(on_disk.public_key_hex.len(), 64);
    }

    #[test]
    fn publish_is_idempotent() {
        let tmp = tempdir().expect("tempdir");
        let identity = build_identity();
        let token = parse_join_token("mesh:m@10.0.0.5:4242#bearer").unwrap();
        let pending =
            build_pending(&identity, "peer:anvil", "anvil", token);
        let p1 = publish_enrollment_request(tmp.path(), "peer:anvil", &pending).unwrap();
        let p2 = publish_enrollment_request(tmp.path(), "peer:anvil", &pending).unwrap();
        assert_eq!(p1, p2);
        // Temp file shouldn't survive the atomic rename.
        let tmp_file = p2.with_extension("json.tmp");
        assert!(!tmp_file.exists());
    }

    // ---- wait_for_signed_bundle -----------------------------

    #[test]
    fn wait_returns_timeout_when_no_bundle_appears() {
        let tmp = tempdir().expect("tempdir");
        let r = wait_for_signed_bundle(
            tmp.path(),
            "peer:anvil",
            Duration::from_millis(50),
            Duration::from_millis(200),
        );
        match r {
            Err(EnrollError::Timeout { elapsed_s: _ }) => {} // OK
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn wait_returns_bundle_when_one_arrives() {
        use crate::ca::bundle::{write_bundle, LighthouseEntry, NebulaBundle};
        let tmp = tempdir().expect("tempdir");
        // Pre-place a valid bundle.
        let bundle = NebulaBundle {
            mesh_id: "m".into(),
            epoch: 0,
            ca_cert_pem: "CA".into(),
            peer_cert_pem: "CERT".into(),
            peer_key_pem: "KEY".into(),
            overlay_ip: "10.42.0.5".into(),
            mesh_cidr: "10.42.0.0/16".into(),
            lighthouses: vec![LighthouseEntry {
                node_id: "peer:lh".into(),
                overlay_ip: "10.42.0.1".into(),
                external_addr: "203.0.113.5:4242".into(),
            }],
            created_at: 1716000000,
        };
        write_bundle(&bundle_path(tmp.path(), "peer:anvil"), &bundle).expect("write");
        let (got, _waited) = wait_for_signed_bundle(
            tmp.path(),
            "peer:anvil",
            Duration::from_millis(50),
            Duration::from_secs(2),
        )
        .expect("ok");
        assert_eq!(got.overlay_ip, "10.42.0.5");
        assert_eq!(got.mesh_id, "m");
    }

    #[test]
    fn wait_returns_bundle_corrupt_on_invalid_json() {
        let tmp = tempdir().expect("tempdir");
        let p = bundle_path(tmp.path(), "peer:anvil");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{not valid").unwrap();
        let r = wait_for_signed_bundle(
            tmp.path(),
            "peer:anvil",
            Duration::from_millis(50),
            Duration::from_secs(1),
        );
        match r {
            Err(EnrollError::BundleCorrupt { .. }) => {}
            other => panic!("expected BundleCorrupt, got {other:?}"),
        }
    }

    // ---- end-to-end enroll_with_token -----------------------

    #[test]
    fn enroll_with_token_returns_invalid_for_garbage() {
        let tmp = tempdir().expect("tempdir");
        let r = enroll_with_token(tmp.path(), "peer:anvil", "anvil", "not a token");
        assert!(matches!(r, Err(EnrollError::InvalidToken { .. })));
    }
}
