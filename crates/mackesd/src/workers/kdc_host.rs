//! KDC2-3.10 — `KdcHost` registered as a `mackesd` worker.
//!
//! Owns the `Arc<mde_kdc::pairing::PairingStore>` + the
//! `Arc<mde_kdc::transport::KdcHost>` for the daemon's lifetime.
//! The mesh-router worker (KDC2-1.8) pulls the KdcHost out of
//! the registry to dispatch through it; the future D-Bus host
//! (KDC2-3.3) does the same.
//!
//! This commit ships the worker shell: Worker trait impl, idle
//! tick loop that surfaces the paired-device count for
//! healthz/instrumentation, shutdown plumbing identical to
//! `lan_discovery`. The real reconcile work (accept incoming
//! KDC connections, drive outgoing handshakes) lands with
//! KDC2-3.2.a alongside the TLS layer (KDC2-2.8).

#![cfg(feature = "async-services")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mde_kdc::pairing::{PairingError, PairingStore};
use mde_kdc::transport::KdcHost;
use tracing::{debug, error, info};

use super::{ShutdownToken, Worker};

/// Health-tick cadence. 30s is the same window
/// `lan_discovery` uses for its idle scan.
const TICK: Duration = Duration::from_secs(30);

/// Async worker that owns the KDC host objects.
pub struct KdcHostWorker {
    config_dir: PathBuf,
    host: Option<Arc<KdcHost>>,
}

impl KdcHostWorker {
    /// Construct with the on-disk config directory. The host
    /// itself is constructed lazily inside `run()` so a failed
    /// keygen / load doesn't abort the daemon startup — the
    /// supervisor sees a worker error + restarts according to
    /// `restart_policy`.
    #[must_use]
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            host: None,
        }
    }

    /// Open the on-disk pairing store + construct the KdcHost.
    /// Pure helper so the run loop can re-init after a restart.
    fn init_host(&mut self) -> Result<(), PairingError> {
        let store = PairingStore::open_or_init(&self.config_dir)?;
        let store_arc = Arc::new(store);
        self.host = Some(Arc::new(KdcHost::new(store_arc)));
        Ok(())
    }

    /// Borrow the live KdcHost. `None` before `init_host()` runs
    /// or after a fatal error. Exposed for the future
    /// mesh_router registration (KDC2-1.8 → 3.10 bridge) +
    /// tests.
    #[must_use]
    pub fn host(&self) -> Option<&Arc<KdcHost>> {
        self.host.as_ref()
    }
}

#[async_trait::async_trait]
impl Worker for KdcHostWorker {
    fn name(&self) -> &'static str {
        "kdc-host"
    }

    async fn run(&mut self, mut shutdown: ShutdownToken) -> anyhow::Result<()> {
        // Lazy init the host. On failure, surface to the
        // supervisor so the restart policy can act.
        if self.host.is_none() {
            self.init_host().map_err(|e| {
                error!(error = %e, "kdc-host: pairing store init failed");
                anyhow::anyhow!("kdc-host init failed: {e}")
            })?;
        }
        info!(
            config_dir = %self.config_dir.display(),
            "kdc-host: started",
        );

        let mut interval = tokio::time::interval(TICK);
        // First tick fires immediately; skip it so we don't
        // double-log "started" + "tick" at startup.
        interval.tick().await;

        loop {
            tokio::select! {
                _ = shutdown.wait() => {
                    info!("kdc-host: shutdown requested; exiting");
                    return Ok(());
                }
                _ = interval.tick() => {
                    debug!(
                        "kdc-host: tick (idle; real reconcile lands in KDC2-3.2.a)",
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn worker_name_matches_module() {
        let w = KdcHostWorker::new(PathBuf::from("/tmp"));
        assert_eq!(w.name(), "kdc-host");
    }

    #[test]
    fn host_is_none_before_init() {
        let w = KdcHostWorker::new(PathBuf::from("/tmp/never-touched"));
        assert!(w.host().is_none());
    }

    #[test]
    fn init_host_populates_the_arc() {
        let tmp = tempdir().unwrap();
        let mut w = KdcHostWorker::new(tmp.path().to_path_buf());
        w.init_host().unwrap();
        assert!(w.host().is_some());
        // The store created an identity.pem.
        assert!(tmp.path().join("identity.pem").exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn worker_exits_on_shutdown_request() {
        let tmp = tempdir().unwrap();
        let mut w = KdcHostWorker::new(tmp.path().to_path_buf());
        let (tx, rx) = tokio::sync::watch::channel(false);
        let token = super::super::ShutdownToken::from_receiver(rx);

        let handle = tokio::spawn(async move { w.run(token).await });
        tx.send(true).expect("shutdown channel intact");
        let result = handle.await.expect("worker join");
        assert!(result.is_ok(), "worker must exit Ok on shutdown");
        // identity.pem was created during init.
        assert!(tmp.path().join("identity.pem").exists());
    }
}
