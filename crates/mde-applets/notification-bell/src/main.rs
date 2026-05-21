//! `mde-applet-notification-bell` binary entry —
//! Phase E1.2.5.

use std::io::{BufRead, BufReader, Write};
use std::process::ExitCode;

use mde_applet_api::HostMessage;
use mde_applet_notification_bell::{
    count_unread, format_badge, handle_host, manifest, notifications_cache_path,
    parse_notifications,
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
                eprintln!("mde-applet-notification-bell: serialize manifest: {e}");
                ExitCode::FAILURE
            }
        }
    } else if argv.iter().any(|a| a == "--count") {
        println!("{}", current_badge());
        ExitCode::SUCCESS
    } else {
        run_loop()
    }
}

fn current_badge() -> String {
    let path = notifications_cache_path();
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    let rows = parse_notifications(&raw);
    format_badge(count_unread(&rows))
}

fn run_loop() -> ExitCode {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let reader = BufReader::new(stdin.lock());
    let _ = writeln!(stdout, "{}", current_badge());
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
            let _ = writeln!(stdout, "{}", current_badge());
            let _ = stdout.flush();
        }
    }
    ExitCode::SUCCESS
}
