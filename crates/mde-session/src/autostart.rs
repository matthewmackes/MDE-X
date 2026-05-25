//! v2.0.0 Phase D.3 — XDG autostart honoring.
//!
//! Reads every `.desktop` file under `$XDG_CONFIG_HOME/autostart/`
//! (and the system-wide `/etc/xdg/autostart/`), filters out the ones
//! with `Hidden=true` or `OnlyShowIn=` lists that exclude MDE, then
//! spawns each as a detached child process. mde-session calls this
//! once at startup; subsequent autostart additions need a relog.

use std::path::PathBuf;

/// Pure helper: parse a `.desktop` file body into a flat key→value
/// map for the `[Desktop Entry]` group. Subsequent groups are
/// ignored (autostart cares only about the default group).
#[must_use]
pub fn parse_desktop_entry(body: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut in_default = false;
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(group) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_default = group.eq_ignore_ascii_case("Desktop Entry");
            continue;
        }
        if !in_default {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.insert(k.trim().to_owned(), v.trim().to_owned());
        }
    }
    out
}

/// Pure helper: should this `.desktop` entry actually launch under
/// MDE? Honors `Hidden=true` + `OnlyShowIn=` + `NotShowIn=`.
#[must_use]
pub fn should_launch(entry: &std::collections::HashMap<String, String>) -> bool {
    if entry
        .get("Hidden")
        .map(|s| s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return false;
    }
    if let Some(only) = entry.get("OnlyShowIn") {
        let parts: Vec<&str> = only.split(';').map(str::trim).collect();
        if !parts.iter().any(|p| p.eq_ignore_ascii_case("MDE")) {
            return false;
        }
    }
    if let Some(not) = entry.get("NotShowIn") {
        if not.split(';').any(|p| p.trim().eq_ignore_ascii_case("MDE")) {
            return false;
        }
    }
    entry.contains_key("Exec")
}

/// Resolve the user + system autostart directories. User dir
/// honors `$XDG_CONFIG_HOME`.
#[must_use]
pub fn autostart_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let user = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map_or_else(
            || dirs::home_dir().unwrap_or_default().join(".config"),
            PathBuf::from,
        );
    out.push(user.join("autostart"));
    out.push(PathBuf::from("/etc/xdg/autostart"));
    out
}

/// Collect every parsed entry from every autostart dir, keyed by
/// `.desktop` ID (so user entries shadow system-wide ones).
fn collect_entries() -> std::collections::HashMap<String, std::collections::HashMap<String, String>>
{
    let mut by_id: std::collections::HashMap<String, std::collections::HashMap<String, String>> =
        std::collections::HashMap::new();
    // Iterate system-wide first so user entries overwrite.
    for dir in autostart_dirs().into_iter().rev() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            by_id.insert(id.to_owned(), parse_desktop_entry(&text));
        }
    }
    by_id
}

/// Spawn every autostart entry that passes [`should_launch`]. Errors
/// are logged + skipped — one bad launcher mustn't kill the rest.
pub async fn launch_user_autostart() {
    let entries = collect_entries();
    for (id, entry) in entries {
        if !should_launch(&entry) {
            tracing::debug!("autostart: skipping {id}");
            continue;
        }
        let Some(exec) = entry.get("Exec") else {
            continue;
        };
        // Strip the `%U` / `%F` / `%i` field codes per the XDG spec.
        let cleaned = strip_exec_field_codes(exec);
        tracing::info!("autostart: launching {id} ({cleaned})");
        let _ = tokio::process::Command::new("sh")
            .args(["-c", &cleaned])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

/// Pure helper: strip the XDG Exec field codes (`%U`, `%F`, `%i`,
/// `%c`, `%k`, etc.) from an Exec command string.
#[must_use]
pub fn strip_exec_field_codes(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            chars.next(); // discard the field-code character (%U, %F, …)
        } else {
            out.push(c);
        }
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_desktop_entry_reads_default_group_only() {
        let body =
            "[Desktop Entry]\nName=Foo\nExec=foo --bar\nHidden=false\n\n[Action one]\nName=Other";
        let e = parse_desktop_entry(body);
        assert_eq!(e.get("Name").map(String::as_str), Some("Foo"));
        assert_eq!(e.get("Exec").map(String::as_str), Some("foo --bar"));
        assert_eq!(e.get("Hidden").map(String::as_str), Some("false"));
    }

    #[test]
    fn parse_desktop_entry_skips_comments_and_blank_lines() {
        let body = "# leading comment\n\n[Desktop Entry]\n# inner\nName=X\n";
        let e = parse_desktop_entry(body);
        assert_eq!(e.get("Name").map(String::as_str), Some("X"));
        assert!(!e.contains_key("# leading comment"));
    }

    #[test]
    fn should_launch_rejects_hidden_true() {
        let mut e = std::collections::HashMap::new();
        e.insert("Hidden".into(), "true".into());
        e.insert("Exec".into(), "x".into());
        assert!(!should_launch(&e));
    }

    #[test]
    fn should_launch_honors_only_show_in() {
        let mut e = std::collections::HashMap::new();
        e.insert("Exec".into(), "x".into());
        e.insert("OnlyShowIn".into(), "GNOME;KDE".into());
        assert!(!should_launch(&e), "MDE missing from OnlyShowIn -> skip");
        e.insert("OnlyShowIn".into(), "GNOME;MDE".into());
        assert!(should_launch(&e), "MDE present -> launch");
    }

    #[test]
    fn should_launch_honors_not_show_in() {
        let mut e = std::collections::HashMap::new();
        e.insert("Exec".into(), "x".into());
        e.insert("NotShowIn".into(), "MDE".into());
        assert!(!should_launch(&e));
    }

    #[test]
    fn should_launch_requires_exec_key() {
        let e = std::collections::HashMap::new();
        assert!(!should_launch(&e));
    }

    #[test]
    fn should_launch_accepts_minimal_entry() {
        let mut e = std::collections::HashMap::new();
        e.insert("Exec".into(), "/usr/bin/something".into());
        assert!(should_launch(&e));
    }

    #[test]
    fn strip_exec_field_codes_drops_known_codes() {
        assert_eq!(strip_exec_field_codes("firefox %U"), "firefox");
        assert_eq!(
            strip_exec_field_codes("gedit %F file.txt"),
            "gedit  file.txt"
        );
        assert_eq!(strip_exec_field_codes("plain"), "plain");
    }
}
