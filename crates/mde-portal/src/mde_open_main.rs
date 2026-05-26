//! `mde-open` — `mde://` URI dispatcher (Portal-35).
//!
//! Tiny binary registered as `x-scheme-handler/mde` so that
//! `xdg-open mde://library/Downloads` routes to the running
//! `dev.mackes.MDE.Portal` D-Bus service.
//!
//! Usage:
//!
//! ```bash
//!   mde-open mde://hub
//!   mde-open mde://library/Downloads
//!   mde-open mde://lock
//!   mde-open mde://app/org.gnome.TextEditor
//! ```
//!
//! Exit codes:
//!   0  — URI parsed and dispatched successfully
//!   1  — usage error (missing argument)
//!   2  — D-Bus dispatch failed (portal not running, etc.)
//!   3  — URI did not parse to a known verb

#![forbid(unsafe_code)]

use std::process::ExitCode;

use mde_portal::uri::{parse_mde_uri, Action};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("MDE_OPEN_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("mde_open=info,warn")),
        )
        .json()
        .init();

    let Some(uri) = std::env::args().nth(1) else {
        eprintln!("usage: mde-open <mde://...>");
        return ExitCode::from(1);
    };

    let action = parse_mde_uri(&uri);
    tracing::info!(uri = %uri, action = ?action, "mde-open: parsed");

    if let Action::Unknown(ref raw) = action {
        eprintln!("mde-open: unrecognized URI: {raw}");
        return ExitCode::from(3);
    }

    let conn = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("mde-open: cannot reach session bus: {e}");
            return ExitCode::from(2);
        }
    };

    let proxy = match mde_portal::dbus_proxy::PortalProxy::new(&conn).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("mde-open: cannot construct Portal proxy: {e}");
            return ExitCode::from(2);
        }
    };

    match proxy.open_uri(&uri).await {
        Ok(canonical) => {
            tracing::info!(canonical, "mde-open: dispatched");
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("mde-open: Portal.OpenUri failed: {e}");
            ExitCode::from(2)
        }
    }
}
