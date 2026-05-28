//! v4.0.1 — headless dnf-update poll daemon (was: Win10 watermark
//! popover).
//!
//! Originally a long-running Iced layer-shell surface anchored to
//! the bottom-right corner of the primary output that showed the
//! pending-dnf-update count. The 2026-05-23 operator pass retired
//! the visible widget — the same information now lives in the
//! start-menu footer (Win10-style system-identity strip). The dnf
//! poll thread + cache-file maintenance stays here as a headless
//! daemon so the start menu always has a fresh count to read on
//! every open.
//!
//! Public surface consumed by other popovers:
//!   * [`WatermarkState`] + [`WatermarkState::load`] — snapshot of
//!     MDE version, Fedora release, build hash/date, hostname, and
//!     pending-update count.
//!   * [`WatermarkState::identity_line`] — short system-identity
//!     string used by `start_menu.rs`'s footer.
//!   * [`current_pending_count`] — fast cache read (no dnf spawn).
//!   * [`spawn_pkexec_dnf_upgrade`] — pkexec-elevated dnf upgrade,
//!     fired by the start-menu's "N updates pending" button.
//!
//! [`run`] is the binary entry point invoked by `mde-popover
//! watermark`. It spawns the poll thread and parks the main thread
//! forever — no iced, no layer-shell surface. The dispatcher in
//! `main.rs` still routes `Kind::Watermark → watermark::run` for
//! source-compatibility with the sway autostart line in
//! `data/sway/config`.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Snapshot of every value the system-identity strip renders.
#[derive(Debug, Clone, Default)]
pub struct WatermarkState {
    pub mde_version: String,
    pub fedora_release: String,
    pub build_hash: Option<String>,
    /// UTC build date in `YYYY-MM-DD` form. `None` on dev checkouts
    /// where `/usr/share/mde/build-date` doesn't exist.
    pub build_date: Option<String>,
    pub hostname: String,
    pub pending_updates: u32,
}

impl WatermarkState {
    /// Best-effort load: reads each field from a stable source,
    /// falling back to an empty string on any error.
    #[must_use]
    pub fn load() -> Self {
        Self {
            mde_version: env!("CARGO_PKG_VERSION").to_string(),
            fedora_release: read_fedora_release(),
            build_hash: option_env!("MDE_BUILD_HASH")
                .map(str::to_owned)
                .or_else(read_build_file_for_hash),
            build_date: read_build_file_for_date(),
            hostname: read_hostname(),
            pending_updates: read_pending_update_count(),
        }
    }

    /// Always-visible system-identity segment — the part the footer
    /// shows even when there are zero pending updates. Mirrors the
    /// Win10 Settings → System → About "Windows specifications"
    /// shape.
    #[must_use]
    pub fn identity_line(&self) -> String {
        format!(
            "MDE {ver} · Fedora {release} · {host}",
            ver = self.mde_version,
            release = self.fedora_release,
            host = self.hostname,
        )
    }
}

fn read_fedora_release() -> String {
    read_os_release_field("VERSION_ID").unwrap_or_else(|| "44".to_string())
}

fn read_os_release_field(key: &str) -> Option<String> {
    let content = std::fs::read_to_string("/etc/os-release").ok()?;
    parse_os_release_field(&content, key)
}

/// Pure parser — pulls `KEY="value"` lines out of /etc/os-release
/// shape strings. Exposed for tests.
#[must_use]
pub fn parse_os_release_field(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{key}=")) {
            let trimmed = rest.trim().trim_matches('"');
            return Some(trimmed.to_string());
        }
    }
    None
}

fn read_hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "fedora".to_string())
}

fn read_pending_update_count() -> u32 {
    let cache_path = dirs::cache_dir()
        .map(|d| d.join("mde/dnf-updates.count"))
        .unwrap_or_default();
    parse_count_file(&cache_path)
}

fn read_build_file_for_hash() -> Option<String> {
    read_build_meta(&[
        "/usr/share/mde/build-hash",
        "/usr/share/mackes-shell/build-hash",
        "build-hash",
    ])
}

fn read_build_file_for_date() -> Option<String> {
    read_build_meta(&[
        "/usr/share/mde/build-date",
        "/usr/share/mackes-shell/build-date",
        "build-date",
    ])
}

/// Pure helper — walk the candidate paths and return the first non-
/// empty trimmed content. Exposed for tests.
#[must_use]
pub fn read_build_meta(candidates: &[&str]) -> Option<String> {
    for c in candidates {
        if let Ok(s) = std::fs::read_to_string(c) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

/// Pure helper — reads + parses the dnf-updates count file. Exposed
/// for tests.
#[must_use]
pub fn parse_count_file(path: &Path) -> u32 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

// ──────────────────────────────────────────────────────────────
// Headless poll daemon
// ──────────────────────────────────────────────────────────────

/// How often we re-poll dnf for the pending-update count. 4 hours
/// matches the v1.x cadence from the spec.
const DNF_POLL_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);

/// Binary entry point — spawns the dnf poll thread and parks the
/// main thread forever. Returns `iced_layershell::Result` so the
/// `main.rs` dispatcher's match arms stay one return type across
/// every popover Kind, but the type carries no semantic meaning
/// here (a headless daemon doesn't open a layer-shell surface).
pub fn run() -> iced_layershell::Result {
    let shared = Arc::new(Mutex::new(WatermarkState::load()));
    spawn_poll_thread(Arc::clone(&shared));
    tracing::info!("mde-popover watermark — headless poll daemon parked");
    std::thread::park();
    Ok(())
}

/// Background OS-thread driver — re-polls dnf every
/// `DNF_POLL_INTERVAL`, writes the resulting count to the shared
/// `WatermarkState` AND to the on-disk cache file the start-menu
/// reads on every open. Runs forever; detached.
fn spawn_poll_thread(state: Arc<Mutex<WatermarkState>>) {
    thread::Builder::new()
        .name("watermark-dnf-poll".into())
        .spawn(move || loop {
            let n = poll_dnf_check_update();
            if let Ok(mut s) = state.lock() {
                s.pending_updates = n;
            }
            cache_count(n);
            tracing::debug!(pending = n, "watermark dnf poll complete");
            thread::sleep(DNF_POLL_INTERVAL);
        })
        .expect("spawn watermark dnf poll thread");
}

/// Run `dnf check-update --quiet` and count the package lines.
/// dnf exits with status 100 when there ARE updates, 0 when none,
/// 1 on error. We count any non-empty, non-header line of stdout
/// as an update.
fn poll_dnf_check_update() -> u32 {
    let output = match std::process::Command::new("dnf")
        .args(["check-update", "--quiet"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(error = %e, "dnf check-update spawn failed");
            return 0;
        }
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| {
            let t = line.trim();
            !t.is_empty() && !t.starts_with("Obsoleting")
        })
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

/// Write the latest count to the cache file so consumers (the
/// start-menu footer today, anything else later) can read it
/// without spawning dnf themselves.
fn cache_count(n: u32) {
    let Some(dir) = dirs::cache_dir() else {
        return;
    };
    let dir = dir.join("mde");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = std::fs::write(dir.join("dnf-updates.count"), format!("{n}\n"));
}

/// Read the cached count from disk. Cheap (small file, ~3 bytes).
/// Returns 0 if the cache file doesn't exist yet (first-boot, the
/// poll thread hasn't completed its first run).
#[must_use]
pub fn current_pending_count() -> u32 {
    let Some(dir) = dirs::cache_dir() else {
        return 0;
    };
    parse_count_file(&dir.join("mde/dnf-updates.count"))
}

/// Spawn `pkexec dnf upgrade` detached so the calling popover (or
/// the start-menu) returns immediately. The polkit agent prompts on
/// the user's behalf; on accept, dnf runs in the background.
/// `wait()` in a background thread to avoid zombies.
pub fn spawn_pkexec_dnf_upgrade() {
    match std::process::Command::new("pkexec")
        .args(["dnf", "upgrade", "-y"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => {
            tracing::warn!(error = %e, "pkexec dnf upgrade spawn failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn parse_count_file_returns_value() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("count");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "42").unwrap();
        assert_eq!(parse_count_file(&path), 42);
    }

    #[test]
    fn parse_count_file_missing_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing");
        assert_eq!(parse_count_file(&path), 0);
    }

    #[test]
    fn parse_count_file_malformed_returns_zero() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("count");
        std::fs::write(&path, "abc\n").unwrap();
        assert_eq!(parse_count_file(&path), 0);
    }

    #[test]
    fn identity_line_excludes_count() {
        let state = WatermarkState {
            mde_version: "4.0.1".into(),
            fedora_release: "44".into(),
            hostname: "host".into(),
            pending_updates: 7,
            ..Default::default()
        };
        let line = state.identity_line();
        assert!(line.contains("MDE 4.0.1"));
        assert!(line.contains("Fedora 44"));
        assert!(line.contains("host"));
        // identity_line is always the short form; count lives in the
        // chip the footer renders alongside, not in this string.
        assert!(!line.contains("updates"));
    }

    #[test]
    fn parse_os_release_field_strips_quotes() {
        let content = "NAME=\"Fedora Linux\"\nVERSION_ID=44\n";
        assert_eq!(
            parse_os_release_field(content, "NAME"),
            Some("Fedora Linux".to_string())
        );
        assert_eq!(
            parse_os_release_field(content, "VERSION_ID"),
            Some("44".to_string())
        );
    }

    #[test]
    fn parse_os_release_field_missing_returns_none() {
        assert!(parse_os_release_field("NAME=x\n", "MISSING").is_none());
    }

    #[test]
    fn read_build_meta_uses_first_non_empty() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::write(&a, "  \n").unwrap();
        std::fs::write(&b, "abc123\n").unwrap();
        let a_str = a.to_string_lossy().to_string();
        let b_str = b.to_string_lossy().to_string();
        let result = read_build_meta(&[&a_str, &b_str]);
        assert_eq!(result.as_deref(), Some("abc123"));
    }
}
