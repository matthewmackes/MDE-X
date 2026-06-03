//! `mde-open` — `mde://` URI dispatcher (Portal-35).
//!
//! Tiny binary registered as `x-scheme-handler/mde` so that
//! `xdg-open mde://library/Downloads` routes to the running portal.
//!
//! DBUS-2: it publishes the URI to the Bus (`action/shell/open-uri`)
//! instead of calling the retired `dev.mackes.MDE.Portal` D-Bus service.
//! This is durable — the portal acts when it next polls the topic, even
//! if it was down at publish time — and Bus-canonical per Q96.
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
//!   0  — URI parsed and published to the Bus
//!   1  — usage error (missing argument)
//!   2  — Bus store unreachable
//!   3  — URI did not parse to a known verb

#![forbid(unsafe_code)]

use std::process::ExitCode;

use mde_bus::hooks::config::Priority;
use mde_portal::uri::{parse_mde_uri, Action};

fn main() -> ExitCode {
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

    let Some(dir) = mde_bus::default_data_dir() else {
        eprintln!("mde-open: no Bus data dir");
        return ExitCode::from(2);
    };
    let persist = match mde_bus::persist::Persist::open(dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("mde-open: cannot open Bus store: {e}");
            return ExitCode::from(2);
        }
    };

    match persist.write("action/shell/open-uri", Priority::Default, None, Some(&uri)) {
        Ok(_) => {
            tracing::info!(uri = %uri, "mde-open: published to action/shell/open-uri");
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("mde-open: publish to Bus failed: {e}");
            ExitCode::from(2)
        }
    }
}
