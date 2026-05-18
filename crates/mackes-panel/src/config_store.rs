// Config-store API used by main + Phase 2.3 (inotify reload). Some entry
// points haven't been wired in yet.
#![allow(dead_code)]

//! On-disk persistence for `~/.config/mackes-panel/panel.toml`.
//!
//! Per Q18 the panel config lives in TOML under `XDG_CONFIG_HOME`. This
//! module is the single read/write boundary for that file:
//!
//! * `path()` resolves the canonical location (`XDG_CONFIG_HOME` with a
//!   `$HOME/.config/mackes-panel/panel.toml` fallback).
//! * `load_or_default()` parses the file if present, else writes the
//!   `default_config()` and returns it. First-launch behavior per
//!   Phase 2.2 of `docs/PROJECT_WORKLIST.md`.
//! * `watch()` attaches a `gio::FileMonitor` (inotify-backed on Linux)
//!   and calls the supplied callback every time the file changes. Phase
//!   2.3 of `docs/PROJECT_WORKLIST.md`.

use std::path::{Path, PathBuf};

use gio::prelude::*;
use mackes_config::{default_config, parse, to_toml_string, PanelConfig};

const REL_PATH: &str = "mackes-panel/panel.toml";

/// Canonical config file path. Reads `XDG_CONFIG_HOME` first; falls back
/// to `$HOME/.config`. Returns `None` only when neither variable nor
/// `$HOME` is set (extremely unusual — bare /bin/sh sessions).
#[must_use]
pub fn path() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join(REL_PATH));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config").join(REL_PATH))
}

/// Load the panel config from `path()` if it exists; otherwise write the
/// default and return it. Bad TOML is logged to stderr and falls back to
/// defaults so the panel always starts in a usable state.
#[must_use]
pub fn load_or_default() -> PanelConfig {
    let Some(p) = path() else {
        return default_config();
    };
    if p.is_file() {
        match std::fs::read_to_string(&p) {
            Ok(text) => match parse(&text) {
                Ok(cfg) => return cfg,
                Err(e) => {
                    eprintln!("mackes-panel: ignoring malformed {}: {e}", p.display());
                }
            },
            Err(e) => {
                eprintln!("mackes-panel: cannot read {}: {e}", p.display());
            }
        }
    } else {
        write_default(&p);
    }
    default_config()
}

/// Attach a `gio::FileMonitor` to `panel.toml` and invoke `on_change`
/// each time the file changes on disk. The monitor is returned so the
/// caller keeps it alive — dropping it cancels the watch.
///
/// On Linux `gio` is inotify-backed, so this is essentially the same
/// signal path as direct `inotify_init1()` but plays nice with the GTK
/// main loop and the per-platform watcher implementations.
///
/// `on_change` receives the freshly-parsed `PanelConfig`, or `None` if
/// the file went away / failed to parse. Phase 2.3 keeps the apply step
/// minimal — Phase 2.5+ will diff the new config against the prior and
/// re-render only the changed slots.
#[must_use]
pub fn watch<F>(on_change: F) -> Option<gio::FileMonitor>
where
    F: Fn(Option<PanelConfig>) + 'static,
{
    let p = path()?;
    let file = gio::File::for_path(&p);
    let monitor = file
        .monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        .ok()?;
    monitor.connect_changed(move |_, _, _, event| {
        // We only care about content settling — CHANGES_DONE_HINT fires
        // once per logical save (atomic editors emit CREATED + DELETED +
        // CHANGES_DONE_HINT in sequence; we'd reload three times if we
        // listened to all of them).
        if event == gio::FileMonitorEvent::ChangesDoneHint
            || event == gio::FileMonitorEvent::Created
        {
            on_change(reload());
        }
    });
    Some(monitor)
}

/// Re-parse `panel.toml` after a watcher signal. Same fallback rules as
/// `load_or_default` but never writes — the file either exists (we're
/// reacting to a change) or it's been deleted (we surface `None`).
fn reload() -> Option<PanelConfig> {
    let p = path()?;
    let text = std::fs::read_to_string(&p).ok()?;
    parse(&text).ok()
}

fn write_default(path: &Path) {
    let cfg = default_config();
    let text = match to_toml_string(&cfg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("mackes-panel: cannot serialize default config: {e}");
            return;
        }
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("mackes-panel: cannot create {}: {e}", parent.display());
            return;
        }
    }
    if let Err(e) = std::fs::write(path, text) {
        eprintln!(
            "mackes-panel: cannot write default to {}: {e}",
            path.display()
        );
    } else {
        eprintln!("mackes-panel: wrote default config to {}", path.display());
    }
}
