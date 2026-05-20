//! `dev.mackes.MDE.Session` zbus surface (Phase D.1).
//!
//! Server-side impl of the interface shape declared in
//! `mackesd_core::ipc::session`. The Workbench Python panels +
//! mde-panel applets call these methods to drive lifecycle events.

use std::sync::Arc;

use tokio::sync::Mutex;
use zbus::interface;

/// Per-session state owned by the server. Tracks the saved layout
/// path + the lock command preference. Wrapped in Arc<Mutex<>> so
/// the zbus interface (which holds `&self`) can mutate it.
#[derive(Clone, Debug, Default)]
pub struct SessionState {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    layout_saved: bool,
}

impl SessionState {
    /// Construct a fresh session-state with `layout_saved=false`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[interface(name = "dev.mackes.MDE.Session")]
impl SessionState {
    /// Request a clean logout (sway exits, graphical-session.target
    /// stops, user returned to the greeter).
    async fn logout(&self) -> zbus::fdo::Result<()> {
        tracing::info!("Session.Logout invoked");
        // The Iced logout dialog (Phase D.2) takes the user's
        // confirmation first; this method is what it calls after
        // the user clicks "Log out". Effect: signal mde-session
        // (parent process) via SIGTERM. systemd's
        // graphical-session.target tear-down handles the rest.
        let pid = std::process::id();
        // Sending SIGTERM to our own PID without unsafe (which the
        // workspace forbids) means shelling out to `kill`.
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
        Ok(())
    }

    /// Reboot the machine via `systemctl reboot`.
    async fn restart(&self) -> zbus::fdo::Result<()> {
        tracing::info!("Session.Restart invoked");
        let _ = std::process::Command::new("systemctl").arg("reboot").status();
        Ok(())
    }

    /// Power-off via `systemctl poweroff`.
    async fn shutdown(&self) -> zbus::fdo::Result<()> {
        tracing::info!("Session.Shutdown invoked");
        let _ = std::process::Command::new("systemctl").arg("poweroff").status();
        Ok(())
    }

    /// Lock the session — runs the configured lock command (Phase
    /// D.4: `swaylock` or whatever the user picks).
    async fn lock(&self) -> zbus::fdo::Result<()> {
        tracing::info!("Session.Lock invoked");
        crate::lock::run_lock_command().await.map_err(|e| {
            zbus::fdo::Error::Failed(format!("lock command failed: {e}"))
        })?;
        Ok(())
    }

    /// Snapshot the current sway layout JSON to
    /// `$XDG_CACHE_HOME/mde/session-layout.json` so the next login
    /// can restore window placement.
    async fn save_layout(&self) -> zbus::fdo::Result<()> {
        tracing::info!("Session.SaveLayout invoked");
        let layout = run_swaymsg_get_tree().await
            .map_err(|e| zbus::fdo::Error::Failed(format!("swaymsg failed: {e}")))?;
        let path = layout_save_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                zbus::fdo::Error::IOError(format!("mkdir {} failed: {e}", parent.display()))
            })?;
        }
        std::fs::write(&path, layout).map_err(|e| {
            zbus::fdo::Error::IOError(format!("write {} failed: {e}", path.display()))
        })?;
        let mut inner = self.inner.lock().await;
        inner.layout_saved = true;
        Ok(())
    }
}

/// Register the SessionState at the canonical object path on the
/// session bus.
///
/// # Errors
/// Returns whatever zbus reports.
pub async fn register_zbus(state: SessionState) -> zbus::Result<zbus::Connection> {
    let conn = zbus::connection::Builder::session()?
        .name(mackesd_core::ipc::session::SERVICE_NAME)?
        .serve_at(mackesd_core::ipc::session::OBJECT_PATH, state)?
        .build()
        .await?;
    Ok(conn)
}

/// Path of the saved-layout sidecar.
fn layout_save_path() -> std::path::PathBuf {
    let cache = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map_or_else(
            || dirs::home_dir().unwrap_or_default().join(".cache"),
            std::path::PathBuf::from,
        );
    cache.join("mde").join("session-layout.json")
}

async fn run_swaymsg_get_tree() -> anyhow::Result<String> {
    let out = tokio::process::Command::new("swaymsg")
        .args(["-t", "get_tree"])
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!("swaymsg get_tree exited non-zero");
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn session_state_starts_with_layout_not_saved() {
        let s = SessionState::new();
        let inner = s.inner.lock().await;
        assert!(!inner.layout_saved);
    }

    #[test]
    fn layout_save_path_honors_xdg_cache_home() {
        let prev = std::env::var_os("XDG_CACHE_HOME");
        std::env::set_var("XDG_CACHE_HOME", "/tmp/test-cache-mde-session");
        let p = layout_save_path();
        assert_eq!(p, std::path::PathBuf::from(
            "/tmp/test-cache-mde-session/mde/session-layout.json"
        ));
        match prev {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None    => std::env::remove_var("XDG_CACHE_HOME"),
        }
    }
}
