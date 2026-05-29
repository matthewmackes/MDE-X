//! `mde-update` — fleet version-skew report (INST-9) + upgrade
//! coordination (INST-10).
//!
//! With no flag it prints the converged HOSTNAME/VERSION/LAST-SEEN
//! table built by unioning the GFS-replicated `<mesh-home>/peers/`
//! dir (PEERVER-3), marking minor `(!)` / major `(!!)` version skew
//! against this node, and stale peers. `--coordinate <version>` writes
//! a GlusterFS upgrade-intent barrier file every peer's mackesd polls.

use std::process::ExitCode;

use clap::Parser;
use mde_installer::intent_file::{self, UpgradeIntent};
use mde_installer::peers::{self, Skew, STALE_THRESHOLD_MS};

#[derive(Parser, Debug)]
#[command(
    name = "mde-update",
    about = "Report fleet version skew, or coordinate a fleet-wide MDE upgrade."
)]
struct Args {
    /// Start a fleet upgrade barrier for the given target version.
    #[arg(long, value_name = "VERSION")]
    coordinate: Option<String>,

    /// Emit the peer table as a JSON array for scripted consumption.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    if let Some(version) = &args.coordinate {
        return match coordinate(version) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                eprintln!("mde-update: {msg}");
                ExitCode::from(2)
            }
        };
    }
    report(args.json)
}

fn coordinate(version: &str) -> Result<(), String> {
    let dir = intent_file::intent_dir(&intent_file::default_mesh_home());
    let intent = UpgradeIntent::new(version, peers::local_hostname());
    let path = intent_file::write_intent(&dir, &intent)
        .map_err(|e| format!("writing upgrade-intent file: {e}"))?;
    println!("upgrade barrier started for {version}");
    println!("intent file: {}", path.display());
    println!("peers' mackesd will pick this up on their next poll and mark themselves ready.");
    Ok(())
}

/// Report the fleet table. Exit codes (for scripted gating, per INST-9):
/// 0 = all versions match, 1 = minor skew, 2 = major skew.
fn report(json: bool) -> ExitCode {
    let local_host = peers::local_hostname();
    let peer_list = peers::list_peers();
    let local_version = peer_list
        .iter()
        .find(|p| p.hostname == local_host)
        .and_then(|p| p.mde_version.clone())
        .or_else(peers::local_mde_version);

    // Worst skew drives the exit code; compute it in both output modes.
    let skew_of = |p: &peers::PeerRecord| -> Skew {
        if p.hostname == local_host {
            Skew::Match
        } else {
            peers::classify_skew(local_version.as_deref(), p.mde_version.as_deref())
        }
    };
    let worst = peer_list.iter().map(&skew_of).fold(Skew::Match, worse);

    if json {
        print_json(&peer_list, local_version.as_deref(), &local_host);
    } else {
        println!("{:<20} {:<12} {:<12} {}", "HOSTNAME", "VERSION", "LAST SEEN", "");
        for p in &peer_list {
            let stale = p.is_stale(STALE_THRESHOLD_MS);
            let ver = p.mde_version.clone().unwrap_or_else(|| "unknown".into());
            let seen = if stale {
                format!("{} STALE", human_age(p.age_ms()))
            } else {
                human_age(p.age_ms())
            };
            println!("{:<20} {:<12} {:<12} {}", p.hostname, ver, seen, skew_of(p).marker());
        }
        match worst {
            Skew::Minor => println!(
                "\n-- {} peer(s) on a different version.",
                count_skew(&peer_list, &local_host, local_version.as_deref())
            ),
            Skew::Major => println!("\n!! major version skew in the fleet — coordinate an upgrade."),
            Skew::Match | Skew::Unknown => {}
        }
    }

    match worst {
        Skew::Major => ExitCode::from(2),
        Skew::Minor => ExitCode::from(1),
        _ => ExitCode::SUCCESS,
    }
}

fn print_json(peer_list: &[peers::PeerRecord], local: Option<&str>, local_host: &str) {
    let rows: Vec<serde_json::Value> = peer_list
        .iter()
        .map(|p| {
            let skew = if p.hostname == local_host {
                Skew::Match
            } else {
                peers::classify_skew(local, p.mde_version.as_deref())
            };
            serde_json::json!({
                "hostname": p.hostname,
                "version": p.mde_version,
                "last_seen_ms": p.last_seen_ms,
                "age_ms": p.age_ms(),
                "stale": p.is_stale(STALE_THRESHOLD_MS),
                "status": match skew {
                    Skew::Match => "match",
                    Skew::Minor => "minor-skew",
                    Skew::Major => "major-skew",
                    Skew::Unknown => "unknown",
                },
            })
        })
        .collect();
    match serde_json::to_string_pretty(&rows) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("mde-update: json encode failed: {e}"),
    }
}

fn count_skew(peer_list: &[peers::PeerRecord], local_host: &str, local: Option<&str>) -> usize {
    peer_list
        .iter()
        .filter(|p| p.hostname != local_host)
        .filter(|p| !matches!(peers::classify_skew(local, p.mde_version.as_deref()), Skew::Match))
        .count()
}

const fn rank(s: Skew) -> u8 {
    match s {
        Skew::Match => 0,
        Skew::Unknown => 1,
        Skew::Minor => 2,
        Skew::Major => 3,
    }
}

fn worse(a: Skew, b: Skew) -> Skew {
    if rank(b) > rank(a) {
        b
    } else {
        a
    }
}

fn human_age(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 90 {
        format!("{secs}s ago")
    } else if secs < 5400 {
        format!("{}m ago", secs / 60)
    } else if secs < 172_800 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}
