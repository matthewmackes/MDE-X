# Installing & updating MDE (`mde-install` / `mde-update`)

`mde-install` converges this machine to a known-clean state for a chosen
profile, then runs birthrights. `mde-update` shows fleet version skew and
coordinates a whole-mesh upgrade. Both are CLI tools — no GUI needed.

The blessed install path is a clean **Fedora Server (CLI)** that you
*build up* from: install the headless base, then layer the desktop only
if you want it.

## The three profiles

| Profile | What it is | Needs `mde-desktop`? |
|---|---|---|
| **lighthouse** | Routing-only mesh node — nebula + mackesd + a read-only `mesh-storage` mount. VPS-friendly, no brick, no desktop. | no |
| **headless** | Headless peer — lighthouse + a storage brick + fleet ansible-pull + monitoring. No desktop. | no |
| **full** | Full workstation — everything above + the sway/Iced desktop. | yes |

Capability builds up: `lighthouse ⊂ headless ⊂ full`. Reaching **full**
*adds* the desktop on top of the server base; it never tears a server
down.

## Installing

```bash
# 1. Land the headless substrate on a clean Fedora Server box.
sudo dnf install mde-core

# 2. Configure a non-desktop node:
sudo mde-install --profile=headless        # or --profile=lighthouse

# …or build up to the full desktop:
sudo dnf install mde-desktop
sudo mde-install --profile=full
```

With no `--profile`, an interactive picker asks (defaults to **full**
when `mde-desktop` is installed). The RPM never auto-runs the installer —
it just prints a reminder to run `sudo mde-install`.

## What `mde-install` wipes (and the three confirms)

Every run converges to a clean baseline: it wipes `~/.config/mde/`,
`~/.local/share/mde/`, `~/.cache/mde/`, `/etc/mde/`, and `/var/lib/mde/`,
then runs the profile's birthrights. This is idempotent on purpose —
`mde-install` always produces a known state regardless of what was there
before.

Because that's destructive, there are up to three guards:

1. **Typed `NUKE`.** Interactive runs first print a tree of exactly what
   will be wiped — each path with its size and file count — plus which
   peers will see this node leave the mesh. You must type the literal
   word `NUKE` to proceed. Anything else aborts with no changes made.

2. **Lossy-downgrade confirm.** If you're re-installing a *lower*
   profile over a higher one (e.g. `full → lighthouse`, which drops your
   desktop, or `full → headless`), after `NUKE` you must also type the
   *previous* profile name. Upgrades and same-profile reinstalls skip
   this. This stops reflexive `NUKE` from quietly demoting a workstation
   to a routing-only lighthouse.

3. **`--yes` audit log.** Unattended runs (`--yes`, or any run with no
   terminal) skip the prompts but write the same summary to
   `/var/log/mde/wipe-<id>.log` *before* anything is destroyed, and print
   the path so you can `tail -F` it from another shell. On a lossy
   downgrade the log's first line is `WARNING: lossy downgrade from <X> to <Y>`.

Useful flags: `--dry-run` (print the plan, change nothing),
`--backup` (tar existing state to `/var/lib/mde/backups/` first),
`--skip-smoke` (skip the post-install health check, for image builds).

## Checking fleet versions: `mde-update`

```bash
mde-update            # report-only: a table of every peer + version
```

```
HOSTNAME    VERSION   LAST SEEN
anvil       2.7.0     12s ago
forge       2.7.0     3m ago
beacon      3.0.0     1m ago      (!!)
3 peer(s) — 1 on a different MAJOR version.
```

A `(!)` marks a minor-version skew, `(!!)` a major one. Exit code is
`0` (all match), `1` (minor skew), or `2` (major skew) for scripting;
`--json` prints the same data machine-readably.

## Coordinating a fleet upgrade

Roll a new version across every peer with one command on any peer:

```bash
mde-update --coordinate 2.7.1          # default 4h grace window
mde-update --coordinate 2.7.1 --grace 1   # or override the grace (hours)
```

This writes an upgrade-intent file into the shared `mesh-storage` volume.
From there it's hands-off — every peer's `mackesd` notices it and:

1. runs `dnf upgrade mde-core` on its own schedule and marks itself
   **ready** (a peer whose repo is broken marks **failed** instead, so it
   doesn't stall everyone else);
2. once enough peers have responded *and* the grace window has passed,
   applies the new bits with `mde-install --yes` and marks itself
   **complete**;
3. a peer that was offline during the window catches up automatically the
   next time it boots — no manual step.

Roll back before the grace fires by deleting the intent:

```bash
mde-update --cancel 2.7.1
```

Completed intents clean themselves up a day later, so re-coordinating the
same version after a rollback-then-redo just works.

## Post-install health check

The last thing `mde-install` does is verify the profile actually came up
— `mackesd`/`nebula` active, the storage brick mounted (headless/full),
and the sway session present (full) — then prints
`>>> mde-install complete: profile=<X>, services=<N>/<N> up.` A failed
check exits non-zero so a scripted install knows the box isn't ready.

## Note

Running `mde-install` on a box already enrolled in the mesh removes it
from the peer registry so other peers stop counting it. A formal Nebula
cert revoke on re-install is a follow-up (it needs a mackesd `Ca.Revoke`
method that isn't wired yet); on a clean Fedora Server build-up there's
no cert or brick to tear down, so the config-state wipe above is the
complete sequence.
