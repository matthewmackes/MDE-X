//! v2.0.0 Phase B.7 — KDE Connect bridge worker.
//!
//! Reparents the existing `crates/mackes-kdc/` crate (Phase 13.1.2)
//! under the unified `workers/` namespace so the supervisor (Phase
//! A.2) can register and restart it alongside every other Phase B
//! task. The standalone `mackesd-kdc-bridge.service` systemd unit
//! retires in favor of the in-process pattern.
//!
//! Today the bridge's live network surface (mDNS announce, connection
//! forwarding) lives outside this module — `mackes_kdc` exposes the
//! typed value model plus the `paired_device_ids()` scanner. This
//! worker drives the scanner on a cadence so the daemon keeps a
//! fresh view of paired devices in memory; richer network behavior
//! lands as the upstream mackes-kdc crate fills in.

#![cfg(feature = "async-services")]

use std::time::Duration;

use super::{ShutdownToken, Worker};

/// Cadence at which the worker re-scans `~/.config/kdeconnect/` for
/// paired-device changes. Slower than the panel's UI refresh because
/// pairings change rarely; the dashboard polls more frequently via
/// the in-process `paired_device_ids()` call.
const SCAN_INTERVAL_S: u64 = 30;

/// Async worker that wraps the mackes-kdc bridge.
pub struct KdcBridgeWorker {
    last_seen: Vec<String>,
}

impl KdcBridgeWorker {
    /// Construct a fresh worker. Initial pass populates `last_seen`
    /// from the current pairing directory.
    #[must_use]
    pub fn new() -> Self {
        Self { last_seen: mackes_kdc::paired_device_ids() }
    }
}

impl Default for KdcBridgeWorker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Worker for KdcBridgeWorker {
    fn name(&self) -> &'static str {
        "kdc-bridge"
    }

    async fn run(&mut self, mut shutdown: ShutdownToken) -> anyhow::Result<()> {
        loop {
            tokio::select! {
                _ = shutdown.wait() => return Ok(()),
                _ = tokio::time::sleep(Duration::from_secs(SCAN_INTERVAL_S)) => {}
            }
            let current = mackes_kdc::paired_device_ids();
            let diff = device_diff(&self.last_seen, &current);
            if !diff.is_empty() {
                tracing::info!(
                    paired = current.len(),
                    "kdc-bridge: pairing set changed, diff: {diff:?}"
                );
                self.last_seen = current;
            }
        }
    }
}

/// Pure helper: compute the set difference between two pairing
/// snapshots. Returns `(added, removed)` tuples so callers can log
/// or react. Order is preserved per-list.
#[must_use]
pub fn device_diff(prior: &[String], current: &[String]) -> Vec<(String, &'static str)> {
    use std::collections::BTreeSet;
    let prior_set: BTreeSet<&str> = prior.iter().map(String::as_str).collect();
    let current_set: BTreeSet<&str> = current.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for added in current_set.difference(&prior_set) {
        out.push(((*added).to_owned(), "added"));
    }
    for removed in prior_set.difference(&current_set) {
        out.push(((*removed).to_owned(), "removed"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_diff_returns_empty_when_unchanged() {
        let a = vec!["uuid-1".to_owned(), "uuid-2".to_owned()];
        let b = a.clone();
        assert!(device_diff(&a, &b).is_empty());
    }

    #[test]
    fn device_diff_reports_added() {
        let prior = vec!["uuid-1".to_owned()];
        let current = vec!["uuid-1".to_owned(), "uuid-2".to_owned()];
        let diff = device_diff(&prior, &current);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0], ("uuid-2".to_owned(), "added"));
    }

    #[test]
    fn device_diff_reports_removed() {
        let prior = vec!["uuid-1".to_owned(), "uuid-2".to_owned()];
        let current = vec!["uuid-1".to_owned()];
        let diff = device_diff(&prior, &current);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0], ("uuid-2".to_owned(), "removed"));
    }

    #[test]
    fn device_diff_reports_both_added_and_removed() {
        let prior = vec!["uuid-old".to_owned()];
        let current = vec!["uuid-new".to_owned()];
        let diff = device_diff(&prior, &current);
        // Order: added before removed (BTreeSet difference ordering).
        assert_eq!(diff.len(), 2);
        assert!(diff.iter().any(|(id, op)| id == "uuid-new" && *op == "added"));
        assert!(diff.iter().any(|(id, op)| id == "uuid-old" && *op == "removed"));
    }

    #[tokio::test]
    async fn kdc_bridge_worker_name_matches_phase_b_lock() {
        let w = KdcBridgeWorker::new();
        assert_eq!(w.name(), "kdc-bridge");
    }

    #[tokio::test]
    async fn kdc_bridge_worker_exits_on_shutdown_token() {
        let mut w = KdcBridgeWorker::new();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let token = ShutdownToken::from_receiver(rx);
        let _ = tx.send(true);
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            w.run(token),
        )
        .await
        .expect("worker must exit on shutdown");
        assert!(result.is_ok());
    }
}
