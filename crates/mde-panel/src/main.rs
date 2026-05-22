//! `mde-panel` binary entry — Phase E.1 skeleton.
//!
//! Launches the Iced panel application. Phase E.2 will wrap this
//! with a `wlr-layer-shell-v1` anchor so the panel pins to the
//! bottom edge with a 40 px exclusive zone.
//!
//! CLI surface (lands per-port):
//! - `--apple-menu`  → Phase E.12 popover
//! - `--expose`      → Phase E.4.4 grid
//! - `--drawer`      → Phase E.8 quick-actions drawer
//! - `--recover`     → Phase E.24 birthright rollback CLI
//! - `--root-menu`   → Phase E.14 wallpaper-area right-click
//! - `--focus <slug>` → Phase E.15 status-cluster click hand-off
//!
//! The skeleton accepts these flags but routes every flag (except
//! `--recover`) into the same Iced app for now — per-port
//! implementations swap in dedicated sub-binaries later.

#![forbid(unsafe_code)]

use clap::Parser;
use tracing::info;

#[derive(Parser, Debug)]
#[command(
    name = "mde-panel",
    about = "Mackes Desktop Environment (MDE) panel — Iced top bar + bottom dock"
)]
struct Cli {
    /// Open the apple-menu popover (Phase E.12).
    #[arg(long, conflicts_with_all = ["expose", "drawer", "recover", "root_menu", "focus"])]
    apple_menu: bool,

    /// Open the exposé grid (Phase E.4.4).
    #[arg(long, conflicts_with_all = ["apple_menu", "drawer", "recover", "root_menu", "focus"])]
    expose: bool,

    /// Open the quick-actions drawer (Phase E.8).
    #[arg(long, conflicts_with_all = ["apple_menu", "expose", "recover", "root_menu", "focus"])]
    drawer: bool,

    /// Print the birthright-rollback preview and exit (Phase E.24).
    #[arg(long, conflicts_with_all = ["apple_menu", "expose", "drawer", "root_menu", "focus"])]
    recover: bool,

    /// Open the wallpaper-area right-click menu (Phase E.14).
    #[arg(long = "root-menu", conflicts_with_all = ["apple_menu", "expose", "drawer", "recover", "focus"])]
    root_menu: bool,

    /// Hand a focus slug to the Workbench (E.15 click target).
    #[arg(long, conflicts_with_all = ["apple_menu", "expose", "drawer", "recover", "root_menu"])]
    focus: Option<String>,
}

fn main() -> iced_layershell::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("MDE_PANEL_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("mde_panel=info,warn")),
        )
        .json()
        .init();

    let cli = Cli::parse();

    if cli.recover {
        mde_panel::recover::run();
        return Ok(());
    }

    use mde_panel::host::{applet_for_subcommand, spawn_by_binary, SubCommand};

    let sub = if cli.apple_menu {
        Some(SubCommand::AppleMenu)
    } else if cli.expose {
        Some(SubCommand::Expose)
    } else if cli.drawer {
        Some(SubCommand::Drawer)
    } else if cli.root_menu {
        Some(SubCommand::RootMenu)
    } else {
        None
    };

    if let Some(sub) = sub {
        let binary = applet_for_subcommand(sub);
        match spawn_by_binary(binary) {
            Ok(mut child) => {
                info!(binary, "spawned applet — waiting for exit");
                let _ = child.wait();
            }
            Err(e) => {
                tracing::error!(binary, error = ?e, "applet spawn failed");
                return Ok(());
            }
        }
        return Ok(());
    }

    if let Some(slug) = cli.focus.as_deref() {
        info!(slug, "focus hand-off — calling mde-workbench");
        let _ = std::process::Command::new("mde-workbench")
            .arg("--focus")
            .arg(slug)
            .spawn();
        return Ok(());
    }

    info!("starting Iced panel app");
    mde_panel::App::run()
}
