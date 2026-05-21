//! `mde-applet-bg` binary entry — Phase E1.2.12.
//!
//! Spawns `swaybg` as a child process. The applet's own
//! process supervises the child; on Shutdown it kills the
//! swaybg child + exits.

use std::process::{Command, ExitCode, Stdio};

use mde_applet_bg::{build_swaybg_argv, manifest, resolve_wallpaper_path};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a == "--manifest") {
        match serde_json::to_string_pretty(&manifest()) {
            Ok(j) => {
                println!("{j}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("mde-applet-bg: serialize manifest: {e}");
                ExitCode::FAILURE
            }
        }
    } else if argv.iter().any(|a| a == "--path") {
        println!("{}", resolve_wallpaper_path().display());
        ExitCode::SUCCESS
    } else {
        run_swaybg()
    }
}

fn run_swaybg() -> ExitCode {
    let path = resolve_wallpaper_path();
    let argv = build_swaybg_argv(&path);
    let Some((bin, args)) = argv.split_first() else {
        eprintln!("mde-applet-bg: empty argv");
        return ExitCode::FAILURE;
    };
    let mut child = match Command::new(bin)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("mde-applet-bg: failed to spawn swaybg: {e}");
            return ExitCode::FAILURE;
        }
    };
    match child.wait() {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1) as u8),
        Err(e) => {
            eprintln!("mde-applet-bg: wait failed: {e}");
            ExitCode::FAILURE
        }
    }
}
