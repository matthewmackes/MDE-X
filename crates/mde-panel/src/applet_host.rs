//! Phase E.4-E.29 panel-host applet orchestration.
//!
//! Polls each `mde-applet-*` binary via its `--now` mode at the
//! cadence appropriate for that applet's data source, and streams
//! every rendered line back into the panel via an Iced subscription.
//! The panel's `App::update` receives an `AppletText { kind, text }`
//! message per emit, which it stores in `TopBarState` for the view
//! to render verbatim.
//!
//! ## Why OS threads instead of `tokio::spawn`
//!
//! `iced_layershell` polls subscription streams outside the tokio
//! runtime's `enter` guard — so any future that needs the tokio
//! reactor (process I/O, time::sleep) parks and never wakes. We
//! sidestep that by running each applet driver on a real OS thread
//! that uses blocking `std::process::Command` + `std::thread::sleep`,
//! and pushes results into the Iced subscription via the runtime-
//! agnostic `mpsc::UnboundedSender` / `try_send` path.
//!
//! `--now` is the lowest-common-denominator protocol every Phase E1
//! applet ships: it prints the current rendered string to stdout and
//! exits cleanly. Some applets also support a long-running stdio
//! mode (clock, audio), but `--now` works for the fire-once ones
//! (sway-cluster, status-cluster) too — making it the simplest host
//! protocol that handles every applet uniformly.

use std::process::Command;
use std::thread;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::stream::Stream;
use iced::stream;
use iced::Subscription;

/// One applet binary the panel hosts as a stdio subprocess. Each
/// variant maps 1:1 to a `/usr/bin/mde-applet-*` binary already
/// shipped by the Phase E1.2.x ports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppletKind {
    Clock,
    Audio,
    Network,
    MeshStatus,
    StatusCluster,
    SwayCluster,
    NotificationBell,
    Dock,
}

impl AppletKind {
    /// Stable order used by `applet_stream` to fan out spawns. New
    /// kinds append; never re-order — the panel layout depends on
    /// this iteration order matching the view's zone composition.
    pub const ALL: &'static [AppletKind] = &[
        AppletKind::Clock,
        AppletKind::Audio,
        AppletKind::Network,
        AppletKind::MeshStatus,
        AppletKind::StatusCluster,
        AppletKind::SwayCluster,
        AppletKind::NotificationBell,
        AppletKind::Dock,
    ];

    /// Bare PATH-resolved binary name. Mirrors
    /// `crates/mde-panel/src/host.rs::default_bindings` +
    /// `tray_applets`.
    pub const fn binary(self) -> &'static str {
        match self {
            AppletKind::Clock => "mde-applet-clock",
            AppletKind::Audio => "mde-applet-audio",
            AppletKind::Network => "mde-applet-network",
            AppletKind::MeshStatus => "mde-applet-mesh-status",
            AppletKind::StatusCluster => "mde-applet-status-cluster",
            AppletKind::SwayCluster => "mde-applet-sway-cluster",
            AppletKind::NotificationBell => "mde-applet-notification-bell",
            AppletKind::Dock => "mde-applet-dock",
        }
    }

    /// How often the host pings the applet with a `Visibility`
    /// `HostMessage`. Each applet's `handle_host` returns `true`
    /// for non-`Shutdown` messages, which triggers a re-render.
    ///
    /// Cadences are chosen per data-source volatility:
    /// - **Clock**: 15 s — sub-minute resolution is wasted on the
    ///   `YYYY-MM-DD HH:MM` format the clock emits.
    /// - **Audio / SwayCluster**: 2 s — volume slider drags + window
    ///   focus changes are user-visible; this is the responsiveness
    ///   threshold below which the panel feels stale.
    /// - **Network / MeshStatus / StatusCluster / NotificationBell
    ///   / Dock**: 5 s — state changes (NM connect, peer up, etc.)
    ///   tolerate a 5 s lag.
    pub const fn ping_secs(self) -> u64 {
        match self {
            AppletKind::Clock => 15,
            AppletKind::Audio | AppletKind::SwayCluster => 2,
            AppletKind::Network
            | AppletKind::MeshStatus
            | AppletKind::StatusCluster
            | AppletKind::NotificationBell
            | AppletKind::Dock => 5,
        }
    }
}

/// One applet's latest stdout line — the rendered text the panel
/// should display in that applet's zone.
#[derive(Debug, Clone)]
pub struct AppletText {
    pub kind: AppletKind,
    pub text: String,
}

/// Iced subscription that spawns every applet + streams their
/// stdout lines as `AppletText` events. Wire this into
/// `Application::subscription` and map the result into your panel's
/// `Message::AppletText` variant.
pub fn subscription<M: 'static>(map: fn(AppletText) -> M) -> Subscription<M> {
    Subscription::run(applet_stream).map(map)
}

/// Build the stream that drives every applet. Free function (not
/// closure) because `Subscription::run` takes a bare `fn`.
///
/// All applet drivers run as a single concurrent future via
/// `futures::future::join_all`. We can't use `tokio::spawn` here
/// because the Iced/iced_layershell runtime doesn't enter the
/// tokio runtime context when polling user subscription futures —
/// `tokio::spawn` would panic with "no reactor running". Joining
/// the drivers in a single future keeps everything in scope without
/// needing the runtime handle.
fn applet_stream() -> impl Stream<Item = AppletText> {
    // 1024-slot buffer gives ~2 minutes of headroom at the
    // worst-case applet cadence (2 s × 8 applets = ~4 emits/sec → a
    // full second of stall fills ~4 slots). Previous 64-slot buffer
    // dropped newest-on-full, which was wrong-shaped for a status
    // panel (operator would rather see latest state than oldest).
    // 1024 is large enough that backpressure-driven drops are an
    // operationally-impossible condition; a v3.1 follow-up could
    // switch to single-slot latest-wins per kind if real-world
    // telemetry ever shows drops.
    stream::channel(1024, |sender| async move {
        tracing::info!(
            "applet_host: subscription started; spawning {} OS-thread drivers",
            AppletKind::ALL.len()
        );
        // One OS thread per applet — each thread blocks on
        // `std::process::Command` + `thread::sleep`, then pushes the
        // text into the Iced stream via `try_send`. The threads are
        // detached (we don't keep their JoinHandles); the panel
        // process holding the runtime is what keeps them alive.
        for &kind in AppletKind::ALL {
            let sender = sender.clone();
            thread::Builder::new()
                .name(format!("applet-{}", kind.binary()))
                .spawn(move || drive_applet_blocking(kind, sender))
                .expect("spawn applet driver thread");
        }
        // Hold the future open so the stream stays alive. The threads
        // own clones of `sender`; when they all drop, the stream
        // completes (which never happens in practice).
        std::future::pending::<()>().await;
    })
}

fn drive_applet_blocking(kind: AppletKind, mut sender: mpsc::Sender<AppletText>) {
    let interval = Duration::from_secs(kind.ping_secs());
    tracing::debug!(
        applet = kind.binary(),
        interval_secs = interval.as_secs(),
        "applet_host: driver thread entered"
    );
    loop {
        match poll_now_blocking(kind) {
            Ok(text) if !text.is_empty() => {
                // `try_send` is non-blocking and runtime-agnostic. If
                // the 64-slot buffer is full (e.g., the panel hasn't
                // rendered in a while), drop this update — the next
                // poll will catch up.
                if let Err(err) = sender.try_send(AppletText { kind, text }) {
                    if err.is_disconnected() {
                        tracing::info!(
                            applet = kind.binary(),
                            "applet_host: subscription closed; exiting driver"
                        );
                        return;
                    }
                    // Buffer full — drop and continue.
                }
            }
            Ok(_) => {
                // Empty output is allowed (notification-bell emits ""
                // when no unread). Skip the send so the previous
                // value stays.
            }
            Err(err) => {
                tracing::warn!(
                    applet = kind.binary(),
                    error = %err,
                    "applet --now poll failed"
                );
            }
        }
        thread::sleep(interval);
    }
}

/// Spawn `<binary> --now` and return its stdout as a trimmed string.
/// Every Phase E1 applet supports `--now`; the call is cheap (cold
/// start < 10 ms for the small applets, ~30 ms for sway-cluster's
/// IPC fetch).
fn poll_now_blocking(kind: AppletKind) -> std::io::Result<String> {
    let out = Command::new(kind.binary()).arg("--now").output()?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "{} --now exited {}",
            kind.binary(),
            out.status
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_binary_and_a_ping_cadence() {
        for &k in AppletKind::ALL {
            assert!(k.binary().starts_with("mde-applet-"));
            assert!(k.ping_secs() > 0);
            assert!(k.ping_secs() <= 60);
        }
    }

    #[test]
    fn kind_order_is_stable() {
        // Pin the first + last entries so a reorder regression
        // surfaces in CI even when nobody runs the panel.
        assert_eq!(AppletKind::ALL.first(), Some(&AppletKind::Clock));
        assert_eq!(AppletKind::ALL.last(), Some(&AppletKind::Dock));
        assert_eq!(AppletKind::ALL.len(), 8);
    }

    #[test]
    fn clock_pings_at_15s_not_per_second() {
        assert_eq!(AppletKind::Clock.ping_secs(), 15);
    }

    #[test]
    fn responsive_applets_ping_under_3s() {
        assert!(AppletKind::Audio.ping_secs() <= 3);
        assert!(AppletKind::SwayCluster.ping_secs() <= 3);
    }
}
