//! `mde-popover` — Iced + wlr-layer-shell popover host.
//!
//! v3.0.2 panel-host wiring: the panel (`mde-panel`) spawns this
//! binary on every clickable zone press. Each popover is a separate
//! layer-shell overlay surface that anchors above the panel edge,
//! dismisses on Esc / outside-click / close-button, and exits cleanly
//! when the user commits or cancels.
//!
//! ```text
//!   mde-popover start-menu         # M button → app launcher
//!   mde-popover audio              # ♫ click → volume slider
//!   mde-popover notifications      # bell click → notification list
//!   mde-popover clock              # clock click → calendar
//!   mde-popover network            # network click → connection list
//! ```
//!
//! Per-kind ports: start-menu, audio, clock, notifications all ship
//! working today with the v3.0.3 close-button + the panel-side
//! toggle dedup + zombie reap fixes. The network kind is
//! grandfathered as an exit-0 stub under §0.12 until the v3.0.3
//! network-popover task closes.

#![forbid(unsafe_code)]

mod admin_menu;
mod audio;
mod clock;
mod dismiss;
mod fonts;
mod notifications;
mod start_menu;
mod watermark;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "mde-popover",
    about = "Mackes Desktop Environment popover overlay surfaces"
)]
struct Cli {
    /// Which popover to mount.
    #[arg(value_enum)]
    kind: Kind,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    StartMenu,
    Audio,
    Notifications,
    Clock,
    Network,
    /// v3.0.3 — right-click-on-Start admin menu (9 actions, 5
    /// sections, foot --hold + pkexec).
    AdminMenu,
    /// v3.0.3 — long-running bottom-right watermark surface.
    /// Polls dnf every 4 hours; visible only when updates pending;
    /// click invokes `pkexec dnf upgrade`. Spawned at session
    /// start via data/sway/config rather than from the panel.
    Watermark,
}

fn main() -> iced_layershell::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("MDE_POPOVER_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("mde_popover=info,warn")),
        )
        .json()
        .init();

    let cli = Cli::parse();
    tracing::info!(kind = ?cli.kind, "mde-popover spawned");

    match cli.kind {
        Kind::StartMenu => start_menu::run(),
        Kind::Audio => audio::run(),
        Kind::Notifications => notifications::run(),
        Kind::Clock => clock::run(),
        Kind::AdminMenu => admin_menu::run(),
        Kind::Watermark => watermark::run(),
        Kind::Network => {
            // Network popover is grandfathered v3.1 follow-up
            // (needs NM D-Bus surface bindings + a connection-list
            // widget set); stub branch keeps the panel click from
            // erroring. Tracked as a v3.0.3 worklist task; closes
            // by replacing this arm with `network::run()`.
            tracing::info!("network popover not yet implemented; exit 0");
            Ok(())
        }
    }
}
