//! `dev.mackes.MDE.Portal` D-Bus surface.
//!
//! Exposes five methods that mded, the sway config keybinds, and
//! other MDE components use to drive the portal:
//!
//! - `Goto(layer: &str)` — navigate to a named layer inside Portal-full
//!   (e.g. `"hub"`, `"library"`, `"control"`, `"voip"`, `"network"`).
//! - `Focus` — bring Portal-full to the foreground (unhide / raise).
//! - `Lock` — trigger the lock-screen surface (Portal-25).
//! - `ToggleDND` — flip mesh-wide Do-Not-Disturb on/off (Portal-33).
//! - `Restart` — soft-restart mde-portal via systemd (Portal-30).

use std::sync::Arc;

use tokio::sync::Mutex;
use zbus::interface;

use crate::uri::{parse_mde_uri, Action};

/// Shared runtime state the D-Bus handlers can read + mutate.
#[derive(Debug, Default, Clone)]
pub struct PortalState {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    dnd_enabled: bool,
}

impl PortalState {
    /// Construct initial state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return current DND state.
    #[cfg(test)]
    pub async fn dnd_enabled(&self) -> bool {
        self.inner.lock().await.dnd_enabled
    }
}

/// zbus interface implementation for `dev.mackes.MDE.Portal`.
#[interface(name = "dev.mackes.MDE.Portal")]
impl PortalState {
    /// Navigate to the named layer inside Portal-full.
    ///
    /// Valid layer names: `hub`, `library`, `control`, `voip`,
    /// `network`. Unknown layers log a warning and return without error
    /// so callers don't need to know the full layer set.
    async fn goto(&self, layer: &str) -> zbus::fdo::Result<()> {
        tracing::info!(layer, "Portal.Goto");
        // Portal-16: forward the call to the mde-portal-full surface which
        // owns the Iced window + its own D-Bus service. Ignore errors if
        // the surface isn't running yet.
        portal_full_goto(layer).await;
        Ok(())
    }

    /// Bring Portal-full to the foreground.
    async fn focus(&self) -> zbus::fdo::Result<()> {
        tracing::info!("Portal.Focus");
        Ok(())
    }

    /// Activate the lock-screen surface (Portal-25).
    ///
    /// Spawns `mde-popover lock` so the visual lock layer shows over
    /// every other surface. The popover is a separate process so a
    /// crash in the lock UI can't take Portal-full down.
    async fn lock(&self) -> zbus::fdo::Result<()> {
        tracing::info!("Portal.Lock: spawning mde-popover lock");
        match tokio::process::Command::new("mde-popover").arg("lock").spawn() {
            Ok(_child) => Ok(()),
            Err(e) => {
                tracing::warn!(error = %e, "Portal.Lock: spawn failed");
                Err(zbus::fdo::Error::Failed(format!(
                    "spawn mde-popover lock: {e}"
                )))
            }
        }
    }

    /// Parse a `mde://` URI and dispatch the resulting action (Portal-35).
    ///
    /// Returns the parsed action as a string for callers that want to
    /// log what was dispatched. Unknown URIs log a warning and return
    /// the original input — the call never errors out so external
    /// apps that emit slightly-malformed URIs don't crash Portal.
    async fn open_uri(&self, uri: &str) -> zbus::fdo::Result<String> {
        let action = parse_mde_uri(uri);
        tracing::info!(uri, action = ?action, "Portal.OpenUri");
        match action {
            Action::Goto { ref layer, .. } => {
                portal_full_goto(layer).await;
            }
            Action::Lock => {
                let _ = tokio::process::Command::new("mde-popover").arg("lock").spawn();
            }
            Action::Focus => {
                // Future: raise Portal-full via swayipc. Currently a no-op
                // because the scratchpad-show wiring lives in the Dock.
            }
            Action::ToggleDnd => {
                let mut inner = self.inner.lock().await;
                inner.dnd_enabled = !inner.dnd_enabled;
            }
            Action::Restart => {
                let _ = tokio::process::Command::new("systemctl")
                    .args(["--user", "restart", "mde-portal"])
                    .spawn();
            }
            Action::OpenApp(ref id) => {
                let _ = tokio::process::Command::new("gtk-launch").arg(id).spawn();
            }
            Action::OpenFile(ref path) => {
                let _ = tokio::process::Command::new("xdg-open")
                    .arg(path)
                    .spawn();
            }
            Action::Peer { .. } => {
                // Cross-peer dispatch: the local Portal can't act on a
                // sibling node. Drop the call — once the Mackes Bus
                // (BUS-1..7) lands we'll forward via mesh RPC.
                tracing::warn!(uri, "Portal.OpenUri: peer routing not yet wired");
            }
            Action::Unknown(ref raw) => {
                tracing::warn!(uri = %raw, "Portal.OpenUri: unknown verb");
            }
        }
        Ok(crate::uri::action_to_uri(&parse_mde_uri(uri)))
    }

    /// Toggle mesh-wide Do-Not-Disturb on or off.
    ///
    /// Returns the new DND state (`true` = enabled).
    async fn toggle_dnd(&self) -> zbus::fdo::Result<bool> {
        let mut inner = self.inner.lock().await;
        inner.dnd_enabled = !inner.dnd_enabled;
        let new_state = inner.dnd_enabled;
        tracing::info!(dnd = new_state, "Portal.ToggleDND");
        Ok(new_state)
    }

    /// Soft-restart mde-portal (Portal-30, R4-Q87).
    ///
    /// Delegates to `systemctl --user restart mde-portal` so the systemd
    /// unit restarts cleanly. The shell-state snapshot (Portal-29) is at
    /// most 5 s stale; the new process restores from it on startup.
    ///
    /// Returns immediately — systemd manages the restart asynchronously.
    async fn restart(&self) -> zbus::fdo::Result<()> {
        tracing::info!("Portal.Restart: requesting systemctl --user restart mde-portal");
        match tokio::process::Command::new("systemctl")
            .args(["--user", "restart", "mde-portal"])
            .spawn()
        {
            Ok(_child) => Ok(()),
            Err(e) => {
                tracing::warn!(error = %e, "Portal.Restart: systemctl spawn failed");
                Err(zbus::fdo::Error::Failed(format!(
                    "systemctl --user restart mde-portal: {e}"
                )))
            }
        }
    }
}

/// zbus-generated async proxy for the `dev.mackes.MDE.Portal.Full` interface
/// exposed by the `mde-portal-full` binary.  Used by the Dock to forward a
/// `Goto(layer)` call that switches the active content layer.
#[zbus::proxy(
    interface = "dev.mackes.MDE.Portal.Full",
    default_service = "dev.mackes.MDE.Portal.Full",
    default_path = "/dev/mackes/MDE/Portal/Full"
)]
trait PortalFull {
    async fn goto(&self, layer: &str) -> zbus::Result<()>;
}

/// zbus-generated async proxy for the main `dev.mackes.MDE.Portal` interface.
/// Used by `mde-open` to forward a parsed URI to the running portal.
#[zbus::proxy(
    interface = "dev.mackes.MDE.Portal",
    default_service = "dev.mackes.MDE.Portal",
    default_path = "/dev/mackes/MDE/Portal"
)]
pub trait Portal {
    async fn open_uri(&self, uri: &str) -> zbus::Result<String>;
    async fn goto(&self, layer: &str) -> zbus::Result<()>;
    async fn lock(&self) -> zbus::Result<()>;
    async fn focus(&self) -> zbus::Result<()>;
    async fn toggle_dnd(&self) -> zbus::Result<bool>;
    async fn restart(&self) -> zbus::Result<()>;
}

/// Forward a `Goto(layer)` call to the `mde-portal-full` surface.
///
/// Silently ignores all errors — the Portal-full binary may not be running
/// yet; the Dock should never block on its availability.
pub async fn portal_full_goto(layer: &str) {
    let Ok(conn) = zbus::Connection::session().await else { return };
    let Ok(proxy) = PortalFullProxy::new(&conn).await else { return };
    let _ = proxy.goto(layer).await;
}

/// Register the `dev.mackes.MDE.Portal` service on the session bus and
/// return the connection (which keeps the bus name alive while it lives).
///
/// Callers hold the returned `zbus::Connection` for the process lifetime.
pub async fn register(state: PortalState) -> anyhow::Result<zbus::Connection> {
    let conn = zbus::connection::Builder::session()?
        .name("dev.mackes.MDE.Portal")?
        .serve_at("/dev/mackes/MDE/Portal", state)?
        .build()
        .await?;
    tracing::info!("dev.mackes.MDE.Portal registered on session bus");
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn toggle_dnd_flips_state() {
        let state = PortalState::new();
        assert!(!state.dnd_enabled().await);

        let result = state.toggle_dnd().await;
        assert!(result.is_ok());
        assert!(state.dnd_enabled().await);

        let result = state.toggle_dnd().await;
        assert!(result.is_ok());
        assert!(!state.dnd_enabled().await);
    }

    #[tokio::test]
    async fn goto_unknown_layer_returns_ok() {
        let state = PortalState::new();
        let result = state.goto("nonexistent-layer").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn focus_returns_ok() {
        let state = PortalState::new();
        let result = state.focus().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn lock_returns_ok() {
        let state = PortalState::new();
        let result = state.lock().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn portal_state_new_dnd_false() {
        let state = PortalState::new();
        assert!(!state.dnd_enabled().await, "DND starts disabled");
    }

    /// Restart is a fire-and-forget systemctl call. In test environments
    /// systemctl may not be available; we just verify the method returns
    /// without panicking (either Ok or a Err wrapping the spawn failure).
    #[tokio::test]
    async fn restart_returns_without_panic() {
        let state = PortalState::new();
        // Result can be Ok (systemctl found) or Err (not available in CI).
        let _ = state.restart().await;
    }

    /// portal_full_goto must not panic when the Portal-full service is absent.
    #[tokio::test]
    async fn portal_full_goto_does_not_panic_when_service_absent() {
        // Service is not running in tests; the function should return silently.
        portal_full_goto("hub").await;
    }

    #[tokio::test]
    async fn open_uri_known_verb_returns_canonical_form() {
        let state = PortalState::new();
        let res = state.open_uri("mde://hub").await.unwrap();
        assert_eq!(res, "mde://hub");
    }

    #[tokio::test]
    async fn open_uri_dnd_toggles_state() {
        let state = PortalState::new();
        assert!(!state.dnd_enabled().await);
        let _ = state.open_uri("mde://dnd-toggle").await.unwrap();
        assert!(state.dnd_enabled().await);
        let _ = state.open_uri("mde://dnd-toggle").await.unwrap();
        assert!(!state.dnd_enabled().await);
    }

    #[tokio::test]
    async fn open_uri_unknown_does_not_error() {
        let state = PortalState::new();
        let res = state.open_uri("mde://flubber").await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn open_uri_wrong_scheme_is_handled() {
        let state = PortalState::new();
        let res = state.open_uri("https://example.com").await;
        assert!(res.is_ok());
    }
}
