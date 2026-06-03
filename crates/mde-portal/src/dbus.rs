//! `dev.mackes.MDE.Portal.Full` client proxy + shared Portal state.
//!
//! DBUS-2 retired the `dev.mackes.MDE.Portal` **server** interface that
//! used to live here: mde-open + mackesd now drive the Portal over the
//! Bus (`action/shell/<verb>` — see [`crate::bus_responder`]), per the
//! Q96 Bus-canonical lock. What remains is the typed state the shell
//! dispatch mutates ([`PortalState`]) and the client proxy that forwards
//! a `Goto(layer)` to the `mde-portal-full` surface — whose own
//! `dev.mackes.MDE.Portal.Full` interface is a separate D-Bus
//! retirement, so this proxy stays for now.

use std::sync::Arc;

use tokio::sync::Mutex;

/// Shared runtime state the shell dispatch reads + mutates.
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

    /// Flip mesh-wide Do-Not-Disturb and return the new state. Backs
    /// `action/shell/toggle-dnd` + the `mde://dnd-toggle` URI.
    pub async fn toggle_dnd_inner(&self) -> bool {
        let mut inner = self.inner.lock().await;
        inner.dnd_enabled = !inner.dnd_enabled;
        inner.dnd_enabled
    }

    /// Return current DND state.
    #[cfg(test)]
    pub async fn dnd_enabled(&self) -> bool {
        self.inner.lock().await.dnd_enabled
    }
}

/// zbus-generated async proxy for the `dev.mackes.MDE.Portal.Full`
/// interface exposed by the `mde-portal-full` binary. The Dock + the
/// shell dispatch forward a `Goto(layer)` through it to switch the
/// active content layer.
#[zbus::proxy(
    interface = "dev.mackes.MDE.Portal.Full",
    default_service = "dev.mackes.MDE.Portal.Full",
    default_path = "/dev/mackes/MDE/Portal/Full"
)]
trait PortalFull {
    async fn goto(&self, layer: &str) -> zbus::Result<()>;
}

/// Forward a `Goto(layer)` call to the `mde-portal-full` surface.
///
/// Silently ignores all errors — the Portal-full binary may not be
/// running yet; callers should never block on its availability.
pub async fn portal_full_goto(layer: &str) {
    let Ok(conn) = zbus::Connection::session().await else {
        return;
    };
    let Ok(proxy) = PortalFullProxy::new(&conn).await else {
        return;
    };
    let _ = proxy.goto(layer).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn toggle_dnd_inner_flips_state() {
        let state = PortalState::new();
        assert!(!state.dnd_enabled().await);
        assert!(state.toggle_dnd_inner().await);
        assert!(state.dnd_enabled().await);
        assert!(!state.toggle_dnd_inner().await);
        assert!(!state.dnd_enabled().await);
    }

    #[tokio::test]
    async fn portal_state_new_dnd_false() {
        let state = PortalState::new();
        assert!(!state.dnd_enabled().await, "DND starts disabled");
    }

    #[tokio::test]
    async fn portal_full_goto_does_not_panic_when_service_absent() {
        // Service not running in tests; the forward returns silently.
        portal_full_goto("hub").await;
    }
}
