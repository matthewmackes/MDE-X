//! GF-1.3.b (v5.0.0) — glusterd bind-address rewriter.
//!
//! Pure-function `rewrite_glusterd_vol(content, overlay_ip)`
//! takes the current `/etc/glusterfs/glusterd.vol` body + the
//! desired overlay IP and returns the new body with the bind
//! line inserted into the `volume management ... end-volume`
//! block (or `Unchanged` when the file already binds to the
//! right address).
//!
//! Defensive: refuses to edit any file that doesn't match the
//! expected Fedora 44 / upstream-`glusterfs-server` shape (a
//! `volume management` block ending in `end-volume`). Returns
//! [`RewriteOutcome::UnrecognizedFormat`] when the marker
//! pair is missing — the caller logs + skips rather than
//! corrupting an unfamiliar config.
//!
//! [`apply_bind`] wraps the pure rewriter with I/O: reads the
//! existing file (if any), runs the rewrite, atomic-writes
//! the new content back only when bytes differ. Returns the
//! same outcome enum so callers can decide whether to fire a
//! `systemctl reload glusterd.service`.

use std::io;
use std::path::{Path, PathBuf};

/// Default on-disk path of the Fedora-shipped glusterd
/// management config file. The helper takes the path as an
/// arg so tests + dev rigs use a tempdir, but production
/// callers (the future `mackesd gluster-nebula-bind` CLI +
/// the `nebula_supervisor::refresh_config` hook) pass this
/// constant.
pub const DEFAULT_GLUSTERD_VOL: &str = "/etc/glusterfs/glusterd.vol";

/// Verb used to recognize the management volume block. Fedora's
/// stock `/etc/glusterfs/glusterd.vol` starts with this line
/// (followed by `type mgmt/glusterd` and a sequence of
/// `option <key> <value>` entries). If a future glusterd
/// release renames the marker, callers see
/// [`RewriteOutcome::UnrecognizedFormat`] rather than a
/// scrambled config.
pub const VOLUME_HEADER: &str = "volume management";

/// Block terminator.
pub const VOLUME_FOOTER: &str = "end-volume";

/// Key we own inside the management block.
pub const BIND_OPTION_KEY: &str = "option transport.socket.bind-address";

/// Outcome of a single rewrite attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewriteOutcome {
    /// The body needed an update — `Wrote(new_content)`
    /// carries the bytes the caller should `write_atomic`.
    Wrote(String),
    /// The body already contained the correct bind line; no
    /// write needed.
    Unchanged,
    /// The body didn't match the expected
    /// `volume management ... end-volume` shape; the caller
    /// must NOT overwrite the file (would scramble unfamiliar
    /// config).
    UnrecognizedFormat,
}

/// Pure transformation: take the existing file body + the
/// desired overlay IP, return the new body (or `Unchanged` /
/// `UnrecognizedFormat`).
///
/// Idempotent + deterministic: same inputs → same output
/// bytes, no time / randomness.
///
/// Insertion strategy:
///
/// 1. Locate the `volume management` line (the body of the
///    block extends until `end-volume`).
/// 2. If a line inside that block already matches
///    [`BIND_OPTION_KEY`], replace it with the new bind
///    line. Indentation + whitespace are preserved on the
///    leading edge; only the value flips.
/// 3. Otherwise, insert a new `    option transport.socket.bind-address <ip>`
///    line immediately before the block's `end-volume`
///    terminator.
/// 4. If the `volume management` line OR the `end-volume`
///    terminator is missing, refuse with `UnrecognizedFormat`.
#[must_use]
pub fn rewrite_glusterd_vol(content: &str, overlay_ip: &str) -> RewriteOutcome {
    let lines: Vec<&str> = content.lines().collect();

    let header_idx = lines
        .iter()
        .position(|l| l.trim() == VOLUME_HEADER);
    let Some(header_idx) = header_idx else {
        return RewriteOutcome::UnrecognizedFormat;
    };

    let footer_idx = lines
        .iter()
        .enumerate()
        .skip(header_idx + 1)
        .find_map(|(i, l)| (l.trim() == VOLUME_FOOTER).then_some(i));
    let Some(footer_idx) = footer_idx else {
        return RewriteOutcome::UnrecognizedFormat;
    };

    let block_range = (header_idx + 1)..footer_idx;
    let mut existing_bind_idx: Option<usize> = None;
    for i in block_range.clone() {
        let trimmed = lines[i].trim_start();
        if trimmed.starts_with(BIND_OPTION_KEY) {
            existing_bind_idx = Some(i);
            break;
        }
    }

    let desired_bind_line = format!("    {BIND_OPTION_KEY} {overlay_ip}");

    if let Some(idx) = existing_bind_idx {
        if lines[idx] == desired_bind_line {
            return RewriteOutcome::Unchanged;
        }
        let mut out_lines: Vec<String> = lines.iter().map(|s| (*s).to_string()).collect();
        out_lines[idx] = desired_bind_line;
        return RewriteOutcome::Wrote(reassemble(&out_lines, content));
    }

    // No existing bind line — insert one immediately before
    // the `end-volume` terminator.
    let mut out_lines: Vec<String> = Vec::with_capacity(lines.len() + 1);
    for (i, l) in lines.iter().enumerate() {
        if i == footer_idx {
            out_lines.push(desired_bind_line.clone());
        }
        out_lines.push((*l).to_string());
    }
    RewriteOutcome::Wrote(reassemble(&out_lines, content))
}

/// Preserve the original file's trailing-newline convention.
/// Most Linux config files end with a final `\n`; some
/// hand-edited ones don't. We round-trip whichever shape the
/// caller had.
fn reassemble(lines: &[String], original: &str) -> String {
    let mut out = lines.join("\n");
    if original.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Outcome of [`apply_bind`] — same shape as `RewriteOutcome`
/// but the `Wrote` variant carries no payload because the
/// caller has already moved bytes to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// File was missing (operator hasn't installed
    /// `glusterfs-server` yet, or this is a fresh peer
    /// pre-enroll). Caller treats this as a no-op.
    Missing,
    /// File existed + already matched the desired bind line.
    Unchanged,
    /// File existed + we atomic-wrote the new content. The
    /// caller should now `systemctl reload glusterd.service`
    /// (or `restart` if the running glusterd doesn't honor a
    /// bind-address reload).
    Wrote,
    /// File existed but didn't match the expected shape; we
    /// left it alone.
    UnrecognizedFormat,
}

/// Read `vol_path`, rewrite the body, atomic-write back if
/// changed.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] when read or write
/// fails. A missing file is NOT an error — it surfaces as
/// `Ok(ApplyOutcome::Missing)`.
pub fn apply_bind(vol_path: &Path, overlay_ip: &str) -> io::Result<ApplyOutcome> {
    let body = match std::fs::read_to_string(vol_path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(ApplyOutcome::Missing);
        }
        Err(e) => return Err(e),
    };

    match rewrite_glusterd_vol(&body, overlay_ip) {
        RewriteOutcome::Unchanged => Ok(ApplyOutcome::Unchanged),
        RewriteOutcome::UnrecognizedFormat => Ok(ApplyOutcome::UnrecognizedFormat),
        RewriteOutcome::Wrote(new_body) => {
            let tmp: PathBuf = vol_path.with_extension("vol.tmp");
            if let Some(parent) = tmp.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&tmp, new_body.as_bytes())?;
            std::fs::rename(&tmp, vol_path)?;
            Ok(ApplyOutcome::Wrote)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock_vol() -> &'static str {
        "volume management\n\
         \x20\x20\x20\x20type mgmt/glusterd\n\
         \x20\x20\x20\x20option working-directory /var/lib/glusterd\n\
         \x20\x20\x20\x20option transport-type socket,rdma\n\
         \x20\x20\x20\x20option ping-timeout 0\n\
         \x20\x20\x20\x20option event-threads 1\n\
         end-volume\n"
    }

    fn vol_with_bind(ip: &str) -> String {
        format!(
            "volume management\n\
             \x20\x20\x20\x20type mgmt/glusterd\n\
             \x20\x20\x20\x20option working-directory /var/lib/glusterd\n\
             \x20\x20\x20\x20option transport-type socket,rdma\n\
             \x20\x20\x20\x20option transport.socket.bind-address {ip}\n\
             \x20\x20\x20\x20option ping-timeout 0\n\
             \x20\x20\x20\x20option event-threads 1\n\
             end-volume\n",
        )
    }

    #[test]
    fn inserts_bind_line_into_clean_vol() {
        let RewriteOutcome::Wrote(new) = rewrite_glusterd_vol(stock_vol(), "10.42.0.5") else {
            panic!("expected Wrote outcome");
        };
        assert!(new.contains("option transport.socket.bind-address 10.42.0.5"));
        // Bind line lands inside the management block, not at
        // the end of the file.
        let bind_pos = new
            .find("option transport.socket.bind-address")
            .expect("bind present");
        let footer_pos = new.find("end-volume").expect("footer present");
        assert!(bind_pos < footer_pos);
    }

    #[test]
    fn replaces_existing_bind_with_new_ip() {
        let starting = vol_with_bind("10.42.0.5");
        let RewriteOutcome::Wrote(new) = rewrite_glusterd_vol(&starting, "10.42.0.7") else {
            panic!("expected Wrote outcome");
        };
        assert!(new.contains("option transport.socket.bind-address 10.42.0.7"));
        // Old IP should be gone.
        assert!(!new.contains("option transport.socket.bind-address 10.42.0.5"));
        // Exactly one bind line should remain.
        let count = new.matches("transport.socket.bind-address").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn returns_unchanged_when_already_correct() {
        let starting = vol_with_bind("10.42.0.5");
        let out = rewrite_glusterd_vol(&starting, "10.42.0.5");
        assert_eq!(out, RewriteOutcome::Unchanged);
    }

    #[test]
    fn refuses_when_volume_management_header_missing() {
        let bad = "# only a comment, no management block here\n";
        let out = rewrite_glusterd_vol(bad, "10.42.0.5");
        assert_eq!(out, RewriteOutcome::UnrecognizedFormat);
    }

    #[test]
    fn refuses_when_end_volume_terminator_missing() {
        let bad = "volume management\n    type mgmt/glusterd\n";
        let out = rewrite_glusterd_vol(bad, "10.42.0.5");
        assert_eq!(out, RewriteOutcome::UnrecognizedFormat);
    }

    #[test]
    fn preserves_unrelated_options_unchanged() {
        let RewriteOutcome::Wrote(new) = rewrite_glusterd_vol(stock_vol(), "10.42.0.5") else {
            panic!("expected Wrote outcome");
        };
        for option in [
            "option working-directory /var/lib/glusterd",
            "option transport-type socket,rdma",
            "option ping-timeout 0",
            "option event-threads 1",
            "type mgmt/glusterd",
        ] {
            assert!(
                new.contains(option),
                "missing expected option: {option}\n{new}",
            );
        }
    }

    #[test]
    fn preserves_trailing_newline_convention_present() {
        let RewriteOutcome::Wrote(new) = rewrite_glusterd_vol(stock_vol(), "10.42.0.5") else {
            panic!("Wrote outcome");
        };
        assert!(new.ends_with('\n'));
    }

    #[test]
    fn preserves_trailing_newline_convention_absent() {
        let no_trailing = stock_vol().trim_end_matches('\n').to_owned();
        let RewriteOutcome::Wrote(new) = rewrite_glusterd_vol(&no_trailing, "10.42.0.5") else {
            panic!("Wrote outcome");
        };
        assert!(!new.ends_with('\n'));
    }

    // apply_bind I/O tests

    #[test]
    fn apply_bind_returns_missing_for_absent_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("nope.vol");
        let out = apply_bind(&path, "10.42.0.5").expect("ok");
        assert_eq!(out, ApplyOutcome::Missing);
    }

    #[test]
    fn apply_bind_writes_when_changed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("glusterd.vol");
        std::fs::write(&path, stock_vol()).expect("seed");
        let out = apply_bind(&path, "10.42.0.5").expect("ok");
        assert_eq!(out, ApplyOutcome::Wrote);
        let new_body = std::fs::read_to_string(&path).expect("read");
        assert!(new_body.contains("option transport.socket.bind-address 10.42.0.5"));
    }

    #[test]
    fn apply_bind_idempotent_second_call_is_unchanged() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("glusterd.vol");
        std::fs::write(&path, stock_vol()).expect("seed");
        let first = apply_bind(&path, "10.42.0.5").expect("first");
        assert_eq!(first, ApplyOutcome::Wrote);
        let second = apply_bind(&path, "10.42.0.5").expect("second");
        assert_eq!(second, ApplyOutcome::Unchanged);
    }

    #[test]
    fn apply_bind_leaves_no_tempfile_on_success() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("glusterd.vol");
        std::fs::write(&path, stock_vol()).expect("seed");
        apply_bind(&path, "10.42.0.5").expect("ok");
        let tmp_path = path.with_extension("vol.tmp");
        assert!(!tmp_path.exists());
    }

    #[test]
    fn apply_bind_refuses_unfamiliar_format() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("glusterd.vol");
        std::fs::write(&path, "# something weird\n").expect("seed");
        let out = apply_bind(&path, "10.42.0.5").expect("ok");
        assert_eq!(out, ApplyOutcome::UnrecognizedFormat);
        // File MUST stay untouched.
        let body = std::fs::read_to_string(&path).expect("read");
        assert_eq!(body, "# something weird\n");
    }

    #[test]
    fn default_path_matches_fedora_44_stock_location() {
        assert_eq!(DEFAULT_GLUSTERD_VOL, "/etc/glusterfs/glusterd.vol");
    }
}
