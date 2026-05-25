//! `dev.mackes.MDE.Portal` D-Bus surface.
//!
//! Exposes four methods that mded, the sway config keybinds, and
//! other MDE components use to drive the portal:
//!
//! - `Goto(layer: &str)` — navigate to a named layer inside Portal-full
//!   (e.g. `"hub"`, `"library"`, `"control"`, `"voip"`, `"network"`).
//! - `Focus` — bring Portal-full to the foreground (unhide / raise).
//! - `Lock` — trigger the lock-screen surface (Portal-25).
//! - `ToggleDND` — flip mesh-wide Do-Not-Disturb on/off (Portal-33).

use std::sync::Arc;

use tokio::sync::Mutex;
use zbus::interface;

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
        // Portal-16 will wire the sway scratchpad + Iced state
        // machine. For Portal-1, we log + return Ok so the D-Bus
        // call completes cleanly and the bus name is exercisable.
        Ok(())
    }

    /// Bring Portal-full to the foreground.
    async fn focus(&self) -> zbus::fdo::Result<()> {
        tracing::info!("Portal.Focus");
        Ok(())
    }

    /// Activate the lock-screen surface (Portal-25).
    async fn lock(&self) -> zbus::fdo::Result<()> {
        tracing::info!("Portal.Lock");
        Ok(())
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
}
