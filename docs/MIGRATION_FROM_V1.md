# Migrating from Mackes Shell 1.x to Mackes Desktop Environment (MDE) 2.0.0

> CB-6.2 — step-by-step walkthrough for upgrading any v1.x
> install (1.0.x, 1.1.x) onto the v2.0.0 MDE line. Companion
> to `docs/MIGRATION_FROM_V2.2.md` which covers the older
> xfce11-unified → 1.0 transition.

## Short version

```bash
sudo dnf upgrade --refresh -y
sudo reboot
```

Next login: pick **Mackes Desktop Environment** from the
greeter session menu. Everything else happens automatically.

## What actually changes

`dnf upgrade` replaces the v1.x `mackes-shell` package with the
v2.0.0 `mde` package via the spec's `Obsoletes: mackes-shell <
2.0.0` line. The transition is **one-way** — once `mde` lands,
the v1.x XFCE + i3 stack is gone (see BREAKING CHANGES in
`CHANGELOG.md` for the full removed-deps list).

On first login under MDE:

1. The greeter shows a new session entry **Mackes Desktop
   Environment** (installed at
   `/usr/share/wayland-sessions/mde.desktop`). Pick it.
2. sway starts as the compositor (the v2.0.0 locked Wayland
   compositor — see CB-2.1).
3. `mde-session.service` (a user systemd unit, enabled by
   default via `90-mde.preset` — CB-3.6) runs the unified
   session orchestrator. It:
   - calls `mde-migrate-from-1x` to atomically move
     `~/.config/mackes-shell/` → `~/.config/mde/` (plus the
     cache + state trees).
   - calls `mde-shell-migrate-v2` to do the first-boot heavy
     lift: replay xfconf channels into the new `settings`
     table, drop the v1.x XDG-autostart overrides, back up
     `~/.config/xfce4/` to `~/.config/xfce4.v1x-backup.<ts>/`,
     and seed `~/.config/sway/` with the MDE defaults from
     `data/sway/config`.
   - autostarts the Iced panel (`mde-panel`), Iced applets,
     Iced workbench, and mde-files (the mesh-first artifact
     manager).
4. `mded` (system unit, enabled by default) provides the
   `dev.mackes.MDE.*` D-Bus surfaces every panel reads from.
5. Your existing mesh state (Headscale enrolment, mesh-VPN
   peer list, fleet config) persists — the settings move; the
   peers don't.

## Visible UI differences

- **Top + bottom panels** → single 40px bottom taskbar (Iced
  layer-shell). Same conceptual layout as the v1.1.0 Win10
  reskin: Start at left, focused-app hero at center-left,
  status cluster + clock at right.
- **Workbench** → Iced+libcosmic instead of GTK3. Every panel
  carries identical knobs to the v1.x equivalent (Themes,
  Fonts, Displays, Notifications, Session, Removable Media,
  Power, Window Manager, Fleet→Push, Fleet→Revisions, …).
- **mde-files** → the mesh-first artifact manager replaces
  Thunar. Sidebar leads with MESH (peers + inbox + outbox);
  LOCAL is collapsed at the bottom behind a disclosure.
- **Notifications** → `mded` runs as `org.freedesktop.Notifications`
  — every existing notify-send / libnotify / GTK app reaches
  it transparently. No more `xfce4-notifyd`.
- **Drawer** → Super+M / right-click panel → DND toggle +
  Caffeine toggle (flag files the
  `notifications_server` + `mde-session` workers honor).

## What's preserved across upgrade

- Every setting from `xfconf` channels you used in v1.x: the
  Phase H.5 first-boot migrator replays them into the new
  settings table.
- Mesh enrolment (Headscale token, peer fingerprints, fleet
  config).
- Your XFCE config: backed up to
  `~/.config/xfce4.v1x-backup.<timestamp>/` for reference
  before being retired.
- The v1.x `mackes` / `mackesd` / `mackes-panel` binaries: ship
  for one more release as **bin-shims** (each is a tiny shell
  script that execs the matching `mde-*` binary, so existing
  shell aliases + scripts + keyboard shortcuts keep working).
  The shims will print a one-shot deprecation warning at v2.1
  cut and disappear at v2.2.

## What changes (you may notice)

- **Compositor.** sway replaces xfwm4 + i3. Your existing i3
  keybindings (Mod+Q to close, Mod+F fullscreen, Mod+Space
  float, Mod+1..0 workspaces, Mod+J/K/L/; focus) are
  port-mapped — `data/sway/config` ships matching binding
  names so muscle memory survives.
- **Env vars rename.** `MACKES_*` → `MDE_*`. Old names still
  read for one release with a one-shot deprecation warning;
  they retire at v2.1.
- **D-Bus surfaces rename.** `org.mackes.*` →
  `dev.mackes.MDE.*`. Alias `.service` files ship for one
  release for any external script that called the old names.

## If something goes wrong

The recovery boot entry preserved from v1.x still works:

1. Reboot, pick **Mackes Recovery** from the GRUB menu.
2. The system boots into a single-user shell.
3. Run `mde recover --latest` (the v1.x snapshot helper is
   wrapped by the new binary) to restore the last
   pre-upgrade snapshot.
4. Re-`reboot`.

If you didn't take a pre-upgrade snapshot but the upgrade
landed on a broken box, file an issue at
https://github.com/matthewmackes/MAP2-RELEASES/issues with
the output of:

```bash
mde status --debug
journalctl --user -u mde-session.service --since "1 hour ago"
journalctl -u mded.service --since "1 hour ago"
```

## What v1.x users sometimes ask

**Q: My panel layout doesn't look the same.**
The Iced rewrite carries identical knobs but the Iced widgets
render slightly differently from GTK3. Theme parity is via the
new `mackes-theme` crate (E3.1) — same Carbon tokens, same
accent-per-preset. If a specific layout differs in a way that
matters to your workflow, please file an issue with a screenshot.

**Q: I'm still on i3 and want to stay.**
You can — the i3 packages aren't gone from Fedora's repos,
they're just not in MDE's deps anymore. `dnf install i3
i3status dmenu` will get them back, but you'll need to manage
the session entry yourself (MDE doesn't ship the v1.x
`i3.desktop` session anymore). The v1.x `mackes-shell` package
is no longer in the repo — once you upgrade, you're on
the MDE line.

**Q: How do I roll back to v1.x without a snapshot?**
You can't, cleanly. The hard-switch upgrade UX (Q2 lock) is
intentional — the v1.x line is end-of-life as of v2.0.0
cut. If you absolutely need to go back, the v1.x tag
(`v1.1.0`) ships an RPM at
https://github.com/matthewmackes/MAP2-RELEASES/releases/tag/v1.1.0
that you can `dnf downgrade` to manually, but the
`Obsoletes: mackes-shell < 2.0.0` line on `mde` means
you'll need to `dnf remove mde` first.
