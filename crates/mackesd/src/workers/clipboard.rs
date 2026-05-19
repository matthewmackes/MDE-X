//! v2.0.0 Phase B.1 — clipboard worker.
//!
//! Supervises the long-running `python3 -m mackes.clipboard_app`
//! daemon during the v1.x → v2.0.0 transition. Replaces
//! `mackes-clipboard-daemon.service`. The v2.0.0 cut reimplements
//! the clipboard watcher in Rust against `wlr_data_control_v1` via
//! smithay-client-toolkit — this worker is the seam.
//!
//! Long-running supervision (any exit treated as failure → restart)
//! mirrors `workers/fs_sync.rs`.

#![cfg(feature = "async-services")]

use std::ffi::OsString;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::{Child, Command};

use super::{ShutdownToken, Worker};

const SHUTDOWN_GRACE_S: u64 = 5;

/// Worker that spawns + supervises the clipboard daemon.
pub struct ClipboardWorker {
    binary: OsString,
    args:   Vec<OsString>,
}

impl Default for ClipboardWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardWorker {
    /// Construct with the canonical clipboard daemon entry point.
    #[must_use]
    pub fn new() -> Self {
        Self {
            binary: OsString::from("python3"),
            args:   vec![
                OsString::from("-m"),
                OsString::from("mackes.clipboard_app"),
            ],
        }
    }

    /// Construct with custom argv — for tests.
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
impl Worker for ClipboardWorker {
    fn name(&self) -> &'static str {
        "clipboard"
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
                    "clipboard: spawning {} failed: {e}",
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
                    anyhow::anyhow!("clipboard: wait failed: {e}")
                })?;
                Err(anyhow::anyhow!(
                    "clipboard: daemon exited {} (expected to run forever)",
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
    async fn clipboard_worker_name_matches_phase_b_lock() {
        let w = ClipboardWorker::new();
        assert_eq!(w.name(), "clipboard");
    }

    #[tokio::test]
    async fn clipboard_worker_exits_clean_on_shutdown_during_run() {
        let mut w = ClipboardWorker::with_argv(
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
    async fn clipboard_worker_returns_err_on_subprocess_exit() {
        let mut w = ClipboardWorker::with_argv(
            "true",
            Vec::<OsString>::new(),
        );
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
