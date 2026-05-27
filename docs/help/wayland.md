# Wayland support

**v2.0.0 reverses the historical X11-only stance.** Mackes Desktop
Environment (MDE) 2.0.0 is **Wayland-only** — sway is the compositor,
the panel is Iced + libcosmic on layer-shell, applets use
wlr-protocols. The legacy "Mackes Shell" 1.x stack (XFCE + i3 +
xfsettingsd, all X11) is the documented baseline below for users still
running 1.x — see `docs/design/wayland-readiness.md` for the per-
surface audit and the Phase E rewrite tracker.

## Status line on v2.0.0

If you are running MDE 2.0.0:

- **sway is the locked compositor.** All v2.0.0 builds ship sway
  + the Iced + libcosmic panel + applets on layer-shell. There's
  no opt-in/out — the rest of this section covers what was true
  on the v1.x line for reference.
- **wlroots compositors (Hyprland, river, niri) are unsupported
  alternatives.** They might run the Iced panel + mde-files, but
  we don't test them; sway is the only one that gates green in
  CI (CB-7.3 Wayland smoke).
- **GNOME / KDE Wayland sessions are explicitly NOT supported.**
  The panel relies on wlr-protocols (layer-shell,
  foreign-toplevel-management, data-control) that those
  compositors don't expose.

## What does work on Wayland today

- `mde wm probe-wayland` reports `XDG_SESSION_TYPE`,
  `WAYLAND_DISPLAY`, `DISPLAY` (Xwayland fallback), and the
  matching `swayipc` connection state.
- `mde wm session-pick` lists every installed
  `/usr/share/wayland-sessions/*.desktop` so you can switch
  greeter session entries.

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

Not supported in the v2.0.0 line. The dnf hard switch
(`Obsoletes: mackes-shell < 2.0.0`) means once you upgrade you're
on MDE 2.0.0. To go back you need a pre-upgrade snapshot — see
`docs/MIGRATION_FROM_V1.md` § "If something goes wrong".

## See also

- [Keyboard shortcuts](keybindings.md) — sway-managed; the v1.x
  i3 bindings carry over with the same modifiers.
- [Troubleshooting](troubleshooting.md) — sway / mde-session
  recovery.
- `docs/design/wayland-readiness.md` — historical per-surface
  audit (developer doc; the items there are all closed by
  Phase E.x).
