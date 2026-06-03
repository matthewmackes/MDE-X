//! Shared Portal state + the goto-forward to the full surface.
//!
//! Historically the `dev.mackes.MDE.Portal` D-Bus surface; DBUS-2 +
//! DBUS-2.b retired all of mde-portal's D-Bus. The module name is kept to
//! limit churn — it now holds only `PortalState` (the DND state the shell
//! dispatch mutates) and `portal_full_goto`, which forwards a layer switch
//! to the mde-portal-full surface over the Bus (`action/shell/goto-full`).

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

/// Forward a `Goto(layer)` to the mde-portal-full surface over the Bus
/// (`action/shell/goto-full`). DBUS-2.b retired the `Portal.Full` D-Bus
/// interface; this publish is fire-and-forget + durable — the full
/// surface acts on its next poll, even if it was down at publish time.
pub async fn portal_full_goto(layer: &str) {
    let layer = layer.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        let Some(dir) = mde_bus::default_data_dir() else {
            return;
        };
        let Ok(persist) = mde_bus::persist::Persist::open(dir) else {
            return;
        };
        let _ = persist.write(
            "action/shell/goto-full",
            mde_bus::hooks::config::Priority::Default,
            None,
            Some(&layer),
        );
    })
    .await;
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
}
