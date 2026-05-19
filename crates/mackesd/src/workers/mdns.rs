//! v2.0.0 Phase B.2 — mDNS relay worker.
//!
//! Supervises the existing `python3 -m mackes.mesh_mdns` daemon
//! (mDNS announce + watch loop that bridges mesh peer presence
//! between LAN segments). Replaces `mackes-mdns-relay.service`.
//! v2.0.0 cut reimplements the announce + listen loop in Rust
//! against the `mdns-sd` crate — this worker is the seam.
//!
//! Same long-running supervision shape as B.3 fs_sync.

#![cfg(feature = "async-services")]

use std::ffi::OsString;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::{Child, Command};

use super::{ShutdownToken, Worker};

const SHUTDOWN_GRACE_S: u64 = 5;

/// Worker that spawns + supervises the mDNS relay daemon.
pub struct MdnsWorker {
    binary: OsString,
    args:   Vec<OsString>,
}

impl Default for MdnsWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl MdnsWorker {
    /// Construct with the canonical mDNS relay entry point.
    #[must_use]
    pub fn new() -> Self {
        Self {
            binary: OsString::from("python3"),
            args:   vec![
                OsString::from("-m"),
                OsString::from("mackes.mesh_mdns"),
            ],
        }
    }

    /// Custom argv — for tests.
    #[must_use]
    pub fn with_argv(
        binary: impl Into<OsString>,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Self {
        Self {
            binary: binary.into(),
            args:   args.into_iter().map(Into::into).collect(),
        }
    }
}

#[async_trait::async_trait]
impl Worker for MdnsWorker {
    fn name(&self) -> &'static str {
        "mdns"
    }

    async fn run(&mut self, mut shutdown: ShutdownToken) -> anyhow::Result<()> {
        let mut child: Child = Command::new(&self.binary)
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "mdns: spawning {} failed: {e}",
                    self.binary.to_string_lossy()
                )
            })?;

        tokio::select! {
            biased;
            _ = shutdown.wait() => {
                let _ = tokio::time::timeout(
                    Duration::from_secs(SHUTDOWN_GRACE_S),
                    child.wait(),
                ).await;
                let _ = child.start_kill();
                let _ = tokio::time::timeout(
                    Duration::from_secs(1), child.wait(),
                ).await;
                Ok(())
            }
            res = child.wait() => {
                let status = res.map_err(|e| {
                    anyhow::anyhow!("mdns: wait failed: {e}")
                })?;
                Err(anyhow::anyhow!(
                    "mdns: daemon exited {} (expected to run forever)",
                    status.code().map_or("?".to_string(), |c| c.to_string())
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mdns_worker_name_matches_phase_b_lock() {
        let w = MdnsWorker::new();
        assert_eq!(w.name(), "mdns");
    }

    #[tokio::test]
    async fn mdns_worker_exits_clean_on_shutdown_during_run() {
        let mut w = MdnsWorker::with_argv(
            "sleep",
            vec![OsString::from("60")],
        );
        let (tx, rx) = tokio::sync::watch::channel(false);
        let token = ShutdownToken::from_receiver(rx);
        let handle = tokio::spawn(async move { w.run(token).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = tx.send(true);
        let result = tokio::time::timeout(
            Duration::from_secs(10), handle,
        )
        .await
        .expect("exits on shutdown")
        .expect("join");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn mdns_worker_returns_err_on_subprocess_exit() {
        let mut w = MdnsWorker::with_argv("true", Vec::<OsString>::new());
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let token = ShutdownToken::from_receiver(rx);
        let result = tokio::time::timeout(
            Duration::from_secs(3), w.run(token),
        )
        .await
        .expect("exits");
        assert!(result.is_err());
    }
}
