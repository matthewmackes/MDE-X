//! Local MDE-state wipe (config-path scope) + service control + the
//! installed-profile marker.
//!
//! **Scope note (§0.12):** this is the clean-install sequence — stop
//! services, remove MDE local state, write the profile marker, restart
//! services, then birthrights run. It does **not** revoke the Nebula
//! cert or tear down the GlusterFS brick (the re-install half of
//! INST-7): those need a mackesd `Ca.Revoke` method that does not exist
//! yet (tracked as INST-3b / INST-PEERVER-adjacent). On a clean Fedora
//! Server build-up there is no cert or brick, so this is the complete
//! path for the canonical install.

use std::path::PathBuf;
use std::process::Command;

use crate::profile::Profile;

/// Services stopped before the wipe and re-started after (best-effort).
pub const MANAGED_SERVICES: &[&str] = &["mackesd", "nebula", "glusterd", "netdata"];

/// The installed-profile marker file.
pub const PROFILE_MARKER: &str = "/var/lib/mde/installed-profile";

/// The MDE local-state paths a clean install removes. Per-user paths
/// resolve against `$HOME`; system paths are absolute.
#[must_use]
pub fn local_state_paths() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let h = PathBuf::from(home);
    vec![
        h.join(".config/mde"),
        h.join(".local/share/mde"),
        h.join(".cache/mde"),
        PathBuf::from("/etc/mde"),
        PathBuf::from("/var/lib/mde"),
    ]
}

/// Subset of `paths` that currently exist — the preflight "will be
/// wiped" list shown before the typed-`NUKE` confirm.
#[must_use]
pub fn existing(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().filter(|p| p.exists()).cloned().collect()
}

/// Remove each path in `paths` that exists. Per-path result is logged
/// into the returned action lines; a missing path or a failed removal
/// does not abort the rest (the install is idempotent on re-run).
#[must_use]
pub fn wipe_paths(paths: &[PathBuf]) -> Vec<String> {
    let mut log = Vec::new();
    for p in paths {
        if !p.exists() {
            log.push(format!("skip (absent): {}", p.display()));
            continue;
        }
        match std::fs::remove_dir_all(p) {
            Ok(()) => log.push(format!("removed: {}", p.display())),
            Err(e) => log.push(format!("FAILED to remove {}: {e}", p.display())),
        }
    }
    log
}

/// `systemctl stop` each unit (best-effort; failures are logged, not fatal).
#[must_use]
pub fn stop_services(units: &[&str]) -> Vec<String> {
    run_systemctl("stop", units)
}

/// `systemctl enable --now` each unit (best-effort).
#[must_use]
pub fn start_services(units: &[&str]) -> Vec<String> {
    let mut log = Vec::new();
    for u in units {
        log.push(systemctl(&["enable", "--now", u]));
    }
    log
}

/// Write the installed-profile marker (`/var/lib/mde/installed-profile`).
///
/// # Errors
/// IO failures creating the parent dir or writing the file.
pub fn write_profile_marker(profile: Profile) -> std::io::Result<()> {
    let path = PathBuf::from(PROFILE_MARKER);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, format!("{profile}\n"))
}

fn run_systemctl(verb: &str, units: &[&str]) -> Vec<String> {
    units.iter().map(|u| systemctl(&[verb, u])).collect()
}

fn systemctl(args: &[&str]) -> String {
    match Command::new("systemctl").args(args).output() {
        Ok(out) if out.status.success() => format!("systemctl {}: ok", args.join(" ")),
        Ok(out) => format!(
            "systemctl {}: exit {} ({})",
            args.join(" "),
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => format!("systemctl {}: spawn failed: {e}", args.join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn existing_filters_to_present_paths() {
        let dir = tempdir().unwrap();
        let present = dir.path().join("here");
        fs::create_dir(&present).unwrap();
        let absent = dir.path().join("gone");
        let got = existing(&[present.clone(), absent]);
        assert_eq!(got, vec![present]);
    }

    #[test]
    fn wipe_removes_present_and_skips_absent() {
        let dir = tempdir().unwrap();
        let present = dir.path().join("state");
        fs::create_dir_all(present.join("nested")).unwrap();
        let absent = dir.path().join("never");
        let log = wipe_paths(&[present.clone(), absent.clone()]);
        assert!(!present.exists());
        assert!(log.iter().any(|l| l.starts_with("removed:")));
        assert!(log.iter().any(|l| l.contains("skip (absent)")));
    }

    #[test]
    fn local_state_paths_are_the_locked_five() {
        std::env::set_var("HOME", "/home/tester");
        let paths = local_state_paths();
        assert_eq!(paths.len(), 5);
        assert!(paths.contains(&PathBuf::from("/home/tester/.config/mde")));
        assert!(paths.contains(&PathBuf::from("/etc/mde")));
        assert!(paths.contains(&PathBuf::from("/var/lib/mde")));
    }
}
