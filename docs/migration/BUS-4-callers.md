# BUS-4 — Notification publisher inventory

**Authority:** BUS-4.1 (worklist).
**Audience:** the engineer implementing BUS-4.2 (the GF-17 hard cut).
**Last updated:** 2026-05-26 — session=opus-47-2026-05-26-ship-R.

This document is the BUS-4.1 audit: every place in the
workspace that publishes a notification, with the migration
target topic each one rewrites to in BUS-4.2.

The original GF-17 design doc referenced a planned
`crates/mackesd/src/notification_bus/` module that was **never
built** — the v6.x Mackes Bus epic (locked 2026-05-25 via the
104-Q poll) supersedes it directly, so the BUS-4.2 hard cut
rewrites the *real* publisher sites listed below rather than
deleting a nonexistent staging module.

---

## Publisher sites

| # | Crate / file | Line | Current API | Target Bus topic |
|--:|---|--:|---|---|
| 1 | `crates/mackesd/src/ipc/notifications.rs` (`NotificationsService::notify`) | 78 | `org.freedesktop.Notifications.Notify` D-Bus method | `fdo/<app>` (BUS-4.4) |
| 2 | `crates/mackesd/src/workers/notification_relay.rs` (`tick`) | 96 | Reads `~/QNM-Shared/<peer>/.qnm-notifications/*.json`; INSERTs into `notifications` SQLite table | — (retire entirely; cross-peer routing becomes a BUS subscription on `fdo/#`) |
| 3 | `crates/mackesd/src/workers/alert_relay.rs` (`tick`) | 96 | Watches `~/.local/share/mde/alerts/*.json`; shells out to `notify-send` | `mon/<class>` + keep JSONL for external consumers (BUS-4.3 dual-write) |
| 4 | `crates/mde-alert-emit/src/main.rs` (binary entry) | n/a | Writes `~/.local/share/mde/alerts/<ulid>.json` | Add direct `mde-bus publish mon/<class>` after the JSONL write (BUS-4.3) |
| 5 | `crates/mackes-panel/src/toasts.rs` (legacy GTK panel toast) | n/a | In-process Iced toast surface | — (retired with v1.x GTK panel; not migrated) |

## Per-site migration plan

### 1. `NotificationsService::notify` (BUS-4.4 target)

**What it does today.** Implements the
`org.freedesktop.Notifications.Notify` D-Bus method on session
bus. Stores each call as a row in the `notifications` SQLite
table (when bound to a connection) and returns either the
synthetic incoming ID (unbound) or the row ID.

**What BUS-4.4 changes.** Bridge: every successful `Notify` call
ALSO publishes to `fdo/<app_name>` via `mde_bus::persist::Persist`
+ `mde_bus::hooks::publisher::publish_to_ntfy`. FDO clients
continue to receive native delivery via the existing path; the
Bus publish is additive so MON / Workbench / audit consumers
see the same events.

**Topic shape.** `fdo/<app_name>` where `<app_name>` is the
sanitised D-Bus `app_name` arg (lowercase, slashes converted to
hyphens, no `..`). Title + body lift from the FDO Notify
`summary` + `body` params; priority maps from `urgency` hint
(0 = `min`, 1 = `default`, 2 = `urgent`).

### 2. `notification_relay::tick` (retire entirely)

**What it does today.** Polls every other peer's
`~/QNM-Shared/<peer>/.qnm-notifications/` directory on a 2s
ticker, parses each new JSON file as a `MirroredEntry`, and
INSERTs it into the local `notifications` table for deduplicated
cross-peer display.

**What BUS-4.2 changes.** Delete the entire worker. Cross-peer
routing is now a side-effect of every `Notify` call also
publishing to `fdo/#` on the Bus — every peer subscribes to
the wildcard and the BUS-1.4 persistence layer dedupes by ULID
naturally. The `~/QNM-Shared/<peer>/.qnm-notifications/`
directories become dead-letter drops; the GF-1.x mesh-home
fold also retires the `QNM-Shared` convention in favor of
`<bus_root>/<topic>/<ulid>.json`.

**Downstream cleanup.**
- `crates/mackesd/src/bin/mackesd.rs:2125` registers
  `NotificationRelayWorker` — that registration line goes too.
- `crates/mackesd/src/workers/mod.rs:156` `pub mod
  notification_relay;` — deleted.
- The `notifications` SQLite table stays (FDO history is still
  needed for the bell / tray surfaces) but a new column links
  rows to their Bus ULID for audit cross-reference.

### 3. `alert_relay::tick` (BUS-4.3 dual-write)

**What it does today.** Polls `~/.local/share/mde/alerts/` on a
2s ticker; for every new `<ulid>.json` file shells out to
`notify-send <severity> <summary>` to surface as an FDO toast.
Tracks seen IDs in `BTreeSet` for idempotency.

**What BUS-4.3 changes.** Keep `notify-send` for FDO toast
surface (external consumers like the Workbench Mesh Health
panel read the JSONL directly). Add a parallel call to
`mde_bus::persist::Persist::write("mon/<class>", priority, title,
body)` so MON alerts also land on the Bus and reach the
priority-based surface dispatcher (status-strip for `warn`,
Theater for `crit`). The `class` field maps from the alert
event's `alert` name (`cpu_overload` → `mon/cpu`, etc.).

### 4. `mde-alert-emit` (BUS-4.3 source-side dual-write)

**What it does today.** CLI tool MON-3 ships; called by Netdata
+ other monitoring sources to write a structured alert JSONL
file. Idempotent by deterministic ULID.

**What BUS-4.3 changes.** After the JSONL write, the binary
also publishes the same payload via `mde-bus publish
mon/<class> --title <summary> --body <details> --priority high`.
This is the upstream half of BUS-4.3 — `alert_relay`'s
downstream notify-send is the legacy delivery side that BUS-4.3
also Bus-publishes, but BUS-4.3 lets external alert sources
skip the JSONL entirely by emitting directly via the Bus.

### 5. `crates/mackes-panel/src/toasts.rs` (retired)

The v1.x GTK panel + its in-process Iced toast surface retires
under EPIC-RETIRE-PY-WORKBENCH + EPIC-UI-MATERIAL. No migration
— toasts come from the BUS-2.x surfaces (tray + status strip +
Theater) once those Iced impls land.

## Cross-check methodology

This audit was produced by:

1. `grep -rEn 'notify-send|fn notify|notification_relay'
   crates/mackesd/src/ crates/mde-bus/src/ crates/mde-alert-emit/src/`
2. `find crates -name 'notification_bus*'` — zero results,
   confirming the original GF-17 staging module never landed.
3. Manual read of each grep hit's surrounding code to
   distinguish publishers (Bus-4.2 targets) from consumers
   (left alone).

The audit is a snapshot. New publishers added between this doc
and BUS-4.2 must be appended to the table above; the lint-voice
addition in BUS-4.5 already scans `crates/mde-bus` + `data/bus`
on every commit, but there's no lint that flags new FDO Notify
implementations — that's manual code review on every PR until
BUS-4.2 lands and the bridge becomes the only valid path.

## Out-of-scope sites

These showed up in the broad grep but are NOT publisher sites:

- `crates/mackesd/src/ipc/notifications.rs` test fixtures
  (lines 217-330) — test-only.
- `mackes/birthright.py` / `mackes/displays.py` /
  `mackes/headless/*.py` — these consume the FDO interface
  (call `org.freedesktop.Notifications.Notify` over D-Bus); the
  bridge in BUS-4.4 catches their publishes automatically since
  it lives in the server implementation, not at each caller.
- `crates/mackes-panel/src/toasts.rs` — retired (above).

## Open follow-ons for BUS-4.2

When the hard-cut PR lands, also do:

- [ ] Delete `crates/mackesd/src/workers/notification_relay.rs`.
- [ ] Delete `crates/mackesd/src/workers/mod.rs:156`
      (`pub mod notification_relay;`).
- [ ] Delete the `notification_relay` worker spawn block in
      `crates/mackesd/src/bin/mackesd.rs` (lines 2275–2295 area).
- [ ] Remove `notification_relay` from the worker_names list
      (line 2286).
- [ ] Drop the `.qnm-notifications` directory creation from
      `mackes/birthright.py` (if still present) — the convention
      retires alongside the worker.
- [ ] Verify `grep -r notification_bus crates/` returns zero
      hits (it already does — see above).

Once BUS-4.2 + BUS-4.3 + BUS-4.4 ship, this doc moves to
`docs/migration/archive/` as historical context.
