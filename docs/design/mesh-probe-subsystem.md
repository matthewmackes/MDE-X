# EPIC-MESH-PROBE — Centralized probe subsystem

**Status:** Locked 2026-05-28 via a 10-question `/plan` survey
(operator-issued lock-lift from the §0.16 feature lock — recorded in
CLAUDE.md §0.16 + memory `project_mesh_probe`).
**Authority:** this doc (per-epic locks, §0.14 tier 4).
**One-liner:** one probe source, every tool consumes it.

---

## 0. Motivation

Today each tool that needs "what's reachable / what services run where"
rolls its own probing: the (just-shipped) `app_sync` media-discovery
TCP-probes peer ports; the planned MESH-A-* network-assessment epic
would scan independently; the connect-action port-mappings would probe
again. That's N probers, N code paths, N network footprints, N caches —
the opposite of §0 "Simple".

**EPIC-MESH-PROBE consolidates all of it into one probe subsystem.**
Each peer probes from its own vantage, publishes a card-shaped
inventory to the GFS mesh-home, and every tool reads that one merged
inventory. This *underpins* the existing MESH-A-* epic (it becomes the
data source MESH-A-1/2/4/7 build on) rather than adding parallel scope.

---

## 1. The 10 locks (2026-05-28 survey)

| # | Decision | Lock | Rationale |
|---|----------|------|-----------|
| Q1 | **Topology** | **Per-peer, centerless** — each peer probes its own vantage; consumers read the merged union | §0 "Centerless"; peers genuinely see different reachability (LAN/NAT/link); no single point of failure |
| Q2 | **Substrate** | **GFS card-files = source of truth + Bus `probe/changed` event** | Reuses the per-peer GFS pattern (nebula-bundle / ban-list / heartbeat); Bus event gives live reactivity without polling |
| Q3 | **Engine** | **nmap for everything** (hard `Requires: nmap`) | Single engine, richest data; no native-Rust fallback path |
| Q4 | **Driver** | **nmap + bundled NSE scripts** | Stock `-sV` plus custom NSE (mesh-media detector, Mackes-service fingerprinter) shipped in the RPM for platform-specific identification |
| Q5 | **Scope** | **Mesh peers + LAN + operator-arbitrary targets** | One probe feeds media-discovery (peers) AND MESH-A-4 surrounding-host ID (LAN) AND operator-declared externals |
| Q6 | **Cadence** | **Two-tier + manual**: fast liveness (~60 s) + deep `-sV`/NSE/LAN sweep (~10 min) + operator manual refresh | Aligns the existing MESH-A-1 "passive always + active 10 min + manual refresh" lock; balances freshness vs. cost/noise |
| Q7 | **Schema** | **Card-shaped** (`mde-card`) — host Cards with per-service child Cards | Renders directly in Portal/Workbench via the Portal-31 `card_index`; no separate data model + transform |
| Q8 | **Read API** | **`mackesd::probe` Rust library** + `card_index` (Portal) + Bus-event | Minimal new surface: Rust workers call the library; the UI gets Cards free; Bus-event drives reactivity |
| Q9 | **Safety** | **Scan everything actively** (no gating by default) | Operator directive; risk accepted + documented (§7). Polite rate-limiting + an optional do-not-scan exclusion list remain as escape hatches |
| Q10 | **Sequence** | **Big-bang epic** — probe subsystem + all consumers repointed in one cohesive landing | One consistent end-state, no transitional dual-prober period |

---

## 2. Architecture

```
            ┌─────────────────── each peer (centerless) ───────────────────┐
            │                                                                │
  cadence ──┤  mackesd::workers::probe                                       │
  (Q6)      │    ├─ fast tier  (~60 s): nmap liveness + curated known-ports  │
            │    └─ deep tier  (~10 min): nmap -sV + bundled NSE (Q4)         │
            │         over { mesh peers ∪ local LAN ∪ arbitrary } (Q5,Q9)    │
            │                         │ parse nmap -oX                        │
            │                         ▼                                       │
            │   probe-inventory: Vec<Card>  (host Card + service child Cards) │
            │                         │ (Q7 mde-card schema)                  │
            │                         ▼                                       │
            │   <qnm_root>/<self>/mackesd/probe-inventory.json  (Q2 GFS)      │
            │                         │  + Bus publish `probe/changed`        │
            └─────────────────────────┼───────────────────────────────────────┘
                                      │  GFS replicates every peer's file
                                      ▼
        mackesd::probe  (Q8 library: inventory() merges all peers' files)
                │                      │                       │
                ▼                      ▼                       ▼
        app_sync (media)        MESH-A-1/2/4/7         connect-action
        peers_with_service       host/service ID        port→action map
        ("airsonic"/"jellyfin")  + defense surfaces
                                      ▲
        Portal / Workbench ──────────┘  (read host/service Cards via card_index)
```

**Probe engine (Q3/Q4):** the worker shells `nmap` with `-oX -` (XML to
stdout) and parses it. Two invocation profiles: a fast liveness profile
(`-sn` + a curated `-p` known-port list) and a deep profile (`-sV
--version-all` + `--script <bundled NSE>`). NSE scripts ship in the RPM
under `/usr/share/mde/nmap/` (mesh-media detector + Mackes-service
fingerprinter); the worker passes `--datadir`/`--script` at the bundled
path.

**Inventory (Q7):** one host = one `Card` (`CardKind::Host`); each open
port/service = a child `Card` (`CardKind::Service`) under
`host.children`. Rich nmap fields (product, version, OS fingerprint,
trust-state, last-seen, source mesh/lan/arbitrary) land in the card
field set; `schema_version` carries forward-compat. Because entries are
Cards, the Portal-31 `card_index` scan surfaces them with no transform.

**Substrate (Q2):** the deep-tier writer writes
`<qnm_root>/<self>/mackesd/probe-inventory.json` (atomic temp+rename,
GFS-replicated) and publishes a `probe/changed` Bus message (priority
`min`) on a material diff. `mackesd::probe::inventory()` reads + merges
every peer's file; consumers subscribe to `probe/changed` for live
re-reads.

**Read API (Q8):** `mackesd::probe` exposes `inventory() -> Vec<Card>`
+ `peers_with_service(kind: &str) -> Vec<HostService>` + a
`probe/changed` subscription helper. Rust consumers call it in-process;
the Portal reads the same Cards via `card_index`.

---

## 3. Consumers repointed (Q10 big-bang)

| Consumer | Today | After |
|----------|-------|-------|
| `app_sync` (media-discovery) | self-TCP-probes peer ports (`mesh_media::discover`) | reads `probe::peers_with_service("airsonic"/"jellyfin")`; `mesh_media`'s probe path retires |
| MESH-A-1 (per-peer assessment) | (open) | consumes the inventory as its assessment data source |
| MESH-A-2 (route-trace) | (open) | route-trace targets sourced from inventory hosts |
| MESH-A-4 (surrounding-host ID) | (open) | the deep nmap/NSE pass + LAN scope IS the surrounding-host identifier |
| MESH-A-7 (port→connect-action) | (open) | the 12 well-known-port mappings read open ports from inventory |
| Portal / Workbench | n/a | render host/service Cards via `card_index` |

---

## 4. Acceptance (each bench-observable)

- Probe worker runs on the two-tier cadence; `nmap` absent → worker logs
  + degrades (no crash), `Requires: nmap` ensures presence in the RPM.
- A peer running Jellyfin on a non-standard port is identified by
  service (not just port) via `-sV`/NSE — proving the engine choice.
- `probe-inventory.json` appears under `<qnm_root>/<self>/mackesd/`,
  GFS-replicates to a second peer, and `mackesd::probe::inventory()` on
  the second peer returns the union.
- Editing the inventory (a server appears/disappears) emits a
  `probe/changed` Bus message; a subscribed consumer re-reads.
- `app_sync` configures media clients from `probe::peers_with_service`
  with `mesh_media`'s own probe path deleted (no double-probe).
- Host + service Cards render in the Portal via the existing
  `card_index` with no probe-specific UI code.
- Bundled NSE scripts install to `/usr/share/mde/nmap/` and load
  (`nmap --script-help <name>` resolves).

---

## 5. Worklist

Lifted into `docs/PROJECT_WORKLIST.md` under `### EPIC-MESH-PROBE`.
Per Q10 the tasks land as one cohesive epic, but each is an independent
bench-observable user-story task per §0.12.

---

## 6. Out of scope

- **Vulnerability scanning** (CVE/compliance). The probe identifies
  services; it does not assess them for vulns. That's a separate concern
  owned by Crowdsec / MESH-A-3. (Nessus was explicitly rejected:
  closed/commercial + wrong shape.)
- **rustscan front-end** (Q4 alt) — raw nmap + NSE is sufficient at
  8-peer scale; rustscan is not packaged in Fedora and adds bundling.
- **Native-Rust probe fallback** (Q3 alt) — nmap is the sole engine; no
  parallel native path.

---

## 7. Risks + security review (DoD gate 8)

**New surface:** active network scanning (nmap) from every peer over
mesh + LAN + arbitrary targets, plus a new GFS artifact
(`probe-inventory.json`) and a new Bus topic (`probe/changed`).

- **(a) surface** — outbound nmap scans (no new listener/port); the
  inventory is a GFS file (Nebula-replicated, never network-exposed);
  `probe/changed` rides the existing per-peer Bus persist tree.
- **(b) what reaches it** — only `mackesd` writes the inventory; the
  `mackesd::probe` library is in-process; the Bus topic is mesh-only.
- **(c) open-mesh fit** — mesh-peer scanning fits the flat-trust model
  (enrolled, consenting nodes). **The Q9 "scan everything actively"
  lock extends active scanning to the LAN + arbitrary targets with no
  gating by default** — this is an operator-accepted risk: active scans
  can trip IDS on networks the operator doesn't own (corporate/cafe) and
  may breach those networks' ToS. **Mitigations retained even under
  "scan everything":** polite nmap timing (`-T` tuning, not `-T5`), and
  an optional operator do-not-scan exclusion list (CIDR/IP) so a known-
  hostile segment can still be carved out. The default is wide-open per
  the operator directive; the escape hatch exists for when they want it.

---

## 8. Provenance

- `mesh_media.py`'s own `DeprecationWarning` pointed discovery at
  "per-peer service telemetry written into the mesh-FS" — EPIC-MESH-PROBE
  is the realized form of that intent (card inventory in GFS), now the
  source for the `app_sync` discovery shipped 2026-05-28.
- Consolidates the probing MESH-A-1/2/4/7 would each have done
  separately; those tasks repoint onto this substrate.
