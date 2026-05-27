# Wayland support

**v6.5 finalizes the Wayland-only stance with a hard cut to
Hyprland.** Mackes Desktop Environment (MDE) v6.5 ships Hyprland
as the locked compositor, replacing the v2.0.0 sway lock. The
panel + applets remain Iced + libcosmic on layer-shell; what
changes is the IPC layer (swayipc → hyprland-rs) + the config
file (`~/.config/sway/config` → `~/.config/hypr/hyprland.conf`).
The legacy "Mackes Shell" 1.x stack (XFCE + i3 + xfsettingsd,
all X11) is the documented baseline below for users still on
the 1.x line.

## Status line on v6.5

If you are running MDE v6.5:

- **Hyprland is the locked compositor.** All v6.5 builds ship
  Hyprland bundled inside the `mde` RPM (no external repo
  required) + the Iced + libcosmic panel + applets on
  layer-shell. There's no opt-in/out.
- **Baseline config lives at `/usr/share/mde/hyprland.conf`.**
  Your `~/.config/hypr/hyprland.conf` sources the baseline and
  layers operator overrides below it. Per-peer monitor overrides
  live at `~/.config/mde/peers/<hostname>/hyprland-monitors.conf`.
- **wlroots-family compositors (sway, river, niri) are
  unsupported on v6.5.** They might run the Iced panel +
  mde-files, but we don't test them; Hyprland is the only one
  CI gates green on.
- **GNOME / KDE Wayland sessions are explicitly NOT supported.**
  The panel relies on wlr-protocols (layer-shell,
  foreign-toplevel-management, data-control) that those
  compositors don't expose.

## What does work on Wayland today

- `mde wm probe-wayland` reports `XDG_SESSION_TYPE`,
  `WAYLAND_DISPLAY`, `DISPLAY` (Xwayland fallback), and the
  matching `hyprctl` connection state.
- `mde wm session-pick` lists every installed
  `/usr/share/wayland-sessions/*.desktop` so you can switch
  greeter session entries.

## Common operator tasks via hyprctl

A few patterns that operators reach for. Each one corresponds
to a Hyprland dispatcher invoked through `hyprctl`.

    # List active windows + their classes + workspaces.
    hyprctl clients

    # Rename the current workspace.
    hyprctl dispatch renameworkspace 3 "Email"

    # Move the focused window to workspace 4.
    hyprctl dispatch movetoworkspace 4

    # List every windowrulev2 currently in effect.
    hyprctl rules

    # Reload the active hyprland.conf without dropping the
    # session (picks up your edits to ~/.config/hypr/hyprland.conf
    # and per-peer monitor overlays).
    hyprctl reload

    # Inspect a single keyword's current value.
    hyprctl getoption misc:allow_tearing

Operator overrides go in the override block of your
`~/.config/hypr/hyprland.conf` (below the `source = ...` line
that pulls in the baseline). Save the file → `hyprctl reload`
applies the changes; no session restart required.

## Allow tearing for games

MDE ships with `allow_tearing = false` globally in
`/usr/share/mde/hyprland.conf` (HYP-26). Tearing artifacts are
hidden by default for typical desktop work. To opt a specific
game class in, add a `windowrulev2 = immediate, class:<regex>`
line to the operator override block of `~/.config/hypr/hyprland.conf`
and reload with `hyprctl reload`:

    # Example: enable tearing for Steam games (immediate
    # presentation skips compositor vsync on that window class).
    windowrulev2 = immediate, class:^(steam_app_.*)$

The setting applies per-window; non-matching windows still
present through the compositor with tearing off. To verify on a
running session, run `hyprctl getoption misc:allow_tearing` and
launch a matched game — the `immediate` rule lets the GPU bypass
vsync for that surface only.

## Reverting to v1.x XFCE/X11

Not supported in the v6.5 line. The dnf hard switch
(`Obsoletes: mackes-shell < 2.0.0`) means once you upgrade you're
on MDE. To go back you need a pre-upgrade snapshot — see
`docs/MIGRATION_FROM_V1.md` § "If something goes wrong".

## See also

- [Keyboard shortcuts](keybindings.md) — Hyprland-managed; the
  v1.x i3 bindings carry over with the same modifiers.
- [Troubleshooting](troubleshooting.md) — Hyprland / mde-session
  recovery.
- `docs/design/v6.5-hyprland-compositor.md` — Hyprland migration
  design lock + 30-Q survey.
