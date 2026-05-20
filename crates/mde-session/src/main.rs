//! `mde-session` — v2.0.0 Phase D.1 (skeleton) + D.4 (lock) + D.7
//! (retires mackes-wm / mackes-enforce-session).
//!
//! User-session orchestrator. systemd's mde-session.service execs
//! this binary at login; it:
//!
//!   1. Reads the v1.x → v2.0.0 config-path migrator's marker (Phase
//!      0.5) — runs the migrator if it hasn't yet.
//!   2. Re-applies persisted settings sidecars (Phase C
//!      $XDG_CACHE_HOME/mde/*) via the matching applier modules.
//!   3. Registers the `dev.mackes.MDE.Session` zbus surface
//!      (Logout / Restart / Shutdown / Lock / SaveLayout).
//!   4. Execs the compositor (sway) and waits.
//!   5. On SIGTERM / SIGINT cleanly tears down: signals the zbus
//!      server, waits for sway to exit, exits 0.
//!
//! Iced + libcosmic for the logout / restart / shutdown dialog
//! (Phase D.2) is intentionally NOT pulled in here — that's a
//! separate process (`mde-logout-dialog`) the Session surface can
//! exec when needed. Keeping this binary Iced-free means the user-
//! session unit stays tiny (~250 LOC) + boots fast.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod autostart;
mod lock;
mod session;

use std::process::Stdio;

use anyhow::Context;
use tokio::process::Command;
use tokio::signal::unix::{signal, SignalKind};

/// Compositor command. Defaults to `sway`; override via
/// `$MDE_COMPOSITOR` for development.
fn compositor_cmd() -> String {
    mackesd_core::env_with_legacy_fallback("MDE_COMPOSITOR", "MACKES_COMPOSITOR")
        .unwrap_or_else(|| "sway".to_owned())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();
    tracing::info!("mde-session: starting");

    // 1. Read autostart entries the user has explicitly enabled and
    //    spawn each as a detached child. Hidden=true overlays
    //    suppress system-wide entries.
    autostart::launch_user_autostart().await;

    // 2. Register the dev.mackes.MDE.Session zbus interface so the
    //    panel + Workbench can drive Logout / Restart / Lock.
    let session = session::SessionState::new();
    let _conn = session::register_zbus(session.clone()).await
        .context("registering dev.mackes.MDE.Session")?;

    // 3. Exec the compositor.
    let cmp = compositor_cmd();
    tracing::info!("mde-session: starting compositor {cmp}");
    let mut child = Command::new(&cmp)
        .stdin(Stdio::null())
        .spawn()
        .with_context(|| format!("spawning {cmp}"))?;

    let mut sigterm = signal(SignalKind::terminate())
        .context("installing SIGTERM handler")?;
    let mut sigint = signal(SignalKind::interrupt())
        .context("installing SIGINT handler")?;

    tokio::select! {
        _ = sigterm.recv() => {
            tracing::info!("mde-session: SIGTERM received; killing compositor");
        }
        _ = sigint.recv() => {
            tracing::info!("mde-session: SIGINT received; killing compositor");
        }
        res = child.wait() => {
            match res {
                Ok(status) => tracing::info!("mde-session: compositor exited {status}"),
                Err(e)     => tracing::error!("mde-session: wait failed: {e}"),
            }
            return Ok(());
        }
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
    tracing::info!("mde-session: exiting");
    Ok(())
}
