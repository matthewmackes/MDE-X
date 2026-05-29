//! mde-clipd — MDE clipboard daemon (BUS-5.1).
//!
//! Connects to the Wayland compositor via `wlr-data-control-unstable-v1`
//! and logs every clipboard change event as a structured `tracing` record.
//! Supervised by the mded `clipd_supervisor` worker.

use anyhow::Context as _;
use clap::Parser;
use tracing::info;
use wayland_client::Connection;

mod proto;
mod session;

/// mde-clipd — MDE clipboard daemon
#[derive(Parser, Debug)]
#[command(name = "mde-clipd", about = "MDE clipboard daemon (BUS-5)")]
struct Cli {
    /// Tracing filter (e.g. "info", "debug", "mde_clipd=debug").
    #[arg(long, env = "MDE_LOG", default_value = "info")]
    log_level: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(&cli.log_level)
        .with_writer(std::io::stderr)
        .init();

    info!("mde-clipd: starting (BUS-5.1)");

    let conn = Connection::connect_to_env()
        .context("failed to connect to Wayland — is $WAYLAND_DISPLAY set?")?;

    session::run(&conn)
}
