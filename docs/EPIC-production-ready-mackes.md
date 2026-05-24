# EPIC: Production-Ready Mackes — Deliver the Brief

**Status:** active · **Owner:** core · **Spans:** v1.8 → v2.6

> "Best-of-breed tools, simply and easily, no matter the location."

A user installs Mackes and within ~90 seconds has a usable, opinionated
desktop. The mesh joins itself when there's one to join. Every panel
feels like a finished product, not an engineering surface. Nothing
tells the user a feature isn't installed.

## Acceptance criteria

- [ ] First-run wizard completes in ≤ 90 seconds of user-perceived time
- [ ] No installer step blocks the foreground thread > 5 seconds
- [ ] Every workbench panel uses PF6 layout patterns (Page + PageSection + Card + Toolbar)
- [ ] Zero "feature X not installed" messages anywhere in the UI
- [ ] All panels keyboard-navigable + screen-reader-named
- [ ] CI green on every push
- [ ] README documents build / run / test / cut-release accurately
- [ ] pytest coverage ≥ 60% on `mesh_vpn`, `mesh_discovery`, `mesh_mdns`, `birthright`

---

## Track 1 — Fast first-run (v2.2.x)

The brief says "fewest clicks." Today the wizard runs `dnf upgrade` +
Guacamole download + Plymouth initrd rebuild on the foreground.

**Stories**

- Parallelize independent steps — themes / fonts / panel-layout / conky
  deploy concurrently. Apply pipeline becomes a DAG, not a list.
- Foreground vs. background classification — every birthright step tagged
  *blocking* (themes, panel, lightdm) or *background* (dnf update,
  Guacamole, Plymouth initrd, Ansible bootstrap, third-party repos).
- `mackes-firstboot.service` — replaces the synchronous wizard apply
  with a fast foreground phase + a systemd unit that completes the rest
  while the user's already on the desktop.
- Status surface for background work — Conky HUD + Dashboard show
  "Setup completing: 3 steps remaining" with live progress.
- Pre-staged image option — document a Kickstart + ostree compose path
  so a Mackes-flavored Fedora installer skips birthright entirely.

---

## Track 2 — Full PF6 panel rewrites (v2.1 → v2.6)

v2.0 shipped the design-system swap. v2.1+ migrates the panels
themselves to PF6 layout patterns.

| Release | Scope |
|---|---|
| v2.1 | `.cds-*` → `.pf-*` selector rename; `mackes/carbon/` → `mackes/patternfly/` module rename (mechanical) |
| v2.2 | Mesh panels (Mesh, Mesh Remote, Mesh Advanced sub-panels) → PF Page + Card |
| v2.3 | System panels (Displays, Window Manager, Workspaces, …) |
| v2.4 | Apps + Maintenance panels |
| v2.5 | Configuration panels (Look & Feel, Devices) |
| v2.6 | Wizard pages on PF Page + Wizard pattern; deprecate Carbon Productive layout |

Per release: screenshots refresh; CHANGELOG documents the panel group.

---

## Track 3 — "Never say feature unavailable" (v1.8 → v2.x)

The brief says no feature should ever be missing. Today there are real
gaps.

**Stories**

- Always-on roles in birthright — Nebula binary + lighthouse role,
  NATS, mDNS responder installed (not necessarily running)
- Auto-elect role on demand — promote a peer to lighthouse /
  NATS broker / NF-1 TCP-443 relay automatically based on availability
- Onboarding wizards package — `mackes/wizard/onboarding/` with concrete
  wizards for: lighthouse public hostname, Guacamole admin password,
  mesh-shared media credentials
- QR-scan discovery — webcam → mesh join (`zbar-tools` + capture surface
  scanning the v2.5 `mesh:<id>@<lighthouse>:<port>#<bearer>` token)
- Lighthouse rotation in auto-heal — manual relay cycle in the 3-retry
  chain after a confirmed lighthouse-unreachable failure

---

## Track 4 — Performance + reliability (v2.2.x)

Many panels shell out in `__init__` (xrandr / nmcli / fc-list / rpm-q).
First-paint cost compounds across the workbench.

**Stories**

- `mackes-stated.service` — system-level cached state daemon. Probes
  slow things on a schedule (5s for status, 60s for inventories).
  Panels read from the cache via D-Bus.
- Async panel construction — every panel `__init__` returns immediately;
  data loads via `GLib.idle_add` worker callbacks.
- Remove blocking shell-outs from `_build_nav` — make nav data fully
  cached.
- Test coverage on mesh + birthright — pytest target ≥ 60% line cover.

---

## Track 5 — Build, test, dev experience (v2.1.x)

Today `ci.yml` fails on every push (pre-existing).

**Stories**

- Fix `ci.yml` — diagnose the 0-second failure, restore green builds.
- README sweep — accurate build / run / test / cut-release / dev-setup,
  match the CLAUDE.md §0.6 flow.
- Node 24 action bump — `actions/checkout@v4`, `softprops/action-gh-release@v2`
  deprecated June 2026.
- `make lint` — wire ruff/mypy as a Make target so `cut release` gates
  on it.
- `make test-fast` vs `make test-slow` — split fast pytest from
  RPM-build smoke.

---

## Track 6 — Mesh polish (v2.2.x)

**Stories**

- Multiple control endpoints — pick the closest (RTT-based) when mDNS
  finds 2+.
- Identity rotation — `mackesd ca revoke <node-id>` + re-enroll flow
  when a peer has been offline > 14 days.
- Open-mesh stays the lock — Nebula's group-based ACL surface is
  available but the v2.5 directive is flat-trust across all enrolled
  peers (NF-* open-mesh lock 2026-05-23). Revisit only on operator
  request.
- Mesh capacity status — visible in Conky as the mesh approaches
  `MESH_CAP` (16 peers).

---

## Sequencing summary

| Release | Tracks delivered |
|---|---|
| v2.1.0 | Track 2 rename + **Mesh Media** (Sublime Music + Delfin + Thunar view) |
| v1.8.0 | Track 3 first half (onboarding wizards, always-on roles) |
| v2.2.0 | Track 1 fast first-run + Track 4 perf + Track 2 mesh panels |
| v2.3–2.6 | Track 2 remaining panel groups, one per release |

## Risk

- **Track 1 fast first-run** has the biggest blast radius — background
  services that fail silently are worse than synchronous failures.
  Mitigation: every background step emits structured failure events to
  Conky + Maintain → Repair.
- **Track 2 panel rewrites** = visible regression risk per release.
  Mitigation: keep before/after screenshots in CHANGELOG; gate each
  release on a manual walk-through.
- **Track 4 `mackes-stated`** introduces a new D-Bus daemon —
  significant attack surface. Mitigation: socket-activated, runs as the
  user not root, no remote endpoints.
