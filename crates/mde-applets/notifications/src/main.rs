//! Binary entry point for the notifications-center modal
//! reader applet. Reads `~/.cache/mackes/notifications.json`
//! + writes the formatted row strip to the host over stdio.
//! `--manifest` prints the applet's manifest JSON and exits
//! (called by the panel host during registration discovery).

use std::io::{BufRead, BufReader, Write};
use std::process::ExitCode;

use mde_applet_api::HostMessage;
use mde_applet_notifications::{
    format_center, group_and_sort, handle_host, manifest, notifications_cache_path,
    parse_notifications, visible,
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
                eprintln!("mde-applet-notifications: serialize manifest: {e}");
                ExitCode::FAILURE
            }
        }
    } else if argv.iter().any(|a| a == "--now") {
        println!("{}", current_center());
        ExitCode::SUCCESS
    } else {
        run_loop()
    }
}

fn current_center() -> String {
    let raw = std::fs::read_to_string(notifications_cache_path()).unwrap_or_default();
    let rows = visible(parse_notifications(&raw));
    let groups = group_and_sort(rows);
    format_center(&groups)
}

fn run_loop() -> ExitCode {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let reader = BufReader::new(stdin.lock());
    let _ = writeln!(stdout, "{}", current_center());
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
            let _ = writeln!(stdout, "{}", current_center());
            let _ = stdout.flush();
        }
    }
    ExitCode::SUCCESS
}
