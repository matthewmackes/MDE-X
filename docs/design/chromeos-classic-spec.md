# Classic ChromeOS visual lock (v2.6+, locked 2026-05-24)

This document is the **canonical design lock** for every UI surface
the MDE platform ships. It supersedes the prior Win11 / Ableton
influence split codified in `.claude/skills/iteration/SKILL.md` per
the **newer-wins-silently** rule (mackes-worklist-management §1) and
replaces the visual-identity.md Q3 charcoal palette + Q11/Q12
typography per the same rule.

**Locked via 22-Q operator survey 2026-05-24:**

- Round 1 (3 Q): era + typography + accent/icon reconciliation.
- Round 2 (4 Q): palette + light mode + window chrome + density.
- Round 3 (15 Q, 4 + 4 + 4 + 3 batches): Shelf, app sidebar,
  selection, hover, focus ring, primary button, text input,
  toggle/checkbox, card elevation, context menu, dialog modal,
  toast/notification chip, scrollbar, tooltip + kbd chip, disabled.

**Standing operator directives** (folded in mid-survey):

1. **mde-files layout does not change** — the existing sidebar /
   list / toolbar structure stays; only the visual treatment swaps
   to Classic ChromeOS.
2. The spec applies to **every surface** in MDE (chrome AND
   content; apps AND system). No Win11/Ableton split.

## Influence reference

**Classic ChromeOS pre-2022** — flat, dense, 4 px corners, no
blur, hard 1 px dividers, accent only where needed.

Reference shots (operator memory): ChromeOS Settings 2018-2021,
the pre-Material-You Files app, the original Shelf + Status Tray
rhythm.

## Surviving MDE locks (not overridden)

- **Accent:** Q2 indigo `#5b6af5`. Single accent across all
  surfaces (no per-zone variation).
- **Iconography:** Carbon Icon Set ONLY. Source: baked
  `assets/icons/carbon/*.svg`. No Lucide / Phosphor / Material /
  Font Awesome / hicolor / Orchis / Black-Sun in production.
- **Voice + tone:** `docs/design/voice-and-tone.md` unchanged.

## Locks superseded by this spec

| Old lock | Replacement | Source |
|---|---|---|
| Q3 charcoal `#1d1d1f` | Classic ChromeOS dark palette below | visual-identity.md Q3 |
| Q11 Geologica body font | Roboto | visual-identity.md Q11 |
| Q12 IBM Plex Mono | Roboto Mono | visual-identity.md Q12 |
| Win11 chrome / Ableton content split | Classic ChromeOS everywhere | iteration skill `History` 2026-05-23 |
| BUG-16 per-window controls (kept geometry, see Window chrome below) | Classic ChromeOS tab-strip header | iteration skill `History` 2026-05-23 |
| v1.1.0 Win10 40 px taskbar | 48 px Classic ChromeOS Shelf | project_1_1_0_win10_layout memory |

## Palette (dark mode default)

```
Background          #202124   page surface
Surface raised      #2d2e30   cards, popovers, hover surfaces
Surface active      #3c4043   active/pressed, sliders, dividers
Divider             #3c4043   1 px, sharp
Text primary        #e8eaed
Text muted          #9aa0a6
Accent              #5b6af5   Q2 indigo (unchanged)
Accent fg on accent #ffffff
Backdrop alpha      #000 @ 60% (dialog overlay)
```

## Palette (light mode — full retrofit epic, see worklist)

```
Background          #f7f7f7
Surface raised      #ffffff
Surface active      #e8eaed
Divider             #dadce0
Text primary        #1d1d1f
Text muted          #5f6368
Accent              #4051d3   Q2 indigo, darker pair
Accent fg on accent #ffffff
Backdrop alpha      #000 @ 40%
```

Light mode follows `XDG_COLOR_SCHEME`, with a per-app override in
the Workbench Appearance panel. Until the retrofit epic ships,
every surface stays dark-only — light-mode tokens are reserved
above so consumers can compile against them without churning the
token names later.

## Typography

```
UI body           Roboto 400        13 px / 18 px line
UI heavy          Roboto 500        13 px / 18 px line
Display title     Roboto 400        22 px / 28 px line
Section header    Roboto Medium     11 px / 14 px line / +0.5 px letter-space
Page title        Roboto 500        18 px / 24 px line
Monospace         Roboto Mono 400   12 px / 16 px line
Letter-spacing    0.0 default,      +0.25 px on small caps
```

Roboto + Roboto Mono ship with Fedora's `google-roboto-fonts` +
`google-roboto-mono-fonts` packages — no bundling required; the
RPM gains a dependency declaration in `mackes-shell.spec`.

## Density

```
Default row height       28 px
Default icon size in row 16 px
Standard pad H           12 px
Spacing grid             4 / 8 / 16 px
Standard control height  32 px (buttons, inputs)
```

## Window chrome — Classic ChromeOS tab-strip header

```
Header height        32 px
Active title         tab-shape chip, 4 px top corners
Tab chip bg          #2d2e30
Tab chip border      1 px #3c4043 (left, right, top only)
Tab chip text        13 px Roboto / #e8eaed
Window controls      top-right, 16 px Carbon glyphs
Controls spacing     12 px between, 12 px from edge
Min / Max / Close    Carbon: subtract / maximize / close
Inactive header      clickable to focus the window
Background           1 px shaded vs window body
```

The BUG-16 per-window-controls-top-right rule is preserved; only
the header treatment around them updates to the tab-strip
vocabulary.

## Shelf (bottom panel)

```
Shelf height         48 px
Background           #202124 (matches app surface)
Top divider          1 px #3c4043
App icons            32 px Carbon, centered horizontally
Icon spacing         8 px between
Launcher (M btn)     bottom-left, 40 × 40 px hit, 24 px Carbon
Status Tray          bottom-right, 200 px wide, popover on click
Clock format         h:mm AM/PM
Right-click app icon Context menu (Carbon icons + Kbd chips)
```

## App sidebar (Workbench / mde-files / wizard)

```
Resting width        56 px (icon-only)
Hovered width        256 px
Hover delay          140 ms before expand
Animation            width transition 200 ms ease-out
Row height           28 px
Icon size            16 px Carbon
Text                 13 px Roboto
Pad H                12 px
Group header         11 px Roboto Medium, +0.5 px letter-space, muted
Divider              1 px #3c4043 between groups
Selected row         solid Q2 indigo fill, #ffffff text + icon
Tooltip when collapsed:  500 ms hover delay, label only
```

The hover-expand idiom replaces any always-expanded sidebar in
the Workbench + wizard. mde-files keeps its existing structure
per the layout-no-change directive; only the chrome treatment
updates.

## Selection

```
Selection bg        #5b6af5 (Q2 indigo, full opacity)
Selection text      #ffffff
Selection icon      #ffffff
Corners             sharp
Multi-select        same treatment per row
Range select        Shift-click / Shift-arrow per platform conv
```

## Hover / active state

```
Default bg     #202124
Hover bg       #2d2e30 (one notch up; same as raised surface)
Active bg      #3c4043 (one more notch; same as divider color)
Transition     none (instant)
Hover does not apply when element is disabled.
```

## Focus ring (keyboard nav only)

```
Outline        2 px solid #5b6af5
Offset         1 px from element edge
Corners        sharp (matches 4 px row scale)
Applies to     buttons, inputs, rows, links, icons, toggles
When           Tab-key focus only — mouse focus is silent
```

## Primary button

```
Background     #5b6af5
Text           #ffffff Roboto 500 / 13 px
Height         32 px
Pad H          16 px
Corners        4 px
Hover bg       +8% luminance on #5b6af5
Active bg      -8% luminance on #5b6af5
Disabled       40% opacity, cursor not-allowed
Icon (leading) 16 px Carbon, 8 px R pad before text
```

Secondary buttons use the outlined treatment (1 px indigo
border, transparent fill, indigo text). Tertiary buttons use the
text-only treatment (indigo text, no border, transparent fill).

## Text input

```
Background     transparent
Border bottom  1 px #3c4043
Focus border   2 px #5b6af5 (bottom only)
Height         32 px
Text           13 px Roboto / #e8eaed
Placeholder    13 px Roboto / #9aa0a6
Pad            0 H, 6 px V
Leading icon   16 px Carbon, 8 px R pad
Trailing icon  16 px Carbon, 8 px L pad (clear, reveal, etc.)
Helper text    11 px / #9aa0a6 below the input
Error helper   11 px / #ea4335 below the input
```

## Toggle / checkbox / radio

```
Toggle pill           32 × 16 px
Toggle knob           12 px circle
Toggle slide          140 ms ease-out
Toggle on bg          #5b6af5
Toggle off bg         #3c4043
Toggle knob color     #ffffff

Checkbox              16 px sharp square
Checkbox border       1 px #3c4043 when off
Checkbox bg on        #5b6af5
Checkbox check        16 px Carbon checkmark glyph, #ffffff

Radio                 16 px circle
Radio border          1 px #3c4043 when off
Radio bg on           transparent
Radio dot on          8 px circle, #5b6af5

Label pad L           8 px from control
Label text            13 px Roboto
```

## Card surface (Bento tile, stat box, capability tile)

```
Background     #202124 (same as page — no fill change)
Border         1 px #3c4043
Corners        4 px
Pad            16 px
Shadow         none
Heading inside 13 px Roboto 500
Body inside    13 px Roboto / #e8eaed
Divider inside 1 px #3c4043 between sections
```

Cards are bordered regions on the page background. No raised
fill, no shadow — flat by lock.

## Right-click context menu

```
Width min            220 px (grows to fit longest label)
Row height           28 px
Corners              4 px
Border               1 px #3c4043
Background           #2d2e30
Group divider        1 px #3c4043 between groups
Icon column          16 px Carbon, 12 px L pad
Label                13 px Roboto, 8 px L pad after icon
Kbd shortcut col     right-aligned, 11 px muted, 12 px R pad
Submenu indicator    Carbon chevron-right, 12 px R pad
Disabled row         40% opacity, not-allowed cursor
```

## Dialog modal

```
Corners              4 px
Background           #2d2e30
Border               1 px #3c4043
Backdrop             #000 @ 60%
Default width        480 px (resizable up to 75% viewport)
Title row            48 px, 18 px Roboto 500, 16 px L/R pad
Body                 13 px Roboto, 16 px pad
Button row           64 px, right-aligned, 8 px gap
Primary button       right of cancel
Cancel button        outlined (secondary)
Close X              top-right, 16 px Carbon, 12 px from edge
ESC                  triggers cancel
Enter                triggers primary
```

## Toast / notification chip

```
Position             bottom-right, 8 px above Shelf
Width                320 px
Max height           auto (caps at 200 px; long text truncates)
Corners              4 px
Border               1 px #3c4043
Background           #2d2e30
Title text           13 px Roboto 500
Body text            13 px Roboto / #e8eaed
Action button row    32 px, right-aligned, 8 px gap
Auto-dismiss         5 s
Dismiss indicator    2 px bottom progress strip (indigo)
Hover                pauses auto-dismiss
Stack                newest on top, 8 px gap between toasts
Close X              top-right, 12 px Carbon
```

## Scrollbar

```
Width                12 px (always shown — reserves layout width)
Track                #2d2e30 always-visible
Thumb                #3c4043
Hover thumb          #5a5d61
Active thumb         #6c7075
Corners              sharp
```

## Tooltip + keyboard-shortcut hint chip

```
Tooltip background   #2d2e30
Tooltip border       1 px #3c4043
Tooltip corners      4 px
Tooltip text         12 px Roboto / #e8eaed
Tooltip pad          8 px H, 6 px V
Hover delay          500 ms before show
Hide delay           100 ms after mouse leaves
Position             below element by default, flips above if clipped

Kbd chip background  transparent (no fill)
Kbd chip border      1 px #3c4043
Kbd chip text        10 px Roboto Mono / #e8eaed
Kbd chip corners     4 px
Kbd chip pad         4 px H, 2 px V
Kbd join             ' + ' separator OUTSIDE the chips (each key in own box)
```

## Disabled / inactive state

```
Opacity              40%
Cursor               not-allowed
Hover                no change — disabled controls do not light up
Focus                element skipped in tab order
Applies to           buttons, inputs, rows, icons, toggles, toolbar items
```

## Object Cards — Material Design layer (locked 2026-05-24)

**Operator directive (folded in mid-survey):**

> "Part of the refresh is to treat all items as files, and those
> are represented as cards in the Start menu, and File Menu.
> Those Cards should be stylized with Material Design, and have a
> Material Design 'card' aesthetic. These Cards should represent
> these objects throughout the interface."
>
> "Applications Also Render as cards, as do files."

This is a **layered rule** on top of the Classic ChromeOS lock,
not a replacement. Most surfaces (rows, lists, settings, tables,
sidebars, popovers, dialogs, toasts, chrome) follow Classic
ChromeOS as locked above. But wherever the surface renders an
**Object** — an app, a file, a peer, a saved network, a paired
phone, a stored credential, a recent document — that object
**must render as a Material Design Object Card**, not as a
ChromeOS row.

### Scope — what counts as an Object

Bench-observable rule: if the operator can drag the thing, open
it, share it, or right-click for a context menu on it as a
discrete unit, it's an Object and renders as a Card.

Concrete inventory:

| Surface | Objects rendered as Cards |
|---|---|
| Start menu (`mde-start`) | Application entries (current + recent + pinned) |
| File menu / mde-files | Every file + folder row in grid view |
| Workbench Mesh Topology | Each peer rendered as a peer card |
| Workbench Networking | Each saved Wi-Fi / VPN connection |
| Workbench Phones (KDC2) | Each paired phone |
| Workbench Credentials | Each saved credential / key |
| Workbench Recent | Each recent document / activity entry |
| Notifications history | Each toast in the history pane |
| Birthright resume | Each resumable workflow step |

Things that stay as ChromeOS rows (NOT Cards): settings entries,
log lines, table data, sidebar nav items, toolbar items,
breadcrumbs, status chips.

### Card spec (locked 2026-05-24, 4-Q survey re-asked)

```
Variant            Material 3 Elevated card
Background         surface-tint @ 5 % over base (#202124)
                   resolves to ~#2b2c2e in dark mode
Shadow             0 1 px 3 px rgba(0,0,0,0.30),
                   0 1 px 2 px rgba(0,0,0,0.15)
Corners            12 px (medium, M3 default)
Border             none in default state
Pad                16 px
Hover              elevation +1 (shadow 0 2 px 6 px rgba(0,0,0,0.35))
                   + 8 % white overlay on the surface tint
Press              elevation +2 (shadow 0 4 px 10 px rgba(0,0,0,0.40))
                   + indigo ripple at #5b6af5 @ 30 %, 300 ms
                   centred on press point
Selected           2 px Q2 indigo border + 15 % indigo overlay
                   (multi-select renders the same per card)
Focus (kbd)        2 px Q2 indigo outline 1 px offset (matches
                   the platform focus-ring lock)
Disabled           40 % opacity, not-allowed cursor (matches
                   the Classic ChromeOS disabled rule)

Content shape      icon + title + 1-line subtitle (compact)
Icon (leading)     32 px Carbon (S/M sizes)
Icon (top)         40 px Carbon (M/L sizes)
Title              14 px Roboto 500 / #e8eaed
Subtitle           12 px Roboto / #9aa0a6
                   (one line, ellipsis on overflow)

Sizing (S / M / L) S =  160 × 72  px   (Workbench inline lists)
                                       leading icon 28 px
                   M =  180 × 100 px   (mde-files grid view)
                                       top icon 40 px
                   L =  200 × 140 px   (Start menu grid)
                                       top icon 48 px
Grid gap           12 px between cards
Grid wrap          to viewport width

Interaction        Click          = primary action (open / launch)
                   Right-click    = context menu
                   Shift-click    = range select
                   Ctrl-click     = add to / remove from selection
                   Long-press     = context menu (touch)
                   Drag           = entire card is grab handle
                   ESC            = clear selection
                   Enter          = open focused card
```

The compact content shape is locked (round 4 re-ask) over the
earlier rich-with-meta-row variant. Surfaces that NEED a meta
row (e.g. mesh-peer cards with transport / latency chips) get a
follow-up `CR-meta-row` task to extend the component, not a
parallel non-Card layout.

### Why 12 px corners on Cards when the rest of MDE is 4 px

This is an intentional break from the 4 px Classic ChromeOS
corner rule. Material Design Cards earn the larger radius because
the rounding is the visual signal that says "this is a tangible
object you can pick up." A 4 px Object Card reads as just another
ChromeOS row, defeating the purpose of the card affordance.

The 12 px rule applies ONLY to Object Cards. Buttons stay 4 px,
dialog windows stay 4 px, popovers stay 4 px, toasts stay 4 px.
Phase 0.8's audit treats `radius: 12 px` as a finding UNLESS the
surrounding code is an Object Card (grep context: `MaterialCard`,
`ObjectCard`, `app_card`, `file_card`, `peer_card`).

### Why M3 shadows when the rest of MDE is shadow-free

Same justification — the elevation is the affordance. Object
Cards earn shadows; rows, panels, sidebars, dialogs do not.

## Iconography

Carbon Icon Set ONLY. Bake source: `assets/icons/carbon/*.svg`.
Resolver: `mde_theme::Icon::carbon_name()` →
`ResolvedIcon::svg_bytes()`. New variants must land an SVG +
matching arm before consuming feature flips to `[✓]`.

Sizes by surface:
- Sidebar / row / toolbar: 16 px
- Carbon glyph on Shelf icon: 32 px
- Launcher button: 24 px
- Window controls (min/max/close): 16 px
- Status Tray glyphs: 16 px
- Card heading icons: 16 px
- Hero / capability row leading icon: 24 px

## Voice + tone

Unchanged — `docs/design/voice-and-tone.md` continues to govern
copy. The Classic ChromeOS visual lock does not touch wording.

## Audit hooks

This lock interacts with the iteration skill's Phase 0.8 design-
criteria audit. The next Phase 0 sweep should:

1. Grep every crate for `#1d1d1f`, `#202020`, `Geologica`,
   `IBM Plex Mono` — flag every hit as a Classic-ChromeOS-retrofit
   task.
2. Grep for `radius:`, `border_radius:` values > 4 px in
   production code — flag as an over-rounding finding.
3. Grep for `8.0`, `12.0`, `16.0` (Iced corner literals) in
   `crates/mde-*` — manual review for Classic ChromeOS compliance.
4. Compare each `docs/design/*.md`'s locked palette / typography
   sections against this spec; rewrite per newer-wins-silently.

## Worklist epic

Surface-by-surface retrofit is tracked in the worklist under
`### CR-1..CR-N: v2.6 — Classic ChromeOS retrofit (locked
2026-05-24)`. Each surface gets its own task with story format +
bench-observable acceptance per
mackes-worklist-management §1.1.
