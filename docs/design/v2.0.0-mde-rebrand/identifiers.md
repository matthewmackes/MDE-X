# MDE Rebrand — Canonical Identifier Table

**Locked:** 2026-05-19.
**Source-of-truth for every rename in Phase 0** of the
`docs/PROJECT_WORKLIST.md` "v2.0.0 Mackes DE" plan. Every later
Phase 0 substep (0.2–0.14) refers back to this document. Phase A–I
worklist entries that name `mackes-*` identifiers are historical
placeholders; the live names are the MDE column.

## Why rebrand

Reasons captured at the 2026-05-19 lock:

1. The v2.0.0 release is a clean break — drops XFCE, X11, i3, every
   Python daemon — so the product is materially different from the
   1.x "shell on top of XFCE" framing.
2. "Mackes Shell" reads as a thin add-on. The unified Rust meta-
   daemon + sway + Iced+libcosmic stack is a full desktop
   environment in its own right.
3. The `mackes-shell` package name conflicts with people's mental
   model of "a shell" (zsh/bash/fish). `mde` and "Mackes Desktop
   Environment" are unambiguous.
4. Versioning continues from 2.0.0 — the major bump matches the
   rebrand so there's no ambiguity about which release crosses
   the line.

## Upgrade path

- `Provides: mackes-shell = 2.0.0` + `Obsoletes: mackes-shell < 2.0.0`
  in the new `mde.spec` so `dnf upgrade` on a 1.x box lands on `mde`
  automatically.
- A `mde-migrate-from-1x` helper (Phase 0.5) atomically moves
  `~/.config/mackes-shell/` → `~/.config/mde/`, same for cache and
  state dirs.
- Env-var fallback shim (Phase 0.6) reads `MACKES_*` when `MDE_*`
  isn't set, with a deprecation warning to journald — removed in
  v2.1.
- D-Bus service aliases (Phase 0.4) keep the v1.x
  `shell.mackes.*` names addressable for one release.
- Binary symlinks for `mackesd` → `mded`, `mackes-panel` →
  `mde-panel`, `mackes` → `mde` survive in `%files` for one
  release so existing scripts keep working.

## Canonical mapping

| Layer | Old (1.x) | New (v2.0.0 MDE) |
|---|---|---|
| Product name | Mackes Shell | Mackes Desktop Environment (MDE) |
| RPM package | `mackes-shell` | `mde` |
| Virtual provides | — | `Provides: mackes-shell = 2.0.0`, `Obsoletes: mackes-shell < 2.0.0` |
| Cargo workspace | `mackes-shell` | `mde` |
| Daemon crate | `mackesd` | `mded` |
| Panel crate | `mackes-panel` | `mde-panel` |
| Config crate | `mackes-config` | `mde-config` |
| Mesh types crate | `mackes-mesh-types` | `mde-mesh-types` |
| KDE Connect crate | `mackes-kdc` | `mde-kdc` |
| Daemon binary | `mackesd` | `mded` |
| Panel binary | `mackes-panel` | `mde-panel` |
| WM helper | `mackes-wm` | `mde-wm` |
| Session binary | `mackes-session` | `mde-session` |
| Session enforcer | `mackes-enforce-session` | `mde-enforce-session` |
| Workbench launcher | `mackes` | `mde` |
| Python package | `mackes` | `mde` |
| Test runner shim | `tests/_run_without_pytest.py` | unchanged (internal) |
| D-Bus namespace | `shell.mackes.*` | `dev.mackes.MDE.*` |
| D-Bus services | `shell.mackes.Panel`, `shell.mackes.Workbench` | `dev.mackes.MDE.Shell`, `dev.mackes.MDE.Settings`, `dev.mackes.MDE.Notifications`, `dev.mackes.MDE.Session`, `dev.mackes.MDE.Fleet` |
| systemd user units | `mackesd.service` | `mded.service` (+ aliases for in-place upgrade for one release) |
| Config dir | `~/.config/mackes-shell/` | `~/.config/mde/` |
| Panel config | `~/.config/mackes-panel/panel.toml` | `~/.config/mde/panel.toml` |
| Cache dir | `~/.cache/mackes/` | `~/.cache/mde/` |
| State dir | `~/.local/state/mackes/` | `~/.local/state/mde/` |
| QNM-Shared root | `~/QNM-Shared/` | unchanged (cross-fleet shared name) |
| Env-var prefix | `MACKES_*` | `MDE_*` |
| CSS namespace | `.mackes-*` | `.mde-*` (Iced/libcosmic theme tokens) |
| metainfo file | `shell.mackes.Panel.metainfo.xml` | `dev.mackes.MDE.metainfo.xml` |
| AppStream component-id | `shell.mackes.Panel` | `dev.mackes.MDE` |
| RPM asset name | `mackes-shell-X.Y.Z-1.fc44.x86_64.rpm` | `mde-2.0.0-1.fc44.x86_64.rpm` |
| GitHub release tag | `vX.Y.Z` | `vX.Y.Z` (unchanged — versions continue from 2.0.0) |
| Repo URL | `github.com/matthewmackes/MAP2-RELEASES.git` | unchanged (out-of-scope user action) |

## D-Bus object-path conventions

Every new MDE service follows the standard reverse-DNS-and-path
shape:

```
service name : dev.mackes.MDE.<Concern>
object path  : /dev/mackes/MDE/<Concern>
interface    : dev.mackes.MDE.<Concern>
```

Concerns: `Shell`, `Settings`, `Notifications`, `Session`, `Fleet`.
The historical `shell.mackes.Panel` and `shell.mackes.Workbench`
get one-release aliases via `Alias=` in the systemd-managed
session-bus directory so any external integration that hard-codes
the v1.x names keeps working for the 2.0.x line.

## Phase 0 cross-cutting impact

Renames touch every file with one of the old identifiers. Concrete
hot-spots picked up in the matching substep:

- 0.2 — `Cargo.toml` workspace members + per-crate `[package]` names
- 0.3 — `bin/` shell wrappers + `data/man/` regen
- 0.4 — `data/dbus-1/services/` regen + zbus `interface(name=…)`
- 0.5 — `mde-migrate-from-1x` helper + journald `mde-migrate` tag
- 0.6 — Rust env-var reads + Python `os.environ.get` calls
- 0.7 — CSS files under `data/css/` + Iced theme adapter
- 0.8 — `packaging/fedora/mackes-shell.spec` → `packaging/fedora/mde.spec`
- 0.9 — `data/metainfo/*` + `data/applications/*.desktop`
- 0.10 — Python `mackes/` → `mde/` + `pyproject.toml`
- 0.11 — README, help docs, About dialog, error messages
- 0.12 — `install.sh` asset-name resolver
- 0.13 — Test sweep for the new names
- 0.14 — `CHANGELOG.md` 2.0.0 header

Per the worklist's "Phase 0 Definition of Done": identifier table
committed (this file); all 12 mechanical renames (0.2–0.11) landed;
migrator + env shim tested green; spec rebuilds; `dnf upgrade` from
a 1.x installation lands on `mde-2.0.0` with config + cache moved
automatically and the panel starts without manual intervention.

## What is NOT being renamed

- The GitHub repo URL — out-of-scope user action (Phase 0.12).
- The version numbering — continues monotonically (`v2.0.0` is the
  cut tag; the version itself is the rebrand signal).
- The 1.x CHANGELOG entries — those releases shipped as "Mackes
  Shell" and the log preserves that historical truth.
- The QNM-Shared root directory name — cross-fleet identifier
  that predates the brand.
- The `Carbon-*` design tokens — orthogonal design vocabulary,
  separately owned by `data/css/tokens.css`.
- `bin/install.sh`'s ability to find a `mackes-shell-*.rpm` asset
  by name — kept as fallback so existing install URLs keep
  resolving until the next major bump.

## See also

- `docs/PROJECT_WORKLIST.md` — Phase 0 substeps reference this
  doc as the source of truth.
- `~/.claude/projects/-home-mm-Desktop-files-mackes-shell/memory/project_v2_0_0_mackes_de.md`
  — lock notes from the 2026-05-19 /plan workflow survey.
- `~/.claude/plans/zazzy-gliding-platypus.md` — the full v2.0.0
  plan that motivated the rebrand.
