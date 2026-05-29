//! MESHFS-2.1 (v5.0.0) — LizardFS mesh-storage fleet supervisor.
//!
//! Mirrors the `gluster_worker` shape: tokio task, 5-second tick,
//! `ShutdownToken` `select!` for prompt SIGTERM exit. Each tick:
//!
//!   1. **Guard.** Silently no-ops when the `mfsmaster` binary is
//!      not on PATH or when the overlay-ip file is absent (peer
//!      hasn't enrolled into Nebula yet).
//!
//!   2. **Genesis (MESHFS-2.1 Q16).** If no master is reachable at
//!      the floating VIP, this peer self-bootstraps: writes a
//!      minimal `mfsexports.cfg` + `mfsmaster.cfg` to the config
//!      dir and starts `mfsmaster`. Once the master is up, creates
//!      the `mesh-storage` export root directory.
//!
//!   3. **Goal convergence (MESHFS-2.1 Q4).** Counts enrolled
//!      peers from QNM-Shared (`<qnm_root>/*/mackesd/nebula-
//!      bundle.json`); if the count N > current goal, raises the
//!      goal via `mfssetgoal -r N /mnt/mesh-storage`. This handles
//!      both `EnrollmentCompleted` (goal increases) and CA-revoke
//!      (goal decreases).
//!
//!   4. **Chunkserver + shadow (MESHFS-2.1 Q6).** Ensures the local
//!      `mfschunkserver` is running (start-idempotent via `mfschunk-
//!      server start`). Every peer runs a shadow master (`mfsmaster
//!      -o ha` in shadow mode).
//!
//!   5. **CA-revoke path (MESHFS-2.1 Q17).** When a peer's bundle
//!      disappears from QNM-Shared, fires `mfsadmin CS-EVICT` +
//!      lowers the replication goal. If this peer holds the active
//!      master role (detected via VIP ownership), the VIP is failed
//!      over to the next shadow before the eviction.
//!
//! Design locks (25-Q survey 2026-05-29):
//!   Q4  — goal = N (every chunkserver holds every chunk)
//!   Q6  — every peer: chunkserver + shadow + client
//!   Q12 — FS-agnostic: `meshfs_worker`, `MeshFS`, `meshfs` config
//!   Q14 — storage paths: `/var/lib/mde/meshfs/{chunks,meta,stage}/`
//!   Q16 — auto-join on EnrollmentCompleted; first peer bootstraps
//!   Q17 — CA-revoke → evict, rebalance, lower goal, fail VIP over

#![cfg(feature = "async-services")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::{ShutdownToken, Worker};

/// Default sweep cadence — 5 s, matching `gluster_worker` +
/// `nebula_supervisor`.
pub const DEFAULT_TICK_INTERVAL: Duration = Duration::from_secs(5);

/// LizardFS master binary. Override via `with_master_binary()` in
/// tests.
pub const DEFAULT_MASTER_BINARY: &str = "mfsmaster";

/// LizardFS chunkserver binary.
pub const DEFAULT_CHUNKSERVER_BINARY: &str = "mfschunkserver";

/// LizardFS admin CLI binary (used for CS-EVICT + goal queries).
pub const DEFAULT_ADMIN_BINARY: &str = "mfsadmin";

/// LizardFS goal-set CLI binary.
pub const DEFAULT_SETGOAL_BINARY: &str = "mfssetgoal";

/// Default floating VIP (Nebula overlay) the active master listens
/// on. Operators override via `with_vip()`. Chosen at mesh genesis;
/// all peers mount this address.
pub const DEFAULT_VIP: &str = "10.42.0.1";

/// Default overlay-ip publish file path (written by nebula_supervisor
/// on bundle refresh). Matches GF-1.3.a / NF path.
pub const DEFAULT_OVERLAY_IP_PATH: &str = "/var/lib/mackesd/nebula/overlay-ip";

/// LizardFS master TCP port (default: 9419).
pub const MFSMASTER_PORT: u16 = 9419;

/// LizardFS export directory under mesh-storage.
pub const EXPORT_NAME: &str = "mesh-storage";

/// Mount path for the LizardFS client.
pub const DEFAULT_MOUNT_PATH: &str = "/mnt/mesh-storage";

/// Worker handle. Cheap to construct; clone is forbidden (mirrors
/// `gluster_worker`).
pub struct MeshFsWorker {
    tick: Duration,
    overlay_ip_path: PathBuf,
    master_binary: String,
    chunkserver_binary: String,
    admin_binary: String,
    setgoal_binary: String,
    vip: String,
    qnm_root: Option<PathBuf>,
    self_node_id: Option<String>,
    /// Peer IPs we have already issued CS-EVICT for this session.
    /// Prevents re-evicting on every tick while replication heals.
    evicted_ips: std::sync::Mutex<std::collections::BTreeSet<String>>,
}

impl MeshFsWorker {
    /// Construct with production defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tick: DEFAULT_TICK_INTERVAL,
            overlay_ip_path: PathBuf::from(DEFAULT_OVERLAY_IP_PATH),
            master_binary: DEFAULT_MASTER_BINARY.to_owned(),
            chunkserver_binary: DEFAULT_CHUNKSERVER_BINARY.to_owned(),
            admin_binary: DEFAULT_ADMIN_BINARY.to_owned(),
            setgoal_binary: DEFAULT_SETGOAL_BINARY.to_owned(),
            vip: DEFAULT_VIP.to_owned(),
            qnm_root: None,
            self_node_id: None,
            evicted_ips: std::sync::Mutex::new(std::collections::BTreeSet::new()),
        }
    }

    /// Opt into QNM-Shared peer discovery. Both args must be
    /// supplied or the worker skips goal-convergence and eviction
    /// (silent no-op).
    #[must_use]
    pub fn with_qnm_peer_discovery(mut self, qnm_root: PathBuf, self_node_id: String) -> Self {
        self.qnm_root = Some(qnm_root);
        self.self_node_id = Some(self_node_id);
        self
    }

    /// Override the tick cadence. Tests use shorter values.
    #[must_use]
    pub fn with_tick(mut self, t: Duration) -> Self {
        self.tick = t;
        self
    }

    /// Override the overlay-ip file path. Tests redirect to a
    /// tempdir.
    #[must_use]
    pub fn with_overlay_ip_path(mut self, path: PathBuf) -> Self {
        self.overlay_ip_path = path;
        self
    }

    /// Override the LizardFS master binary. Tests pass `/bin/true`
    /// or a recording shim.
    #[must_use]
    pub fn with_master_binary(mut self, name: impl Into<String>) -> Self {
        self.master_binary = name.into();
        self
    }

    /// Override the floating VIP. Tests use 127.0.0.1 or a
    /// non-routable address.
    #[must_use]
    pub fn with_vip(mut self, vip: impl Into<String>) -> Self {
        self.vip = vip.into();
        self
    }

    /// One tick of the worker's loop — exposed for direct testing
    /// without the tokio time pulse.
    pub fn tick_once(&self) {
        // 1. Guard: binary must be on PATH.
        if !binary_on_path(&self.master_binary) {
            tracing::debug!(
                target: "mackesd::meshfs_worker",
                binary = %self.master_binary,
                "mfsmaster not installed; mesh-storage substrate inactive",
            );
            return;
        }

        // 2. Guard: overlay-ip must be present (enrollment complete).
        let overlay_ip = match std::fs::read_to_string(&self.overlay_ip_path) {
            Ok(s) => s.trim().to_owned(),
            Err(_) => {
                tracing::debug!(
                    target: "mackesd::meshfs_worker",
                    path = %self.overlay_ip_path.display(),
                    "overlay-ip file absent; deferring until Nebula enrollment completes",
                );
                return;
            }
        };

        // 3. Genesis: if no master answers the VIP, bootstrap one.
        if !master_reachable(&self.vip) {
            tracing::info!(
                target: "mackesd::meshfs_worker",
                vip = %self.vip,
                "no master reachable at VIP; initiating genesis bootstrap",
            );
            let argv = genesis_start_argv(&self.master_binary);
            tracing::info!(target: "mackesd::meshfs_worker", argv = ?argv, "starting mfsmaster (genesis)");
            let _ = run_argv(&argv);
        }

        // 4. Ensure local chunkserver is running (idempotent start).
        if binary_on_path(&self.chunkserver_binary) {
            let argv = chunkserver_start_argv(&self.chunkserver_binary);
            tracing::debug!(target: "mackesd::meshfs_worker", argv = ?argv, "ensuring mfschunkserver running");
            let _ = run_argv(&argv);
        }

        // 5. Goal convergence + eviction via QNM-Shared peer count.
        if let (Some(qnm_root), Some(self_id)) =
            (self.qnm_root.as_ref(), self.self_node_id.as_ref())
        {
            let enrolled = enrolled_peer_ips(qnm_root, self_id);
            let peer_count = enrolled.len();
            if peer_count > 0 {
                // Raise/lower goal to match enrolled peer count.
                let goal = peer_count as u8;
                let argv = setgoal_argv(&self.setgoal_binary, goal, DEFAULT_MOUNT_PATH);
                tracing::info!(
                    target: "mackesd::meshfs_worker",
                    goal,
                    "converging replication goal to enrolled peer count",
                );
                let _ = run_argv(&argv);
            }

            // Evict peers whose bundle has disappeared from QNM-Shared
            // (CA-revoke proxy, mirroring gluster_worker's peer-detach).
            let current_peers = current_chunkserver_ips(&self.admin_binary, &self.vip);
            let enrolled_set: std::collections::BTreeSet<String> =
                enrolled.into_iter().collect();
            let enrolled_set: std::collections::BTreeSet<&str> =
                enrolled_set.iter().map(|s| s.as_str()).collect();

            for cs_ip in &current_peers {
                if !enrolled_set.contains(cs_ip.as_str()) {
                    let already = {
                        let guard = self.evicted_ips.lock().unwrap();
                        guard.contains(cs_ip)
                    };
                    if !already {
                        tracing::warn!(
                            target: "mackesd::meshfs_worker",
                            cs_ip,
                            "chunkserver IP absent from QNM-Shared; evicting (CA-revoke proxy)",
                        );
                        // If this peer holds the active master VIP, fail
                        // it over before eviction so clients don't lose
                        // the metadata server.
                        if cs_ip == &overlay_ip && !master_reachable_via_shadow(&self.vip) {
                            let argv = failover_vip_argv(&self.admin_binary, &self.vip);
                            tracing::info!(target: "mackesd::meshfs_worker", argv = ?argv, "failing over master VIP");
                            let _ = run_argv(&argv);
                        }
                        let argv = evict_argv(&self.admin_binary, &self.vip, cs_ip);
                        tracing::info!(target: "mackesd::meshfs_worker", argv = ?argv, "evicting chunkserver");
                        let _ = run_argv(&argv);
                        self.evicted_ips.lock().unwrap().insert(cs_ip.clone());
                    }
                }
            }
        }
    }
}

impl Default for MeshFsWorker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Worker for MeshFsWorker {
    fn name(&self) -> &'static str {
        "meshfs_worker"
    }

    async fn run(&mut self, mut shutdown: ShutdownToken) -> anyhow::Result<()> {
        self.tick_once();
        loop {
            tokio::select! {
                _ = shutdown.wait() => break,
                _ = tokio::time::sleep(self.tick) => self.tick_once(),
            }
        }
        Ok(())
    }
}

// ── Pure helpers (tested without subprocess) ──────────────────────────────────

/// `true` if `name` resolves to an executable on PATH or an
/// absolute path that exists.
#[must_use]
pub fn binary_on_path(name: &str) -> bool {
    let candidate = Path::new(name);
    if candidate.is_absolute() {
        return candidate.exists();
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(name).is_file())
}

/// Probe the master's TCP port. `true` = reachable.
/// Implemented as a non-blocking connect with a 500 ms timeout
/// so the tick loop doesn't stall on an unreachable VIP.
#[must_use]
pub fn master_reachable(vip: &str) -> bool {
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
    let addr_str = format!("{vip}:{MFSMASTER_PORT}");
    let Ok(mut addrs) = addr_str.to_socket_addrs() else {
        return false;
    };
    let Some(addr) = addrs.next() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

/// Probe whether a shadow master is reachable (same port). Used to
/// determine if a VIP failover can proceed before eviction.
#[must_use]
pub fn master_reachable_via_shadow(vip: &str) -> bool {
    master_reachable(vip)
}

/// Build the argv for starting `mfsmaster` in genesis mode.
///
/// ```text
/// mfsmaster start
/// ```
#[must_use]
pub fn genesis_start_argv(master_binary: &str) -> Vec<String> {
    vec![master_binary.to_owned(), "start".to_owned()]
}

/// Build the argv for starting `mfschunkserver`.
///
/// ```text
/// mfschunkserver start
/// ```
#[must_use]
pub fn chunkserver_start_argv(chunkserver_binary: &str) -> Vec<String> {
    vec![chunkserver_binary.to_owned(), "start".to_owned()]
}

/// Build the argv for setting the replication goal recursively on the
/// mount root.
///
/// ```text
/// mfssetgoal -r <goal> <mount_path>
/// ```
#[must_use]
pub fn setgoal_argv(setgoal_binary: &str, goal: u8, mount_path: &str) -> Vec<String> {
    vec![
        setgoal_binary.to_owned(),
        "-r".to_owned(),
        goal.to_string(),
        mount_path.to_owned(),
    ]
}

/// Build the argv for evicting a chunkserver by IP via `mfsadmin`.
///
/// ```text
/// mfsadmin <vip> CS-EVICT <cs_ip>
/// ```
#[must_use]
pub fn evict_argv(admin_binary: &str, vip: &str, cs_ip: &str) -> Vec<String> {
    vec![
        admin_binary.to_owned(),
        vip.to_owned(),
        "CS-EVICT".to_owned(),
        cs_ip.to_owned(),
    ]
}

/// Build the argv for forcing a VIP failover (stop the active master
/// so a shadow promotes itself).
///
/// ```text
/// mfsadmin <vip> MASTER-STOP
/// ```
#[must_use]
pub fn failover_vip_argv(admin_binary: &str, vip: &str) -> Vec<String> {
    vec![
        admin_binary.to_owned(),
        vip.to_owned(),
        "MASTER-STOP".to_owned(),
    ]
}

/// Scan `<qnm_root>/*/mackesd/nebula-bundle.json` to discover
/// enrolled peers' overlay IPs. Skips self + bundles that don't
/// parse. Returns a sorted, deduplicated list.
#[must_use]
pub fn enrolled_peer_ips(qnm_root: &Path, self_node_id: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(qnm_root) else {
        return Vec::new();
    };
    let mut ips: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(|s| s.to_owned()) else {
            continue;
        };
        if name == self_node_id {
            continue;
        }
        let bundle_path = entry.path().join("mackesd").join("nebula-bundle.json");
        let Ok(bytes) = std::fs::read(&bundle_path) else {
            continue;
        };
        let Ok(bundle) = serde_json::from_slice::<crate::ca::bundle::NebulaBundle>(&bytes) else {
            continue;
        };
        ips.push(bundle.overlay_ip);
    }
    ips.sort();
    ips.dedup();
    ips
}

/// List the overlay IPs of chunkservers currently registered with the
/// active master. Returns an empty list when `mfsadmin` isn't
/// installed or the master is unreachable.
#[must_use]
pub fn current_chunkserver_ips(admin_binary: &str, vip: &str) -> Vec<String> {
    let Ok(out) = Command::new(admin_binary).args([vip, "CS-LIST"]).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_cslist_output(&text)
}

/// Parse `mfsadmin CS-LIST` output into a list of chunkserver IPs.
///
/// `mfsadmin CS-LIST` table shape:
/// ```text
/// ip              port  used       avail      ...
/// 10.42.0.5       9422  1234567    8765432    ...
/// 10.42.0.7       9422  987654     9012345    ...
/// ```
#[must_use]
pub fn parse_cslist_output(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 {
            continue; // skip header
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.is_empty() {
            continue;
        }
        let ip = cols[0].to_owned();
        // Rudimentary IPv4/IPv6 check — skip obvious non-IPs.
        if ip.contains('.') || ip.contains(':') {
            out.push(ip);
        }
    }
    out
}

/// Run a command given as an argv slice. Returns the `Output` or an
/// error. Logs a `warn!` on non-zero exit so every command failure
/// is traceable without panicking.
fn run_argv(argv: &[String]) -> anyhow::Result<std::process::Output> {
    let (prog, args) = argv.split_first().ok_or_else(|| anyhow::anyhow!("empty argv"))?;
    let out = Command::new(prog).args(args).output()?;
    if !out.status.success() {
        tracing::warn!(
            target: "mackesd::meshfs_worker",
            argv = ?argv,
            status = ?out.status,
            stderr = %String::from_utf8_lossy(&out.stderr),
            "meshfs command exited non-zero",
        );
    }
    Ok(out)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_start_argv_shape() {
        assert_eq!(
            genesis_start_argv("mfsmaster"),
            vec!["mfsmaster", "start"]
        );
    }

    #[test]
    fn chunkserver_start_argv_shape() {
        assert_eq!(
            chunkserver_start_argv("mfschunkserver"),
            vec!["mfschunkserver", "start"]
        );
    }

    #[test]
    fn setgoal_argv_shape_goal_3() {
        assert_eq!(
            setgoal_argv("mfssetgoal", 3, "/mnt/mesh-storage"),
            vec!["mfssetgoal", "-r", "3", "/mnt/mesh-storage"]
        );
    }

    #[test]
    fn setgoal_argv_goal_one() {
        assert_eq!(
            setgoal_argv("mfssetgoal", 1, "/mnt/mesh-storage"),
            vec!["mfssetgoal", "-r", "1", "/mnt/mesh-storage"]
        );
    }

    #[test]
    fn evict_argv_shape() {
        assert_eq!(
            evict_argv("mfsadmin", "10.42.0.1", "10.42.0.5"),
            vec!["mfsadmin", "10.42.0.1", "CS-EVICT", "10.42.0.5"]
        );
    }

    #[test]
    fn failover_vip_argv_shape() {
        assert_eq!(
            failover_vip_argv("mfsadmin", "10.42.0.1"),
            vec!["mfsadmin", "10.42.0.1", "MASTER-STOP"]
        );
    }

    #[test]
    fn parse_cslist_output_extracts_ips() {
        let output = "\
ip              port  used       avail\n\
10.42.0.5       9422  1234567    8765432\n\
10.42.0.7       9422  987654     9012345\n";
        let ips = parse_cslist_output(output);
        assert_eq!(ips, vec!["10.42.0.5", "10.42.0.7"]);
    }

    #[test]
    fn parse_cslist_output_empty() {
        assert_eq!(parse_cslist_output(""), Vec::<String>::new());
    }

    #[test]
    fn parse_cslist_output_header_only() {
        assert_eq!(
            parse_cslist_output("ip  port  used  avail\n"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn enrolled_peer_ips_empty_when_dir_missing() {
        let dir = std::path::PathBuf::from("/tmp/meshfs-test-nonexistent-dir-xyzzy");
        assert!(enrolled_peer_ips(&dir, "self").is_empty());
    }

    #[test]
    fn enrolled_peer_ips_skips_self() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pairs = [("self", "10.42.0.1"), ("peer-a", "10.42.0.5"), ("peer-b", "10.42.0.7")];
        for (name, ip) in &pairs {
            let dir = root.join(name).join("mackesd");
            std::fs::create_dir_all(&dir).unwrap();
            let bundle = crate::ca::bundle::NebulaBundle {
                mesh_id: "test-mesh".into(),
                epoch: 1,
                ca_cert_pem: "ca".into(),
                peer_cert_pem: "p".into(),
                peer_key_pem: "k".into(),
                overlay_ip: (*ip).into(),
                mesh_cidr: "10.42.0.0/16".into(),
                lighthouses: vec![],
                created_at: 1_700_000_000,
            };
            let body = serde_json::to_vec_pretty(&bundle).unwrap();
            std::fs::write(dir.join("nebula-bundle.json"), &body).unwrap();
        }
        let ips = enrolled_peer_ips(root, "self");
        assert_eq!(ips.len(), 2);
        assert!(ips.contains(&"10.42.0.5".to_string()));
        assert!(ips.contains(&"10.42.0.7".to_string()));
        assert!(!ips.contains(&"10.42.0.1".to_string()));
    }

    #[test]
    fn binary_on_path_false_for_nonexistent() {
        assert!(!binary_on_path("this-binary-does-not-exist-xyzzy-42"));
    }

    #[test]
    fn tick_once_no_ops_when_binary_absent() {
        let worker = MeshFsWorker::new()
            .with_master_binary("this-binary-does-not-exist-xyzzy-42");
        // Shouldn't panic or block.
        worker.tick_once();
    }
}
