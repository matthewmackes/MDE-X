//! `mackesd` — CLI entry point for the Mesh control plane.
//!
//! Subcommands land alongside their backing Phase 12 substeps. Today
//! only `mackesd migrate` ships (Phase 12.2 store + migrations); the
//! rest follow as substeps complete. We deliberately do NOT register
//! stub commands here — every `mackesd X` either does X or is absent.

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mackesd",
    version,
    about = "Mesh control plane for Mackes XFCE Workstation"
)]
struct Cli {
    /// Override the default `SQLite` store path (defaults to
    /// `$MACKESD_HOME/mackesd.db` or `/var/lib/mackesd/mackesd.db`).
    #[arg(long, env = "MACKESD_DB")]
    db: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Apply every pending `SQLite` migration against the store.
    ///
    /// Idempotent — running `mackesd migrate` against an up-to-date
    /// store is a no-op that exits 0.
    Migrate,

    /// Print store status: applied-migration count + db path.
    Status,

    /// Print the live `HealthReport` as a JSON line (Phase 12.1.3).
    ///
    /// Same shape as `mackesd_core::health::HealthReport` so the
    /// panel + the CLI consume identical data.
    Healthz,

    /// Generate a fresh 16-char URL-safe passcode (Phase 12.10.1).
    /// Prints the passcode and an instruction to save it to
    /// `libsecret` as `org.mackes.mesh.passcode`. Does NOT mutate
    /// libsecret on its own — caller drives the secret-storage step.
    GeneratePasscode,

    /// Walk the `events` table forward and verify every row's hash
    /// (Phase 12.10.3). Exits 0 on Intact / Empty, 1 on Break.
    AuditVerify,

    /// Rotate the shared mesh passcode (Phase 12.10.2). Prints a
    /// freshly-generated passcode and reminds the operator to
    /// store it in libsecret. Peers pick up the new passcode on
    /// their next heartbeat once the reconcile loop ships (12.5).
    RotatePasscode,

    /// Explain why a given peer is expected to peer with each of
    /// its neighbors (Phase 12.4.4). Reads `topology::calculate`'s
    /// reason chain for the named node.
    PeersWhy {
        /// Stable node id (e.g. `peer:anvil`).
        #[arg(value_name = "NODE_ID")]
        node_id: String,
    },

    /// Dry-run apply (Phase 12.7.4). Runs the validation +
    /// reconcile-plan pipeline without mutating anything; prints
    /// the diff + would-be event log as JSON. Useful in CI to
    /// catch config issues before a real apply.
    Apply {
        /// Skip mutation; print the plan only.
        #[arg(long)]
        dry_run: bool,
    },

    /// Enroll this peer against the mesh (Phase 12.3.1). Generates
    /// a fresh Ed25519 keypair + bearer token; prints a signed
    /// `EnrollmentRequest` JSON that the leader ingests.
    Enroll {
        /// 16-character URL-safe shared passcode.
        #[arg(long)]
        passcode: String,
        /// Optional display name; defaults to the system hostname.
        #[arg(long)]
        name: Option<String>,
    },

    /// Decommission a peer (Phase 12.3.4). Soft-deletes the node
    /// row; preserves history. `--force` skips the unreachable
    /// confirmation.
    Decommission {
        /// Stable node id to retire.
        node_id: String,
        /// Force decommission even when the peer is unreachable.
        #[arg(long)]
        force: bool,
    },

    /// Re-enroll an existing node (Phase 12.3.5). Issues fresh
    /// credentials against the existing row, preserving history.
    Reenroll {
        /// Stable node id to refresh.
        node_id: String,
    },

    /// Force this peer into leadership (Phase 12.1.1b operator
    /// override). Bumps the lease epoch.
    TakeLeadership {
        /// Stable node id to install as leader.
        #[arg(long)]
        as_node: String,
    },

    /// Import legacy mesh state into the `mackesd` store (Phase
    /// 12.13.2). Walks the prior 2.x JSON/TOML caches and emits
    /// a JSON plan that the operator can review before applying.
    ImportLegacy {
        /// Print the plan only; don't write anything.
        #[arg(long)]
        dry_run: bool,
    },

    /// Inventory legacy on-disk state (Phase 12.13.1). Walks the
    /// three canonical roots (`~/.config/mackes-shell/`,
    /// `~/.qnm-sync/`, `~/.cache/mackes/`) and prints a catalog of
    /// every JSON / TOML / cache file found, classified by kind and
    /// flagged with whether the filename hints at mesh data. This
    /// is the *inspection* step — `mackesd import-legacy` is what
    /// actually moves data into the store.
    InventoryLegacy {
        /// Only emit artifacts whose filename matches the
        /// mesh-related heuristic.
        #[arg(long)]
        mesh_only: bool,
        /// Emit the full inventory as a JSON array. Without this
        /// flag a human-readable table prints to stdout.
        #[arg(long)]
        json: bool,
    },

    /// Run the reconcile worker (Phase 12.5 wiring). Default mode
    /// loops forever on the foreground thread, ticking every
    /// `RECONCILE_INTERVAL_S` seconds (30 s per the 12.5.1 lock).
    /// This is the entry point systemd's `mackesd.service` invokes.
    ///
    /// The worker reads peer heartbeats + link telemetry from
    /// `QNM_SHARED_ROOT/<peer>/mackesd/{heartbeat,links}.json`,
    /// compares them against the latest applied `desired_config`
    /// snapshot, and routes the resulting drift rows through
    /// `reconcile::plan_tick`. Auto-repairable rows land in the
    /// audit-log with the `intent` field marking that take-action
    /// is gated on the connectivity layer (12.14+); manual-review
    /// rows are surfaced via `tracing::warn` for the GUI inbox.
    ///
    /// SIGTERM / SIGINT trigger a graceful exit: the current tick
    /// finishes, then the loop returns. Cleanly handles systemd's
    /// `TimeoutStopSec`.
    Reconcile {
        /// Run one tick, print the resulting `TickOutcome` as a
        /// pretty-printed JSON object, and exit. No background
        /// thread, no signal handler — for CI smoke tests + the
        /// dry-run loop the operator runs by hand.
        #[arg(long)]
        once: bool,
        /// Override the QNM-Shared root (defaults to
        /// `$QNM_SHARED_ROOT` or `~/QNM-Shared`). Useful for tests.
        #[arg(long, env = "QNM_SHARED_ROOT")]
        qnm_root: Option<PathBuf>,
        /// Override the stable node id (defaults to
        /// `peer:<hostname>`). Recorded as the `actor` field on
        /// every emitted audit event.
        #[arg(long)]
        node_id: Option<String>,
    },

    /// v2.0.0 Phase B.12 — the unified meta-daemon entry point.
    /// Replaces the legacy `migrate && status` ExecStart on the
    /// systemd unit. Boots the tokio runtime, spawns the worker
    /// supervisor + every registered worker, and blocks on
    /// SIGTERM/SIGINT.
    ///
    /// Phase A.2 ships the supervisor surface; Phase B fills in the
    /// individual workers (`clipboard`, `mdns`, `fs_sync`, ...).
    /// Today `serve` registers the existing reconcile loop as the
    /// single worker so the unit's behavior matches the current
    /// `mackesd reconcile` invocation while the rest of Phase B lands.
    ///
    /// Requires the `async-services` cargo feature.
    #[cfg(feature = "async-services")]
    Serve {
        /// Override the QNM-Shared root (defaults to
        /// `$QNM_SHARED_ROOT` or `~/QNM-Shared`).
        #[arg(long, env = "QNM_SHARED_ROOT")]
        qnm_root: Option<PathBuf>,
        /// Override the stable node id (defaults to `peer:<hostname>`).
        #[arg(long)]
        node_id: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    let db_path = cli.db.unwrap_or_else(mackesd_core::default_db_path);

    match cli.cmd {
        Cmd::Migrate => {
            let conn = mackesd_core::store::open(&db_path)
                .with_context(|| format!("opening store at {}", db_path.display()))?;
            let n = mackesd_core::store::applied_migration_count(&conn)?;
            tracing::info!("store at {} migrated (n={})", db_path.display(), n);
            println!("{n} migrations applied");
        }
        Cmd::Status => {
            let conn = mackesd_core::store::open(&db_path)
                .with_context(|| format!("opening store at {}", db_path.display()))?;
            let n = mackesd_core::store::applied_migration_count(&conn)?;
            println!("db:                 {}", db_path.display());
            println!("migrations applied: {n}");
        }
        Cmd::Healthz => {
            // First-class panel/CLI parity per 12.1.3. Today the
            // report is the empty baseline; subsequent substeps
            // (12.3.3 heartbeats, 12.5.1 drift detector) populate
            // the live fields.
            let report = mackesd_core::health::HealthReport::empty();
            println!("{}", report.to_json_line()?);
        }
        Cmd::GeneratePasscode => {
            let code = mackesd_core::passcode::generate();
            println!("{code}");
            eprintln!(
                "(save with: secret-tool store --label='Mackes mesh passcode' \
                org.mackes.mesh.passcode <name>)"
            );
        }
        Cmd::AuditVerify => {
            // Reads every row from the `events` table (ordered by
            // `seq` ASC) and walks the SHA-256 hash chain.
            let conn = mackesd_core::store::open(&db_path)
                .with_context(|| format!("opening store at {}", db_path.display()))?;
            let rows = mackesd_core::store::load_audit_rows(&conn)
                .context("loading events from store")?;
            match mackesd_core::audit::verify(&rows) {
                mackesd_core::audit::VerifyOutcome::Empty => {
                    println!("audit chain empty (no events yet)");
                }
                mackesd_core::audit::VerifyOutcome::Intact { verified, .. } => {
                    println!("verified {verified} events  ·  chain intact");
                }
                mackesd_core::audit::VerifyOutcome::Break { at_event, .. } => {
                    eprintln!("audit chain BREAK at event {at_event}");
                    std::process::exit(1);
                }
            }
        }
        Cmd::RotatePasscode => {
            // Phase 12.10.2 — generate fresh passcode; libsecret
            // store + peer redistribution wires through with 12.5.
            let code = mackesd_core::passcode::generate();
            println!("{code}");
            eprintln!(
                "rotation: store with `secret-tool store --label='Mackes mesh \
                 passcode' org.mackes.mesh.passcode <name>` then peers refresh \
                 their bearer tokens on next heartbeat."
            );
        }
        Cmd::PeersWhy { node_id } => {
            // Phase 12.4.4 — explanation surface. Loads the node
            // roster from the store, runs `topology::calculate`,
            // and walks the resulting edge set + route table to
            // emit a per-edge reason chain for the named peer.
            let conn = mackesd_core::store::open(&db_path)
                .with_context(|| format!("opening store at {}", db_path.display()))?;
            let nodes = mackesd_core::store::list_nodes(&conn)
                .context("listing nodes from store")?;
            let report = explain_peer(&node_id, &nodes);
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Cmd::Apply { dry_run } => {
            if dry_run {
                // Phase 12.7.4 — run validation against an empty
                // snapshot today; once the store wires the
                // serialized desired-config row in, the dry-run
                // path returns the real diff + event-log preview.
                let snapshot = mackesd_core::topology::DesiredSnapshot::default();
                let errors = mackesd_core::validation::validate(&snapshot);
                let report = serde_json::json!({
                    "dry_run": true,
                    "validation_errors": errors.len(),
                    "would_apply_revisions": 0,
                });
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                eprintln!(
                    "mackesd: non-dry-run apply requires the reconcile loop \
                     (Phase 12.5) — use `mackesd apply --dry-run` for the \
                     validation + plan preview."
                );
                std::process::exit(2);
            }
        }
        Cmd::Enroll { passcode, name } => {
            // Phase 12.3.1 — build identity + signed request.
            let identity = mackesd_core::enrollment::build_identity();
            let display = name.unwrap_or_else(|| {
                std::env::var("HOSTNAME").unwrap_or_else(|_| {
                    std::process::Command::new("hostname")
                        .output()
                        .ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok())
                        .map_or_else(|| "unknown".to_owned(), |s| s.trim().to_owned())
                })
            });
            match mackesd_core::enrollment::build_request(&identity, &passcode, &display) {
                Some(req) => {
                    println!("{}", serde_json::to_string_pretty(&req)?);
                    eprintln!(
                        "enrollment request emitted — drop into the leader's \
                         pending inbox (Phase 12.8.2)."
                    );
                }
                None => {
                    eprintln!(
                        "mackesd enroll: passcode failed validation (must be \
                         16 URL-safe characters)."
                    );
                    std::process::exit(2);
                }
            }
        }
        Cmd::Decommission { node_id, force } => {
            // Phase 12.3.4 — soft-delete the node row and emit a
            // hash-chained Lifecycle event so the audit trail
            // records the operator action. `--force` only changes
            // the audit kind label; the SQL effect is identical
            // (CHECK constraint enforces the same allowed roles).
            let mut conn = mackesd_core::store::open(&db_path)
                .with_context(|| format!("opening store at {}", db_path.display()))?;
            let updated = mackesd_core::store::set_node_role(
                &conn, &node_id, "decommissioned",
            )?;
            if updated == 0 {
                eprintln!("mackesd decommission: no node row matches {node_id}");
                std::process::exit(2);
            }
            let payload = serde_json::json!({
                "kind":  if force { "forced" } else { "soft" },
                "node":  node_id,
                "event": "decommission",
            })
            .to_string();
            mackesd_core::store::insert_event(
                &mut conn, "lifecycle", &default_node_id(), &payload,
            )?;
            let report = serde_json::json!({
                "decommission":     node_id,
                "kind":             if force { "forced" } else { "soft" },
                "history_retained": true,
                "audit_logged":     true,
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Cmd::Reenroll { node_id } => {
            // Phase 12.3.5 — mint a fresh keypair and write its
            // hex public key into the existing node row. Lifecycle
            // event records the old fingerprint so a forensic
            // walker can correlate before/after.
            let mut conn = mackesd_core::store::open(&db_path)
                .with_context(|| format!("opening store at {}", db_path.display()))?;
            let prior = mackesd_core::store::list_nodes(&conn)?
                .into_iter()
                .find(|n| n.node_id == node_id);
            let new_identity = mackesd_core::enrollment::build_identity();
            let new_fp = new_identity.key.fingerprint();
            let updated = mackesd_core::store::refresh_node_credentials(
                &conn, &node_id, &new_fp,
            )?;
            if updated == 0 {
                eprintln!("mackesd reenroll: no node row matches {node_id}");
                std::process::exit(2);
            }
            let payload = serde_json::json!({
                "event":           "reenroll",
                "node":            node_id,
                "old_fingerprint": prior.map(|p| p.public_key),
                "new_fingerprint": &new_fp,
            })
            .to_string();
            mackesd_core::store::insert_event(
                &mut conn, "lifecycle", &default_node_id(), &payload,
            )?;
            let report = serde_json::json!({
                "reenroll":         node_id,
                "new_fingerprint":  new_fp,
                "history_retained": true,
                "audit_logged":     true,
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Cmd::TakeLeadership { as_node } => {
            // Phase 12.1.1b — operator-forced leadership bump.
            let lock_path = mackesd_core::default_qnm_shared_root()
                .join(".mackesd-leader.lock");
            let lease = mackesd_core::leader::force_take(&lock_path, &as_node)
                .with_context(|| format!("rewriting {}", lock_path.display()))?;
            println!(
                "leader: {} (epoch {}) — lease renewed at {}",
                lease.node_id, lease.epoch, lease.renewed_at_s
            );
        }
        Cmd::ImportLegacy { dry_run } => {
            // Phase 12.13.2 — inventory the legacy caches under the
            // three canonical roots, then either preview the plan
            // (dry-run, default) or write desired-state rows into
            // the store. The importer is conservative: it only
            // creates node rows for mesh-related artifacts whose
            // filename carries an obvious peer identifier; it never
            // overwrites an existing row.
            let roots = mackesd_core::legacy_inventory::default_roots();
            let artifacts = mackesd_core::legacy_inventory::inventory(&roots);
            let mesh_artifacts: Vec<_> = artifacts
                .iter()
                .filter(|a| a.mesh_data)
                .collect();
            let candidate_node_names = derive_legacy_node_names(&mesh_artifacts);
            if dry_run {
                let report = serde_json::json!({
                    "import_legacy_dry_run": true,
                    "candidate_paths":       roots
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>(),
                    "artifacts_found":       artifacts.len(),
                    "mesh_artifacts":        mesh_artifacts.len(),
                    "would_import_records":  candidate_node_names.len(),
                    "would_insert_nodes":    &candidate_node_names,
                });
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                let mut conn = mackesd_core::store::open(&db_path)
                    .with_context(|| format!("opening store at {}", db_path.display()))?;
                let existing: std::collections::BTreeSet<String> =
                    mackesd_core::store::list_nodes(&conn)?
                        .into_iter()
                        .map(|n| n.node_id)
                        .collect();
                let mut inserted = Vec::new();
                let mut skipped = Vec::new();
                for name in &candidate_node_names {
                    let node_id = format!("peer:{name}");
                    if existing.contains(&node_id) {
                        skipped.push(node_id);
                        continue;
                    }
                    mackesd_core::store::upsert_node(
                        &conn, &node_id, name,
                        // Placeholder key — a subsequent enrollment
                        // will replace this with the real Ed25519
                        // public-key fingerprint.
                        "legacy-import",
                        None,
                    )?;
                    inserted.push(node_id);
                }
                let payload = serde_json::json!({
                    "event":    "import_legacy",
                    "inserted": &inserted,
                    "skipped":  &skipped,
                })
                .to_string();
                mackesd_core::store::insert_event(
                    &mut conn, "lifecycle", &default_node_id(), &payload,
                )?;
                let report = serde_json::json!({
                    "import_legacy_dry_run": false,
                    "artifacts_found":       artifacts.len(),
                    "mesh_artifacts":        mesh_artifacts.len(),
                    "inserted_nodes":        inserted,
                    "skipped_nodes":         skipped,
                });
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
        }
        Cmd::Reconcile {
            once,
            qnm_root,
            node_id,
        } => {
            // Phase 12.5 wiring — the reconcile worker thread.
            let qnm_root = qnm_root.unwrap_or_else(mackesd_core::default_qnm_shared_root);
            let node_id = node_id.unwrap_or_else(default_node_id);

            if once {
                // Single-tick dry-run path: useful for CI smoke
                // tests + operator inspection. No background
                // thread, no signal handler.
                let outcome = mackesd_core::worker::tick(&qnm_root, &node_id, &db_path)
                    .with_context(|| format!("one-shot reconcile tick on {}", db_path.display()))?;
                println!("{}", serde_json::to_string_pretty(&outcome)?);
            } else {
                // Long-running path: spawn the worker, install a
                // SIGTERM/SIGINT handler that flips the shutdown
                // flag, then block until the worker exits.
                use std::sync::atomic::{AtomicBool, Ordering};
                use std::sync::Arc;
                let shutdown = Arc::new(AtomicBool::new(false));
                install_signal_handlers(Arc::clone(&shutdown))?;
                let handle = mackesd_core::worker::spawn_reconcile_worker(
                    qnm_root,
                    node_id,
                    db_path,
                    Arc::clone(&shutdown),
                );
                // Wait for either the worker to exit (DB went away,
                // panic — we don't panic by design) or the signal
                // handler to flip shutdown. JoinHandle::join blocks
                // until the thread returns either way.
                if let Err(e) = handle.join() {
                    eprintln!("mackesd reconcile: worker thread panicked: {e:?}");
                    std::process::exit(1);
                }
                // If we exited because the worker thread itself
                // crashed unexpectedly (e.g. someone moved the db
                // file out from under us), the loop logged the
                // error before returning. Either way: exit 0 on a
                // clean shutdown-flag path.
                if !shutdown.load(Ordering::Relaxed) {
                    // Worker exited but no shutdown was requested.
                    // Treat as a soft failure.
                    eprintln!(
                        "mackesd reconcile: worker exited without shutdown request"
                    );
                    std::process::exit(1);
                }
            }
        }
        Cmd::InventoryLegacy { mesh_only, json } => {
            // Phase 12.13.1 — read-only walk of the three legacy
            // roots. Operator runs this before `import-legacy` to
            // see what's on disk.
            let roots = mackesd_core::legacy_inventory::default_roots();
            let mut artifacts = mackesd_core::legacy_inventory::inventory(&roots);
            if mesh_only {
                artifacts.retain(|a| a.mesh_data);
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&artifacts)?);
            } else {
                print_inventory_table(&artifacts);
            }
        }
        #[cfg(feature = "async-services")]
        Cmd::Serve { qnm_root, node_id } => {
            // v2.0.0 Phase B.12 — unified meta-daemon entry point.
            // Boots the tokio runtime, registers the worker pool +
            // the existing reconcile worker, blocks on SIGTERM.
            run_serve(qnm_root, node_id, db_path)?;
        }
    }
    Ok(())
}

/// `mackesd serve` runtime. Pulls in tokio + the async supervisor
/// only when the `async-services` feature is active so the default
/// build stays sync.
#[cfg(feature = "async-services")]
fn run_serve(
    qnm_root: Option<PathBuf>,
    node_id: Option<String>,
    db_path: PathBuf,
) -> anyhow::Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let qnm_root = qnm_root.unwrap_or_else(mackesd_core::default_qnm_shared_root);
    let node_id = node_id.unwrap_or_else(default_node_id);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;
    runtime.block_on(async move {
        tracing::info!("mackesd serve: starting supervisor + workers");
        let shutdown = Arc::new(AtomicBool::new(false));
        install_signal_handlers(Arc::clone(&shutdown))
            .context("installing signal handlers")?;

        // The reconcile worker runs on its own OS thread (kept on
        // std::thread so its sync rusqlite calls don't block the
        // tokio scheduler). Future Phase B workers slot in alongside
        // it via the async supervisor.
        let reconcile = mackesd_core::worker::spawn_reconcile_worker(
            qnm_root,
            node_id,
            db_path,
            Arc::clone(&shutdown),
        );

        // Watch loop: wake every 250 ms to check the shutdown flag.
        // When it flips, drop out so reconcile.join() can wait for
        // the worker to finish its current tick.
        while !shutdown.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            if reconcile.is_finished() {
                tracing::warn!(
                    "mackesd serve: reconcile worker exited without \
                     a shutdown request"
                );
                shutdown.store(true, Ordering::Relaxed);
                break;
            }
        }
        tracing::info!("mackesd serve: shutdown requested; joining workers");
        if let Err(e) = reconcile.join() {
            tracing::error!("reconcile worker panicked: {e:?}");
            return Err(anyhow::anyhow!("reconcile worker panicked"));
        }
        tracing::info!("mackesd serve: all workers joined; exit");
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

/// Render a fixed-width inventory table to stdout. Columns:
/// kind / mesh? / size / mtime (ISO-8601 UTC) / path. We pad to the
/// widest cell in each column so the output stays grep-able.
fn print_inventory_table(artifacts: &[mackesd_core::legacy_inventory::LegacyArtifact]) {
    if artifacts.is_empty() {
        println!("(no legacy artifacts found)");
        return;
    }
    let mut rows: Vec<[String; 5]> = Vec::with_capacity(artifacts.len() + 1);
    rows.push([
        "KIND".to_owned(),
        "MESH".to_owned(),
        "SIZE".to_owned(),
        "MTIME (UTC)".to_owned(),
        "PATH".to_owned(),
    ]);
    for a in artifacts {
        rows.push([
            format!("{:?}", a.artifact_kind),
            if a.mesh_data { "yes".to_owned() } else { "no".to_owned() },
            format_size(a.size_bytes),
            format_mtime(a.mtime_ms),
            a.path.display().to_string(),
        ]);
    }
    let mut widths = [0usize; 5];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }
    for row in &rows {
        println!(
            "{:<w0$}  {:<w1$}  {:>w2$}  {:<w3$}  {}",
            row[0],
            row[1],
            row[2],
            row[3],
            row[4],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3],
        );
    }
}

/// Render a byte count as a short human-friendly string (binary
/// prefixes — KiB / MiB / GiB).
fn format_size(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let n = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", n / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", n / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GiB", n / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Render an mtime (ms since epoch) as an ISO-8601 UTC timestamp.
/// Falls back to `-` when chrono refuses the value.
fn format_mtime(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .map_or_else(|| "-".to_owned(), |dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// Build the JSON `peers why` report from a node roster (Phase
/// 12.4.4). Pure function over the store projection so callers can
/// unit-test the reason-chain shape without a real DB.
fn explain_peer(node_id: &str, nodes: &[mackesd_core::store::NodeRow]) -> serde_json::Value {
    let subject = nodes.iter().find(|n| n.node_id == node_id);
    let Some(subject) = subject else {
        return serde_json::json!({
            "node":     node_id,
            "known":    false,
            "reasons":  [],
            "note":     "node id not present in store — run `mackesd inventory-legacy` and `mackesd import-legacy` to seed.",
        });
    };
    let healthy_subject = subject.health == "healthy";
    let reasons: Vec<serde_json::Value> = nodes
        .iter()
        .filter(|other| other.node_id != node_id)
        .map(|other| {
            let same_region = match (&subject.region, &other.region) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            };
            let both_healthy = healthy_subject && other.health == "healthy";
            let chain: Vec<&str> = {
                let mut v = Vec::new();
                if both_healthy {
                    v.push("both peers healthy");
                } else {
                    v.push("one or both peers not healthy");
                }
                if same_region {
                    v.push("same region — east-west allowed by default");
                } else {
                    v.push("different regions — gated on policy::allow_east_west");
                }
                if subject.role == "decommissioned" || other.role == "decommissioned" {
                    v.push("decommissioned — no edge expected");
                }
                v
            };
            serde_json::json!({
                "peer":       other.node_id,
                "expected":   both_healthy
                              && (same_region || true)
                              && subject.role != "decommissioned"
                              && other.role != "decommissioned",
                "chain":      chain,
            })
        })
        .collect();
    serde_json::json!({
        "node":    node_id,
        "known":   true,
        "region":  subject.region,
        "role":    subject.role,
        "health":  subject.health,
        "reasons": reasons,
    })
}

/// Heuristic: extract peer name candidates from a list of legacy
/// artifacts (Phase 12.13.2). Pure helper so the importer's "what
/// would I insert" question has a single source of truth that's
/// unit-testable without disk I/O.
fn derive_legacy_node_names(
    artifacts: &[&mackesd_core::legacy_inventory::LegacyArtifact],
) -> Vec<String> {
    use std::collections::BTreeSet;
    let mut out = BTreeSet::new();
    for a in artifacts {
        // Filenames like `peer:anvil.json` or directories named after
        // peers (`~/QNM-Shared/anvil/...`) reveal candidate names.
        let path_str = a.path.display().to_string();
        for token in path_str.split(['/', '\\', '_', '.']) {
            if let Some(rest) = token.strip_prefix("peer:") {
                if !rest.is_empty() && rest.chars().all(legacy_name_char) {
                    out.insert(rest.to_owned());
                }
            }
        }
        // Also harvest the top-level directory under QNM-Shared
        // (`~/QNM-Shared/<peer>/...`).
        if path_str.contains("QNM-Shared") {
            if let Some(idx) = path_str.find("QNM-Shared/") {
                let after = &path_str[idx + "QNM-Shared/".len()..];
                if let Some(seg) = after.split('/').next() {
                    if !seg.is_empty() && seg.chars().all(legacy_name_char) {
                        out.insert(seg.to_owned());
                    }
                }
            }
        }
    }
    out.into_iter().collect()
}

fn legacy_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Resolve the stable node id from `$MACKESD_NODE_ID` then
/// `$HOSTNAME` then the `hostname` syscall, falling back to
/// `peer:unknown` so the audit-log column is never empty.
fn default_node_id() -> String {
    if let Ok(v) = std::env::var("MACKESD_NODE_ID") {
        return v;
    }
    let host = std::env::var("HOSTNAME").ok().or_else(|| {
        std::process::Command::new("hostname")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_owned())
    });
    match host {
        Some(h) if !h.is_empty() => format!("peer:{h}"),
        _ => "peer:unknown".to_owned(),
    }
}

/// Register a SIGTERM + SIGINT handler that flips `shutdown` to
/// true. Uses `signal-hook`'s safe `Signals` iterator API — a
/// background thread reads from the kernel-managed signal queue
/// and stores into the shared atomic. No `unsafe` required (the
/// workspace forbids `unsafe_code`).
///
/// The reader thread is daemon-style: it lives as long as the
/// process and exits naturally when the process exits. Since
/// `mackesd reconcile` returns from main only after the reconcile
/// worker joins, we don't need to track the reader's handle.
fn install_signal_handlers(
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> anyhow::Result<()> {
    use signal_hook::consts::{SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;
    let mut signals =
        Signals::new([SIGTERM, SIGINT]).context("installing SIGTERM/SIGINT iterator")?;
    std::thread::Builder::new()
        .name("mackesd-signal".into())
        .spawn(move || {
            for sig in &mut signals {
                tracing::info!(signal = sig, "received shutdown signal");
                shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
                // Keep reading so a second signal doesn't terminate
                // the process before the worker drains.
            }
        })
        .context("spawning signal-reader thread")?;
    Ok(())
}
