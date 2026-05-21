//! `mde-applet-network` binary entry — Phase E1.2.3.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, ExitCode};

use mde_applet_api::HostMessage;
use mde_applet_network::{format_chip, handle_host, manifest, parse_active};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a == "--manifest") {
        match serde_json::to_string_pretty(&manifest()) {
            Ok(j) => {
                println!("{j}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("mde-applet-network: serialize manifest: {e}");
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
    let raw = run_nmcli_active();
    let parsed = parse_active(&raw);
    format_chip(parsed.as_ref())
}

fn run_nmcli_active() -> String {
    let Ok(output) = Command::new("nmcli")
        .args([
            "-t",
            "-f",
            "NAME,TYPE,DEVICE,STATE",
            "connection",
            "show",
            "--active",
        ])
        .output()
    else {
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
