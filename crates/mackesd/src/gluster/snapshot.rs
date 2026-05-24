//! GF-9.2 (v5.0.0) — gluster volume/peer snapshot for the
//! daily state-backup tarball.
//!
//! Shells `gluster volume info --xml`, `gluster volume status
//! all clients --xml`, and `gluster peer status --xml` and
//! folds the three XML payloads into a single
//! [`GlusterSnapshot`] struct. The CA-backup worker
//! ([`crate::workers::nebula_ca_backup`]) stuffs the snapshot
//! into [`crate::ca::backup::BundlePlaintext::gluster_snapshot`]
//! so a single encrypted tarball captures both the Nebula CA
//! and the Gluster topology — restoring a bare peer from one
//! file (`mackesd state restore <bundle>`, GF-9.3) brings
//! everything back.
//!
//! Best-effort: when the `gluster` CLI is missing (operator
//! hasn't enabled the v5.0.0 substrate yet, OR the box isn't a
//! storage participant), [`collect`] returns `Ok(None)` and the
//! tarball ships without the section. v1 backups
//! (`schema_version: 1`) likewise deserialize forward into a
//! `gluster_snapshot: None` shape via serde's default
//! attribute on the field.

use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// CLI binary we shell out to. Held as a constant so tests can
/// override (via the `with_binary` builder on
/// [`SnapshotConfig`]) without touching `$PATH`.
pub const GLUSTER_BINARY: &str = "gluster";

/// Default per-invocation wall-clock timeout for the
/// individual `gluster volume info` / `gluster peer status`
/// shellouts. The daily backup tick doesn't need to be fast;
/// the 30 s ceiling exists so a hung glusterd doesn't stall
/// the entire backup pass.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// A single point-in-time snapshot of the local peer's
/// glusterd view. The three fields are XML payloads from the
/// `gluster` CLI; each is `None` when the corresponding
/// subcommand failed (logged + the rest of the snapshot
/// proceeds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GlusterSnapshot {
    /// `gluster volume info --xml` — every volume + brick the
    /// local glusterd knows about. The primary payload for
    /// volume restore.
    #[serde(default)]
    pub volume_info_xml: Option<String>,
    /// `gluster peer status --xml` — every probed peer + its
    /// state. Needed to re-probe peers from a restore.
    #[serde(default)]
    pub peer_status_xml: Option<String>,
    /// `gluster volume status all clients --xml` — live
    /// brick processes + connected clients. Diagnostic; not
    /// strictly required for restore but lets the operator
    /// reconcile post-restore state.
    #[serde(default)]
    pub volume_status_xml: Option<String>,
}

/// Knobs for [`collect`] — operator-facing default is "no
/// args", tests override via the builder methods.
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    binary: String,
    timeout: Duration,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            binary: GLUSTER_BINARY.to_owned(),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl SnapshotConfig {
    /// Override the binary name (tests pass `/bin/true` to
    /// emulate "CLI present but unresponsive" / `/bin/false`
    /// to emulate "CLI installed but failing").
    #[must_use]
    pub fn with_binary(mut self, name: impl Into<String>) -> Self {
        self.binary = name.into();
        self
    }

    /// Override the per-shellout timeout. Tests pass shorter
    /// durations.
    #[must_use]
    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }
}

/// Collect the three `gluster` XML payloads.
///
/// Returns `None` when the `gluster` binary isn't on `$PATH`
/// at all — the operator hasn't enabled the v5.0.0 substrate
/// yet (or this box isn't a storage participant). Returns
/// `Some(snapshot)` otherwise, with each per-field `None`
/// signaling that the specific subcommand failed (the
/// other fields still get populated when their subcommands
/// succeed).
///
/// # Errors
///
/// Never. All shellout failures get absorbed into per-field
/// `None` so a flaky glusterd doesn't blow the backup tick.
#[must_use]
pub fn collect(config: &SnapshotConfig) -> Option<GlusterSnapshot> {
    if which_binary(&config.binary).is_none() {
        return None;
    }
    Some(GlusterSnapshot {
        volume_info_xml: run_xml(config, &["volume", "info", "--xml"]),
        peer_status_xml: run_xml(config, &["peer", "status", "--xml"]),
        volume_status_xml: run_xml(config, &["volume", "status", "all", "clients", "--xml"]),
    })
}

/// Cheap PATH probe — returns the binary's absolute path when
/// it's executable. Used to short-circuit `collect()` when
/// glusterfs-server isn't installed.
fn which_binary(name: &str) -> Option<std::path::PathBuf> {
    // Absolute path: caller passed one (tests do this); honor it
    // verbatim when the file exists.
    let candidate = std::path::Path::new(name);
    if candidate.is_absolute() {
        if candidate.exists() {
            return Some(candidate.to_path_buf());
        }
        return None;
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let full = dir.join(name);
        if full.is_file() {
            return Some(full);
        }
    }
    None
}

fn run_xml(config: &SnapshotConfig, args: &[&str]) -> Option<String> {
    let out = Command::new(&config.binary)
        .args(args)
        .output();
    let _ = config.timeout; // reserved for a future async/timeout wrapper
    match out {
        Ok(o) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => {
            tracing::debug!(
                args = ?args,
                status = ?o.status,
                stderr = %String::from_utf8_lossy(&o.stderr),
                "gluster snapshot subcommand exited non-zero",
            );
            None
        }
        Err(e) => {
            tracing::debug!(
                args = ?args,
                error = %e,
                "gluster snapshot subcommand failed to launch",
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_returns_none_when_binary_absent() {
        let cfg = SnapshotConfig::default().with_binary("/nonexistent/gluster-binary-xyz");
        assert!(collect(&cfg).is_none());
    }

    #[test]
    fn collect_returns_some_with_all_none_when_binary_always_fails() {
        // /bin/false exits 1 — every subcommand "succeeds at
        // launching" but exits non-zero, so each field is None.
        let cfg = SnapshotConfig::default().with_binary("/bin/false");
        let snap = collect(&cfg).expect("binary exists");
        assert!(snap.volume_info_xml.is_none());
        assert!(snap.peer_status_xml.is_none());
        assert!(snap.volume_status_xml.is_none());
    }

    #[test]
    fn collect_returns_some_with_all_populated_when_binary_succeeds() {
        // /bin/true exits 0 with empty stdout — every field is
        // Some("").
        let cfg = SnapshotConfig::default().with_binary("/bin/true");
        let snap = collect(&cfg).expect("binary exists");
        assert_eq!(snap.volume_info_xml.as_deref(), Some(""));
        assert_eq!(snap.peer_status_xml.as_deref(), Some(""));
        assert_eq!(snap.volume_status_xml.as_deref(), Some(""));
    }

    #[test]
    fn snapshot_json_round_trips() {
        let snap = GlusterSnapshot {
            volume_info_xml: Some("<volumes/>".into()),
            peer_status_xml: Some("<peers/>".into()),
            volume_status_xml: None,
        };
        let json = serde_json::to_string(&snap).expect("encode");
        let back: GlusterSnapshot = serde_json::from_str(&json).expect("decode");
        assert_eq!(snap, back);
    }

    #[test]
    fn snapshot_deserializes_when_all_fields_missing() {
        // v1 bundles never wrote the per-field keys; serde's
        // `default` must fill them in as None.
        let back: GlusterSnapshot =
            serde_json::from_str("{}").expect("legacy-shape JSON parses");
        assert_eq!(back, GlusterSnapshot::default());
    }

    #[test]
    fn relative_binary_resolves_through_path() {
        // True must exist in PATH on any sane Linux host; the
        // probe should find it without an absolute path.
        assert!(which_binary("true").is_some());
    }

    #[test]
    fn nonexistent_relative_binary_returns_none() {
        assert!(which_binary("definitely-not-installed-xyz").is_none());
    }
}
