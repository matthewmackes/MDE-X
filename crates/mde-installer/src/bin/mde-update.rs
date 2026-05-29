//! `mde-update` — fleet upgrade coordination (INST-10) + local version
//! report.
//!
//! `mde-update --coordinate <version>` writes a GlusterFS upgrade-intent
//! barrier file every peer's mackesd polls. With no flag it reports this
//! node's installed `mde-core` version.
//!
//! The cross-peer version table (INST-9) is **not** here: it needs
//! mackesd per-peer version tracking (INST-PEERVER), which does not
//! exist yet. Rather than print a fake table, the no-flag path reports
//! the local version and says where the cross-peer view will come from.

use std::process::{Command, ExitCode};

use clap::Parser;
use mde_installer::intent_file::{self, UpgradeIntent};

#[derive(Parser, Debug)]
#[command(
    name = "mde-update",
    about = "Coordinate a fleet-wide MDE upgrade, or report the local version."
)]
struct Args {
    /// Start a fleet upgrade barrier for the given target version.
    #[arg(long, value_name = "VERSION")]
    coordinate: Option<String>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("mde-update: {msg}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    if let Some(version) = &args.coordinate {
        return coordinate(version);
    }
    report_local()
}

fn coordinate(version: &str) -> Result<(), String> {
    let dir = intent_file::intent_dir(&intent_file::default_mesh_home());
    let intent = UpgradeIntent::new(version, hostname());
    let path = intent_file::write_intent(&dir, &intent)
        .map_err(|e| format!("writing upgrade-intent file: {e}"))?;
    println!("upgrade barrier started for {version}");
    println!("intent file: {}", path.display());
    println!("peers' mackesd will pick this up on their next poll and mark themselves ready.");
    Ok(())
}

fn report_local() -> Result<(), String> {
    let version = local_version().unwrap_or_else(|| "unknown".to_string());
    println!("{:<20} {}", hostname(), version);
    println!(
        "\n(cross-peer version table requires mackesd peer-version tracking — \
         INST-PEERVER, not yet shipped. Use `mde-update --coordinate <version>` \
         to start a fleet upgrade.)"
    );
    Ok(())
}

fn local_version() -> Option<String> {
    let out = Command::new("rpm")
        .args(["-q", "--qf", "%{VERSION}", "mde-core"])
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn hostname() -> String {
    Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "localhost".to_string())
}
