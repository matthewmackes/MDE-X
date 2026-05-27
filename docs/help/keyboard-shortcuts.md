# Keyboard shortcuts

Every key binding that Mackes Desktop Environment defines or respects,
in one place. Surfaced inside the app from **Apple menu → Help →
Keyboard shortcuts** and shipped to `/usr/share/mde/help/` so the
headless `mde help keyboard-shortcuts` flow works too.

Bindings are grouped by the layer that handles them — the compositor,
the panel, or the workbench. MDE never overrides a binding silently:
if a shortcut would conflict with an existing user binding, the wizard
offers a "Keep mine" / "Use MDE's" / "Backup mine and use MDE's"
choice and writes the answer to `~/.config/mde/keybindings.toml`.

## Compositor (Hyprland, v6.5+)

These keys are owned by Hyprland and survive an MDE uninstall.
The binding set lives in `/usr/share/mde/hyprland.conf`; operator
overrides go in `~/.config/hypr/hyprland.conf`.

| Binding             | Action                                                 |
|---------------------|--------------------------------------------------------|
| `Super`             | Open the Apple menu (Mackes button)                    |
| `Super + Space`     | Toggle Portal-compact (mesh globe / wireframe strip)   |
| `Super + L`         | Lock screen (Portal-25 ext-session-lock-v1)            |
| `Super + M`         | Toggle the notification drawer                         |
| `Super + E`         | Open MDE Files                                         |
| `Super + Return`    | Open the default terminal (foot)                       |
| `Super + Shift + R` | Reload Hyprland config (`hyprctl reload`)              |
| `Super + Shift + E` | Exit the session (returns to greetd)                   |
| `Super + Q`         | Close focused window                                   |
| `Super + Tab`       | App-aware window switcher (Portal-28 Dock cycle)       |
| `Alt + Tab`         | Cycle visible windows                                  |
| `F3`                | Exposé grid                                            |

Workspace + layout bindings (mirrored from i3's layer cake; the
Hyprland dispatchers replace the v1.x `i3-msg` calls):

| Binding                  | Hyprland dispatcher                                       |
|--------------------------|-----------------------------------------------------------|
| `Super + 1` … `9`        | `workspace 1..9`                                          |
| `Super + Shift + 1`…`9`  | `movetoworkspace 1..9`                                    |
| `Super + H` / `V`        | `togglesplit` (horizontal/vertical follows focus)         |
| `Super + F`              | `fullscreen 1` (full-bleed) / `fullscreen 2` (real)       |

Resize is mouse-grab in v6.5 (HYP-13): grab a window edge with
`Super + middle-click drag` to live-resize. The legacy v1.x
"resize mode" submap is retired.

### v1.x XFCE / i3 baseline (for users still on 1.x)

These bindings come from xfwm4 / i3 on the legacy Mackes Shell 1.x
stack. v2.0.0+ replaces them with sway, v6.5+ with Hyprland; the
table here is for the v1.x line only.

| Binding             | Action                                            | xfwm4 | i3   |
|---------------------|---------------------------------------------------|-------|------|
| `Alt + F4`          | Close focused window                              | ✓     | —    |
| `Alt + F10`         | Toggle maximize on focused window                 | ✓     | —    |
| `Alt + F9`          | Minimize focused window                           | ✓     | —    |
| `Super + Shift + R` | Reload i3 config (`i3-msg reload`)                | —     | ✓    |

Workspaces under xfwm4 are usually collapsed to one
(`workspace_count = 1`); workspaces 2-4 only appear when i3 is the
active WM. Mackes Shell 1.x configs live at
`/usr/share/mackes-shell/i3/config`.

## Panel (`mackes-panel`)

The Rust panel owns its own shortcuts via the portal-friendly path
under Phase 11.3; today they're driven by the WM bindings above. No
panel-local key handlers run yet — every panel interaction goes through
the menu, the dock, or the status cluster.

## Workbench (Python sidebar — `mackes`)

When the Workbench window has focus, these bindings work in addition
to the standard GTK ones (`Tab` / `Shift+Tab` to navigate focus,
`Space` to activate, `Escape` to cancel a dialog).

| Binding         | Action                                             |
|-----------------|----------------------------------------------------|
| `Ctrl + F`      | Focus the Workbench search box (1.1.0+)            |
| `Ctrl + L`      | Focus the sidebar (jump back to navigation)        |
| `Ctrl + W`      | Close the current Workbench window                 |
| `Ctrl + Q`      | Quit Mackes (closes every Mackes window)           |
| `F1`            | Open the Help tab for the currently visible panel  |
| `Escape`        | Close the active dialog / cancel the current edit  |

Within a settings panel:

| Binding                   | Action                                                  |
|---------------------------|---------------------------------------------------------|
| `Ctrl + Return` / `Enter` | Apply the panel's edits                                 |
| `Ctrl + Z`                | Revert the last unapplied edit (where supported)        |
| `Ctrl + .`                | Open the Drift card for this panel (Maintain → Drift)   |

## Drawer (`mackes --drawer`)

Drawer-specific bindings work while the drawer is the foreground
window. `Super + M` toggles the drawer from anywhere.

| Binding            | Action                                       |
|--------------------|----------------------------------------------|
| `Escape`           | Close the drawer                             |
| `Tab` / `Shift+Tab`| Cycle focus through the quick-toggle row     |
| `Ctrl + N`         | Jump to the Notifications section            |
| `Ctrl + M`         | Jump to the Mesh section                     |
| `Ctrl + F`         | Jump to the Fleet section                    |

## CLI flags that mirror keyboard actions

```sh
mackes                 # open the Workbench (no flags)
mackes --drawer        # open the drawer
mackes --drawer --drawer-focus mesh   # open + scroll to Mesh section
mackes --about         # open the About Mackes window
mackes --wizard        # force the first-run wizard
mackes-wm i3           # live-switch to i3
mackes-wm xfwm4        # live-switch back to xfwm4
mackes-wm status       # print the running WM
```

These are useful for binding from `xfce4-keyboard-settings` or any other
shortcut surface — every keyboard binding above can be reproduced by
spawning the matching CLI invocation.

## Customizing

`~/.config/mackes-panel/panel.toml` carries a `[keybindings]` section
(empty by default — every shortcut above comes from the system layer).
Adding a key here overrides the system binding for the active session:

```toml
[keybindings]
"drawer"      = "super+grave"     # was super+m
"apple_menu"  = "super+w"         # was super+space (and super)
"lock_screen" = "ctrl+alt+l"      # was super+l
```

Save the file — `mackes-panel` watches it via inotify and reapplies the
bindings within ~1 s. Backups of any system bindings Mackes overrides
live at `~/.config/mackes-panel/keybindings.backup.toml`.

## Accessibility

Every interactive panel widget exposes an AT-SPI name + description
(Phase 11.2 — landed in 1.0.7). Screen readers announce the status
cluster as "Mesh: 3 peers online" / "Battery: 87 percent" /
"Notifications: 1 unread" rather than just "button". Focus order
follows reading order (top-to-bottom, left-to-right) on every panel.

If a screen reader does NOT pick up a panel's content, please file an
issue: it's almost certainly a missing `set_accessible_name` call on
the offending widget.
