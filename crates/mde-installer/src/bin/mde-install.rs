//! `mde-install` — converge this node to a known-clean state for a
//! chosen install profile, then run birthrights.
//!
//! INST-3a / INST-4 (picker) / INST-5 (typed-`NUKE` confirm + audit
//! log). The clean Fedora-Server build-up path: pick a profile →
//! confirm → wipe MDE local state → run `mackes.birthright` for the
//! profile. Re-install extras (Nebula cert-revoke + GlusterFS brick
//! teardown) are INST-3b, blocked on a mackesd `Ca.Revoke` method;
//! on a clean box there is nothing there to tear down.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use mde_installer::confirm;
use mde_installer::profile::Profile;
use mde_installer::wipe;

#[derive(Parser, Debug)]
#[command(
    name = "mde-install",
    about = "Converge an MDE node to a known-clean state for an install profile, then run birthrights."
)]
struct Args {
    /// Install profile (skips the interactive picker): lighthouse|headless|full.
    #[arg(long)]
    profile: Option<String>,

    /// Non-interactive: skip the typed-NUKE confirm (writes an audit log instead). Requires --profile.
    #[arg(long)]
    yes: bool,

    /// Print the plan without changing anything.
    #[arg(long)]
    dry_run: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("mde-install: {msg}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let profile = resolve_profile(args)?;
    println!("Install profile: {profile} — {}", profile.describe());
    if profile.needs_desktop_rpm() && !desktop_rpm_present() {
        println!(
            "note: the `full` profile needs the `mde-desktop` RPM; install it with \
             `dnf install mde-desktop` to get the sway desktop (mesh substrate still installs)."
        );
    }

    // Preflight — what the wipe will remove.
    let all = wipe::local_state_paths();
    let targets = wipe::existing(&all);
    println!("\nLocal MDE state to be wiped:");
    if targets.is_empty() {
        println!("  (none — clean machine)");
    } else {
        for p in &targets {
            println!("  {}", p.display());
        }
    }
    println!(
        "Peers affected: not shown — cross-peer impact needs mackesd peer-version \
         tracking (INST-PEERVER, not yet shipped)."
    );

    if args.dry_run {
        println!("\n[dry-run] would stop {:?}, wipe the paths above, write the \
                  profile marker, restart services, then run birthrights for {profile}.",
                 wipe::MANAGED_SERVICES);
        return Ok(());
    }

    // Confirm.
    if confirm::stdin_is_tty() && !args.yes {
        let ok = confirm::require_typed(
            "NUKE",
            "\nType NUKE to wipe the above and (re)install: ",
        )
        .map_err(|e| format!("reading confirmation: {e}"))?;
        if !ok {
            return Err("not confirmed — aborted".to_string());
        }
    } else if !args.yes {
        return Err(
            "no TTY for the NUKE confirm; re-run with --yes (and --profile) for unattended installs"
                .to_string(),
        );
    }

    let mut audit = Vec::new();
    audit.push(format!("profile: {profile}"));

    // Wipe sequence (clean-install scope).
    for line in wipe::stop_services(wipe::MANAGED_SERVICES) {
        audit.push(line);
    }
    for line in wipe::wipe_paths(&all) {
        audit.push(line);
    }
    if let Err(e) = wipe::write_profile_marker(profile) {
        audit.push(format!("FAILED to write profile marker: {e}"));
    } else {
        audit.push(format!("wrote {}", wipe::PROFILE_MARKER));
    }
    for line in wipe::start_services(wipe::MANAGED_SERVICES) {
        audit.push(line);
    }

    // Always leave an audit trail (INST-5).
    let log_path = write_audit_log(&audit);
    match &log_path {
        Ok(p) => println!("audit log: {}", p.display()),
        Err(e) => eprintln!("mde-install: could not write audit log: {e}"),
    }

    // Birthrights for the profile.
    run_birthrights(profile)?;
    println!("\nmde-install: {profile} node converged.");
    Ok(())
}

fn resolve_profile(args: &Args) -> Result<Profile, String> {
    if let Some(p) = &args.profile {
        return p.parse::<Profile>().map_err(|e| e.to_string());
    }
    if confirm::stdin_is_tty() {
        let default = desktop_rpm_present().then_some(Profile::Full);
        let stdin = std::io::stdin();
        let mut locked = stdin.lock();
        let mut out = std::io::stdout();
        return confirm::pick_profile_from(&mut locked, &mut out, default)
            .map_err(|e| format!("reading profile choice: {e}"));
    }
    Err("no --profile and no TTY for the picker; pass --profile=lighthouse|headless|full".to_string())
}

fn desktop_rpm_present() -> bool {
    Command::new("rpm")
        .args(["-q", "mde-desktop"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_birthrights(profile: Profile) -> Result<(), String> {
    println!("\nRunning birthrights for {profile}…");
    let status = Command::new("python3")
        .args([
            "-m",
            "mackes.birthright",
            "--profile",
            profile.as_str(),
            "--noninteractive",
        ])
        .status()
        .map_err(|e| format!("spawning birthrights: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "birthrights exited with {}",
            status.code().unwrap_or(-1)
        ))
    }
}

fn write_audit_log(lines: &[String]) -> std::io::Result<PathBuf> {
    let dir = PathBuf::from("/var/log/mde");
    fs::create_dir_all(&dir)?;
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("wipe-{ms}.log"));
    let mut f = fs::File::create(&path)?;
    for line in lines {
        writeln!(f, "{line}")?;
    }
    Ok(path)
}
