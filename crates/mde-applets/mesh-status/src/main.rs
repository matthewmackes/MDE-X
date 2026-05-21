//! `mde-applet-mesh-status` binary entry — Phase E1.2.4.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, ExitCode};

use mde_applet_api::HostMessage;
use mde_applet_mesh_status::{format_chip, handle_host, manifest, parse_healthz};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a == "--manifest") {
        match serde_json::to_string_pretty(&manifest()) {
            Ok(j) => {
                println!("{j}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("mde-applet-mesh-status: serialize manifest: {e}");
                ExitCode::FAILURE
            }
        }
    } else if argv.iter().any(|a| a == "--now") {
        println!("{}", current_chip());
        ExitCode::SUCCESS
    } else {
        run_loop()
    }
}

fn current_chip() -> String {
    let raw = run_mded_healthz();
    format_chip(&parse_healthz(&raw))
}

/// Shell out to `mded healthz`. Empty string on any failure
/// — the parser produces an `unknown` / 0 report from empty.
fn run_mded_healthz() -> String {
    let Ok(output) = Command::new("mded").arg("healthz").output() else {
        return String::new();
    };
    if !output.status.success() {
        return String::new();
    }
    String::from_utf8(output.stdout).unwrap_or_default()
}

fn run_loop() -> ExitCode {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let reader = BufReader::new(stdin.lock());
    let _ = writeln!(stdout, "{}", current_chip());
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
            let _ = writeln!(stdout, "{}", current_chip());
            let _ = stdout.flush();
        }
    }
    ExitCode::SUCCESS
}
