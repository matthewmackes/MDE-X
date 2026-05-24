# Hardware bench harnesses

The Hardware Testing epic per `.claude/CLAUDE.md` §0.7 + the
"Epic: Hardware Testing" section at the bottom of
`docs/PROJECT_WORKLIST.md`. These scripts validate physical
deployments — they can't run on a dev workstation without the
appropriate hardware setup (clean Fedora 44 VM, real Android
phone for KDC2, multi-host bench fleet for mesh tests).

| Script | Worklist | Requires |
|---|---|---|
| `hw1_fresh_install.sh` | HW-1 / CB-7.1 | Fresh Fedora 44 VM, ISO URL |
| `hw2_upgrade.sh` | HW-2 / CB-7.2 | Fedora 44 with prior `mackes-xfce-workstation` install |
| `hw3_wayland_smoke.sh` | HW-3 / CB-7.3 | CI runner with `sway`, `wlr-randr` packages |
| `hw4_docker_peer.sh` | HW-4 | Docker daemon (CI or bench) |
| `kdc2_7_acceptance.sh` | KDC2-7.1..7.7 | Live MDE bench + real Android phone w/ KDE Connect |

Each script exits 0 on PASS, non-zero on FAIL, with a colored
operator-visible progress log. Operator invokes manually before
`cut release X.Y.Z` for the bench-sign-off; CI runs the
runner-friendly subset (HW-3, HW-4) via the `acceptance` job
in `.github/workflows/ci.yml`.

Per the hardware-testing carve-out, these scripts shipping is
the gate-completion signal — running them against real hardware
is the operator's bench-cadence pass + doesn't block release.
