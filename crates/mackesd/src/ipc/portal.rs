//! Client side of the Portal shell IPC — mackesd drives the portal from
//! daemon-side events (CRITICAL-alert navigation, idle-lock, DND sync).
//!
//! DBUS-2: this publishes to the Bus (`action/shell/<verb>`) instead of
//! calling the retired `dev.mackes.MDE.Portal` D-Bus service. Publishes
//! are fire-and-forget + durable — the portal acts when its
//! `bus_responder` next polls the topic, even if it was down at publish
//! time. mackesd callers (alert relay, idle-lock) import
//! [`PortalClient::new`] + call `.goto()` / `.lock()` / `.toggle_dnd()`.

#[cfg(feature = "async-services")]
mod inner {
    /// Bus topic prefix the portal's `bus_responder` serves.
    pub const SHELL_TOPIC_PREFIX: &str = "action/shell";

    /// Append one shell command to `action/shell/<verb>` (fire-and-forget).
    /// Opens the Bus store, writes, and returns — the non-`Send` `Persist`
    /// never crosses an `.await`.
    fn publish_shell(verb: &str, body: &str) -> anyhow::Result<()> {
        let dir =
            mde_bus::default_data_dir().ok_or_else(|| anyhow::anyhow!("no Bus data dir"))?;
        let persist = mde_bus::persist::Persist::open(dir)?;
        persist.write(
            &format!("{SHELL_TOPIC_PREFIX}/{verb}"),
            mde_bus::hooks::config::Priority::Default,
            None,
            Some(body),
        )?;
        Ok(())
    }

    /// Bus client for the portal shell verbs.
    ///
    /// Stateless — each call opens the Bus store, appends the command, and
    /// returns. Cheap to clone + construct.
    #[derive(Clone, Debug, Default)]
    pub struct PortalClient;

    impl PortalClient {
        /// Construct a client.
        #[must_use]
        pub fn new() -> Self {
            Self
        }

        /// Publish `action/shell/goto` — navigate to a named Portal-full layer.
        ///
        /// # Errors
        /// Bus-store open / write failures.
        pub async fn goto(&self, layer: &str) -> anyhow::Result<()> {
            publish_shell("goto", layer)?;
            tracing::info!(layer, "PortalClient: published action/shell/goto");
            Ok(())
        }

        /// Publish `action/shell/focus` — bring Portal-full to the foreground.
        ///
        /// # Errors
        /// Bus-store open / write failures.
        pub async fn focus(&self) -> anyhow::Result<()> {
            publish_shell("focus", "")?;
            tracing::info!("PortalClient: published action/shell/focus");
            Ok(())
        }

        /// Publish `action/shell/lock` — activate the lock-screen surface.
        ///
        /// # Errors
        /// Bus-store open / write failures.
        pub async fn lock(&self) -> anyhow::Result<()> {
            publish_shell("lock", "")?;
            tracing::info!("PortalClient: published action/shell/lock");
            Ok(())
        }

        /// Publish `action/shell/toggle-dnd` — flip mesh-wide Do-Not-Disturb.
        ///
        /// Fire-and-forget: the portal owns the authoritative DND state, so
        /// (unlike the old D-Bus call) no new state is returned here.
        ///
        /// # Errors
        /// Bus-store open / write failures.
        pub async fn toggle_dnd(&self) -> anyhow::Result<()> {
            publish_shell("toggle-dnd", "")?;
            tracing::info!("PortalClient: published action/shell/toggle-dnd");
            Ok(())
        }
    }
}

#[cfg(feature = "async-services")]
pub use inner::{PortalClient, SHELL_TOPIC_PREFIX};

#[cfg(test)]
mod tests {
    #[test]
    fn shell_topic_prefix_is_action_shell() {
        #[cfg(feature = "async-services")]
        assert_eq!(super::SHELL_TOPIC_PREFIX, "action/shell");
    }

    #[cfg(feature = "async-services")]
    #[test]
    fn portal_client_constructs() {
        let _ = super::PortalClient::new();
    }
}
