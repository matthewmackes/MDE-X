//! `mde-musicd` binary — AIR-4 slice.
//!
//! The full daemon (D-Bus + MPRIS + PipeWire) lands in AIR-2/5/6; this
//! entry point ships the `ping` subcommand that loads the mesh-shared
//! creds + reaches the Airsonic server, exercising the [`airsonic`] +
//! [`creds`] modules end-to-end (their §0.12 runtime reachability).

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use mde_musicd::airsonic::Client;
use mde_musicd::creds;

#[derive(Parser)]
#[command(name = "mde-musicd", about = "MDE native Airsonic music daemon.")]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Load the mesh-shared creds + reach the Airsonic server, printing
    /// its reported API version. Exits non-zero when creds are missing
    /// or the server is unreachable.
    Ping,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match args.cmd {
        Cmd::Ping => ping(),
    }
}

fn ping() -> ExitCode {
    let creds = match creds::load() {
        Ok(c) => c,
        Err(e) => {
            // The Missing case already carries the first-run hint.
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let client = Client::new(&creds.server_url, &creds.username, &creds.password);
    // Drive the async ping on a small runtime — the daemon proper will
    // host a long-lived runtime (AIR-2).
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("mde-musicd: runtime build failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(client.ping()) {
        Ok(version) => {
            println!("airsonic {}: reachable (API v{version})", creds.server_url);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("mde-musicd: {e}");
            ExitCode::from(3)
        }
    }
}
