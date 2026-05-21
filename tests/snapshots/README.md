# Visual-regression snapshot goldens

**Owner:** UX-23 (CI gate) + UX-13 (gallery, source of goldens).
**Worklist:** see `docs/PROJECT_WORKLIST.md` UX-23, UX-13, UX-26.
**Last updated:** 2026-05-21 (placeholder — no goldens yet).

This directory holds the golden PNG snapshots used by the
`ui-snapshot.yml` CI workflow (UX-23) to detect visual regressions
in the MDE design system.

## Layout

```
tests/snapshots/
├── README.md                                    (this file)
├── dark/
│   ├── compact/
│   │   ├── button-rest.png
│   │   ├── button-hover.png
│   │   ├── button-active.png
│   │   ├── button-focus.png
│   │   ├── button-focus-visible.png
│   │   ├── button-disabled.png
│   │   ├── button-loading.png
│   │   ├── button-error.png
│   │   ├── button-success.png
│   │   ├── input-rest.png
│   │   …
│   ├── comfortable/
│   │   …
│   └── spacious/
│       …
└── light/
    └── (mirror of dark/)
```

**Components covered (per UX-13 / UX-26):** button, input, toggle,
dropdown, tab, nav-item, list-row, card, badge, tooltip,
scrollbar.

**States covered (per UX-13):** rest, hover, active, focus,
focus-visible, disabled, loading, error, success, empty.

**Themes (per Q5 / FU-2):** dark, light — full parity.

**Densities (per Q26 / Q27 / UX-15):** compact, comfortable,
spacious.

Total expected goldens at full coverage: ~440 (some states
N/A for some components — scrollbar has no "loading", etc.).

## Workflow

### Regenerating goldens

```bash
make snapshots-regen
```

Runs `cargo run --example gallery -p mde-theme` for each
(theme, density) combo, captures each cell to PNG, writes into
`tests/snapshots/{theme}/{density}/{component}-{state}.png`.
Overwrites existing files. Commit the diff in a PR with the
`design-review` label.

### Local diff check (no CI, no HW-3 needed)

```bash
make snapshots-local
```

Same capture flow, but writes to `tests/snapshots-local/` and
runs a 0.5% Lab-distance diff against the committed goldens.
Outputs a diff report to `tests/snapshots-local/diff/`. Attach
to PRs during the HW-3 deferral window (UX-26 fallback).

### CI gate (UX-23, post HW-3)

`.github/workflows/ui-snapshot.yml` runs the same flow under
the Wayland-in-Docker runner from HW-3, compares against the
committed goldens, posts the diff image inline on the PR for
visual review. PRs touching the design system MUST pass the
diff OR land with a `design-review` label + reviewer sign-off.

## Diff tolerance

0.5% Lab-distance via the `image-compare` crate (added as
dev-dependency to `crates/mde-theme/Cargo.toml`). Pixel-exact
comparison is too brittle — subpixel-rendering and font-
hinting variance across runners produces false positives.
Lab-distance is robust against those without hiding real
regressions.

## Storage budget

8-bit PNG per golden, expected ≤ 8 KB per file (gallery cells
are small — 240 × 80 typical). Full coverage budget ≈ 3.5 MB.

## When to regenerate

- A design lock changes (palette, spacing, type, motion).
- A `mde-theme` component gains a new state or visual variant.
- A new density mode is added (currently 3; future revision
  may add 4th).
- A theme is added (currently dark + light).

Do **not** regenerate to "fix" a CI failure without first
confirming the visual change is intentional. The `design-review`
label exists specifically to gate that conversation.
