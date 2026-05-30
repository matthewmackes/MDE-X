//! `mde-musicd` binary — AIR-4 slice.
//!
//! The full daemon (D-Bus + MPRIS + PipeWire) lands in AIR-2/5/6; this
//! entry point ships the `ping` subcommand that loads the mesh-shared
//! creds + reaches the Airsonic server, exercising the [`airsonic`] +
//! [`creds`] modules end-to-end (their §0.12 runtime reachability).

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use mde_musicd::airsonic::Client;
use mde_musicd::{cache, creds, state};

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
    /// Inspect or trim the mesh-shared audio cache (AIR-7).
    Cache {
        #[command(subcommand)]
        op: CacheOp,
    },
    /// Inspect mesh playback state or request a take-over (AIR-8).
    State {
        #[command(subcommand)]
        op: StateOp,
    },
}

#[derive(Subcommand)]
enum StateOp {
    /// Print the authoritative "who is playing what" record.
    Show,
    /// List every peer's last-known playback snapshot.
    ByPeer,
    /// Request that the peer currently playing yields to this host.
    Takeover {
        /// The host to take over from (the current playing peer).
        peer: String,
    },
}

#[derive(Subcommand)]
enum CacheOp {
    /// Print the cache size, track count, and cap.
    Status {
        /// Cap in GiB (default 10).
        #[arg(long, default_value_t = 10)]
        cap_gb: u64,
    },
    /// Evict least-recently-played non-starred tracks to fit the cap.
    Gc {
        /// Cap in GiB (default 10).
        #[arg(long, default_value_t = 10)]
        cap_gb: u64,
    },
}

fn main() -> ExitCode {
    let args = Args::parse();
    match args.cmd {
        Cmd::Ping => ping(),
        Cmd::Cache { op } => cache_cmd(&op),
        Cmd::State { op } => state_cmd(&op),
    }
}

fn local_hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn state_cmd(op: &StateOp) -> ExitCode {
    let dir = state::data_dir();
    match op {
        StateOp::Show => {
            match state::read_state(&dir) {
                Some(s) if s.playing => {
                    println!("playing on {}: song {} @ {}ms", s.peer, s.song_id, s.position_ms);
                }
                Some(s) => println!("idle (last owner: {})", s.peer),
                None => println!("no mesh playback state (nobody is playing)"),
            }
            ExitCode::SUCCESS
        }
        StateOp::ByPeer => {
            let bp_dir = dir.join("music-state-by-peer");
            match std::fs::read_dir(&bp_dir) {
                Ok(rd) => {
                    let mut any = false;
                    for entry in rd.flatten() {
                        if let Some(s) = std::fs::read_to_string(entry.path())
                            .ok()
                            .and_then(|t| serde_json::from_str::<state::MusicState>(&t).ok())
                        {
                            any = true;
                            println!(
                                "{}: {}",
                                s.peer,
                                if s.playing { "playing" } else { "idle" }
                            );
                        }
                    }
                    if !any {
                        println!("no peer snapshots yet");
                    }
                }
                Err(_) => println!("no peer snapshots yet"),
            }
            ExitCode::SUCCESS
        }
        StateOp::Takeover { peer } => {
            let me = local_hostname();
            match state::post_takeover(&dir, &me, Some(peer.clone()), now_ms()) {
                Ok(i) => {
                    println!("take-over requested: {} → {} (intent {})", me, peer, i.intent_id);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("mde-musicd: take-over failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn cache_cmd(op: &CacheOp) -> ExitCode {
    let dir = cache::cache_dir();
    match op {
        CacheOp::Status { cap_gb } => {
            let index = cache::read_index(&dir);
            let cap = cap_gb * 1024 * 1024 * 1024;
            println!(
                "music cache: {} across {} track(s) (cap {})",
                cache::human_bytes(index.total_bytes()),
                index.entries.len(),
                cache::human_bytes(cap),
            );
            ExitCode::SUCCESS
        }
        CacheOp::Gc { cap_gb } => {
            let cap = cap_gb * 1024 * 1024 * 1024;
            match cache::run_gc(&dir, cap) {
                Ok(evicted) => {
                    println!("music cache: evicted {} track(s)", evicted.len());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("mde-musicd: cache gc failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
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
