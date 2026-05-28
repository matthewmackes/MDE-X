//! Binary entry point for the Super+Space centered launcher
//! applet (Spotlight-style). Walks the desktop-entry index +
//! XBEL recents + supports inline math evaluation; writes the
//! ranked hit list to the host over stdio. `--manifest` prints
//! the applet's manifest JSON and exits (called by the panel
//! host during registration discovery).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mde_applet_api::HostMessage;
use mde_applet_apple_menu::{
    build_hits, format_hits, handle_host, manifest, parse_app_row, AppRow,
};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a == "--manifest") {
        match serde_json::to_string_pretty(&manifest()) {
            Ok(j) => {
                println!("{j}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("mde-applet-apple-menu: serialize manifest: {e}");
                ExitCode::FAILURE
            }
        }
    } else if argv.iter().any(|a| a == "--now") {
        // No query in --now mode — show "(no matches)"
        // to mirror the contract of the other applets.
        println!("{}", current_snapshot(""));
        ExitCode::SUCCESS
    } else {
        run_loop()
    }
}

fn current_snapshot(query: &str) -> String {
    let apps = load_apps();
    let hits = build_hits(&apps, query);
    format_hits(&hits)
}

fn load_apps() -> Vec<AppRow> {
    let mut out = Vec::new();
    for dir in application_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }
            let Some(base) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            out.push(parse_app_row(&base, &raw));
        }
    }
    out
}

fn application_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        out.push(Path::new(&home).join(".local/share/applications"));
    }
    let xdg =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for component in xdg.split(':') {
        if component.is_empty() {
            continue;
        }
        out.push(Path::new(component).join("applications"));
    }
    out
}

fn run_loop() -> ExitCode {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let reader = BufReader::new(stdin.lock());
    let _ = writeln!(stdout, "{}", current_snapshot(""));
    let _ = stdout.flush();
    for line in reader.lines() {
        let Ok(line) = line else {
            return ExitCode::from(2);
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<HostMessage>(&line) else {
            return ExitCode::from(2);
        };
        if matches!(msg, HostMessage::Shutdown) {
            return ExitCode::SUCCESS;
        }
        if handle_host(&msg) {
            let _ = writeln!(stdout, "{}", current_snapshot(""));
            let _ = stdout.flush();
        }
    }
    ExitCode::SUCCESS
}
