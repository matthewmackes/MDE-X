// Window-enumeration API consumed by AppModule state updates.
#![allow(dead_code)]

//! Open-window snapshot via the `wmctrl` CLI.
//!
//! libwnck has no maintained safe Rust binding on crates.io (only the
//! `wnck-sys` raw FFI shim), so for Phase 5.2 we delegate to the
//! `wmctrl` command-line tool, which every XFCE install already has
//! and which prints exactly the data we need:
//!
//!   $ wmctrl -lp
//!   0x03800001  0 1234  hostname  Firefox — Inbox (123) - Gmail
//!   0x03c00003  0 5678  hostname  vim ~/notes.md
//!
//! Columns: window id, desktop, pid, host, title (free-form). For
//! Phase 5.2 we only care about title + pid; the panel maps title or
//! pid back to a `.desktop` Name via the
//! `desktop_files::scan()` index in `dock::DockModule::state()`.
//!
//! `wmctrl` is added to the RPM Requires so it's always available on
//! Mackes installs.

use std::process::Command;

/// One open top-level window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWindow {
    /// X11 window id (e.g. `0x03800001`). Stable while the window exists.
    pub window_id: String,
    /// Owning process PID. Maps to `/proc/<pid>/comm` for Exec matching.
    pub pid: u32,
    /// Free-form title shown in window decorations.
    pub title: String,
}

/// Run `wmctrl -lp` and parse its output. Returns an empty Vec if
/// wmctrl isn't installed, the X server can't be reached, or the
/// command otherwise errors — every call site is best-effort.
#[must_use]
pub fn list_open_windows() -> Vec<OpenWindow> {
    let Ok(output) = Command::new("wmctrl").arg("-lp").output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_wmctrl(&String::from_utf8_lossy(&output.stdout))
}

/// Pure-text parser for `wmctrl -lp` output, exposed for unit tests.
pub fn parse_wmctrl(text: &str) -> Vec<OpenWindow> {
    let mut out = Vec::new();
    for line in text.lines() {
        // Columns are whitespace-separated, BUT title may include
        // arbitrary whitespace. Split into at-most 5 fields so the
        // last one keeps internal spaces.
        let mut parts = line.splitn(5, char::is_whitespace);
        let Some(window_id) = parts.next() else {
            continue;
        };
        // After the id, the desktop column may be followed by extra
        // spaces. Use a second splitn to skip a single whitespace
        // run at a time.
        let after_id: String = parts.collect::<Vec<&str>>().join(" ");
        let mut tokens = after_id.split_whitespace();
        let Some(_desktop) = tokens.next() else {
            continue;
        };
        let Some(pid_token) = tokens.next() else {
            continue;
        };
        let Ok(pid) = pid_token.parse::<u32>() else {
            continue;
        };
        let Some(_host) = tokens.next() else {
            continue;
        };
        let title = tokens.collect::<Vec<&str>>().join(" ");
        if window_id.is_empty() {
            continue;
        }
        out.push(OpenWindow {
            window_id: window_id.to_owned(),
            pid,
            title,
        });
    }
    out
}

/// Identify whether any open window appears to belong to the
/// `.desktop` entry. Matches on the entry's `Name`, the `Exec`'s
/// leading command basename, or `/proc/<pid>/comm` against the same
/// basename. Used by `AppModule::state()` to flip Idle → Running.
#[must_use]
pub fn app_is_running(desktop_name: &str, exec: &str, windows: &[OpenWindow]) -> bool {
    let needle_name = desktop_name.to_ascii_lowercase();
    let needle_cmd = exec_basename(exec).to_ascii_lowercase();
    for w in windows {
        if w.title.to_ascii_lowercase().contains(&needle_name) {
            return true;
        }
        if w.title.to_ascii_lowercase().contains(&needle_cmd) {
            return true;
        }
        if pid_command_matches(w.pid, &needle_cmd) {
            return true;
        }
    }
    false
}

fn exec_basename(exec: &str) -> &str {
    exec.split_whitespace()
        .next()
        .map_or(exec, |first| first.rsplit('/').next().unwrap_or(first))
}

fn pid_command_matches(pid: u32, needle: &str) -> bool {
    let path = format!("/proc/{pid}/comm");
    std::fs::read_to_string(&path)
        .is_ok_and(|text| text.trim().to_ascii_lowercase().contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typical_wmctrl_output() {
        let text = "\
0x03800001  0 1234  fedora Firefox — Inbox\n\
0x03c00003  0 5678  fedora vim ~/notes.md\n";
        let v = parse_wmctrl(text);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].window_id, "0x03800001");
        assert_eq!(v[0].pid, 1234);
        assert!(v[0].title.starts_with("Firefox"));
        assert_eq!(v[1].pid, 5678);
        assert!(v[1].title.starts_with("vim"));
    }

    #[test]
    fn skips_malformed_lines() {
        let text = "\
not-a-window\n\
0xdeadbeef  0 999  host  ok\n\
0xdead  bogus pid line\n";
        let v = parse_wmctrl(text);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].window_id, "0xdeadbeef");
    }

    #[test]
    fn app_is_running_matches_by_name() {
        let w = vec![OpenWindow {
            window_id: "0x1".into(),
            pid: 1,
            title: "Firefox — Inbox".into(),
        }];
        assert!(app_is_running("Firefox", "firefox %U", &w));
    }

    #[test]
    fn app_is_running_matches_by_exec_basename() {
        let w = vec![OpenWindow {
            window_id: "0x1".into(),
            pid: 1,
            title: "vim ~/notes.md".into(),
        }];
        assert!(app_is_running("Vim", "/usr/bin/vim", &w));
    }

    #[test]
    fn app_is_running_returns_false_when_no_match() {
        let w = vec![OpenWindow {
            window_id: "0x1".into(),
            pid: 1,
            title: "Thunar".into(),
        }];
        assert!(!app_is_running("Firefox", "firefox", &w));
    }

    #[test]
    fn exec_basename_strips_path_and_args() {
        assert_eq!(exec_basename("/usr/bin/firefox %U"), "firefox");
        assert_eq!(exec_basename("firefox"), "firefox");
        assert_eq!(exec_basename(""), "");
    }
}
