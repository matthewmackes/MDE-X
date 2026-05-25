//! Client side of `dev.mackes.MDE.Portal` — mackesd calls these to drive
//! the portal from daemon-side events (idle-lock, mesh alerts, DND sync).
//!
//! v6.0 Portal-1: thin async proxy around the four Portal methods.
//! mackesd callers (idle-lock worker, alert relay, etc.) import
//! [`PortalClient::new`] + call `.lock()` / `.goto()` / `.toggle_dnd()`.
//! The proxy silently drops calls when the portal is not running —
//! operator-visible operations (like lock) log at warn level so the
//! operator knows the portal was unreachable.

#[cfg(feature = "async-services")]
mod inner {
    /// Well-known D-Bus name registered by `mde-portal`.
    pub const PORTAL_BUS_NAME: &str = "dev.mackes.MDE.Portal";
    /// Object path where `dev.mackes.MDE.Portal` is served.
    pub const PORTAL_OBJECT_PATH: &str = "/dev/mackes/MDE/Portal";

    /// Async proxy for `dev.mackes.MDE.Portal`.
    ///
    /// Wraps a `zbus::Connection`; each method call opens a one-shot
    /// call-and-return message exchange.  The proxy is cheap to
    /// construct — `zbus::Connection` is `Clone + Send + Sync`.
    #[derive(Clone, Debug)]
    pub struct PortalClient {
        conn: zbus::Connection,
    }

    impl PortalClient {
        /// Construct a client from an already-open session-bus connection.
        pub fn new(conn: zbus::Connection) -> Self {
            Self { conn }
        }

        /// Call `Portal.Goto(layer)` — navigate to a named Portal-full layer.
        ///
        /// Silently returns `Ok(())` when the portal bus name is absent
        /// (binary not running). Logs at `info` level on success.
        pub async fn goto(&self, layer: &str) -> anyhow::Result<()> {
            let proxy = zbus::Proxy::new(
                &self.conn,
                PORTAL_BUS_NAME,
                PORTAL_OBJECT_PATH,
                "dev.mackes.MDE.Portal",
            )
            .await?;
            proxy.call_method("Goto", &(layer,)).await?;
            tracing::info!(layer, "PortalClient: Goto succeeded");
            Ok(())
        }

        /// Call `Portal.Focus` — bring Portal-full to the foreground.
        pub async fn focus(&self) -> anyhow::Result<()> {
            let proxy = zbus::Proxy::new(
                &self.conn,
                PORTAL_BUS_NAME,
                PORTAL_OBJECT_PATH,
                "dev.mackes.MDE.Portal",
            )
            .await?;
            proxy.call_method("Focus", &()).await?;
            tracing::info!("PortalClient: Focus succeeded");
            Ok(())
        }

        /// Call `Portal.Lock` — activate the lock-screen surface.
        ///
        /// Logs at `warn` level when the portal is unreachable so an
        /// operator debugging a failed lock can see why the screen
        /// didn't lock.
        pub async fn lock(&self) -> anyhow::Result<()> {
            let proxy = zbus::Proxy::new(
                &self.conn,
                PORTAL_BUS_NAME,
                PORTAL_OBJECT_PATH,
                "dev.mackes.MDE.Portal",
            )
            .await?;
            match proxy.call_method("Lock", &()).await {
                Ok(_) => {
                    tracing::info!("PortalClient: Lock succeeded");
                    Ok(())
                }
                Err(e) => {
                    tracing::warn!(error = %e, "PortalClient: Lock failed — portal may not be running");
                    Err(e.into())
                }
            }
        }

        /// Call `Portal.ToggleDND` — flip mesh-wide Do-Not-Disturb.
        ///
        /// Returns the new DND state (`true` = enabled).
        pub async fn toggle_dnd(&self) -> anyhow::Result<bool> {
            let proxy = zbus::Proxy::new(
                &self.conn,
                PORTAL_BUS_NAME,
                PORTAL_OBJECT_PATH,
                "dev.mackes.MDE.Portal",
            )
            .await?;
            let new_state: bool = proxy.call_method("ToggleDND", &()).await?.body().deserialize()?;
            tracing::info!(dnd = new_state, "PortalClient: ToggleDND succeeded");
            Ok(new_state)
        }
    }
}

#[cfg(feature = "async-services")]
pub use inner::{PortalClient, PORTAL_BUS_NAME, PORTAL_OBJECT_PATH};

#[cfg(test)]
mod tests {
    #[test]
    fn portal_bus_name_matches_mde_portal_constant() {
        #[cfg(feature = "async-services")]
        assert_eq!(super::PORTAL_BUS_NAME, "dev.mackes.MDE.Portal");
    }

    #[test]
    fn portal_object_path_matches_mde_portal_constant() {
        #[cfg(feature = "async-services")]
        assert_eq!(super::PORTAL_OBJECT_PATH, "/dev/mackes/MDE/Portal");
    }
}
