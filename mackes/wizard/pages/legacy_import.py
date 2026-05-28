"""Wizard screen — Legacy Import (Phase 10.2; v3.0.0 Q49).

Sits between Welcome and Preset Pick. Detects any 2.x leftovers under
``~/.config/mackes-shell/`` and presents a checklist of what will be
folded forward into ``~/.config/mackes-panel/panel.toml``. The user
hits *Import* to execute the migration; the page then disables the
button and renders a "Done" banner so it's safe to navigate forward.

When ``detect()`` returns ``None`` (fresh install) — or when it
raises (corrupted legacy state) — the page renders a friendly
"nothing to import" message and never blocks the wizard.

Layout::

    Migrate from 2.x?
    -----------------
    Mackes found settings from a previous installation. Pick what to
    carry forward.

      [✓] Preset:     hashbang
      [✓] Wallpaper:  ~/Pictures/sunset.jpg
      [✓] Pinned apps (3): firefox · org.gnome.Terminal · gimp
      [✓] Drawer overrides (2 keys)

    [ Import ]    (after click: "Imported — see log below")
"""
from __future__ import annotations

import logging
from pathlib import Path
from typing import Optional

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.legacy_import import LegacyState, detect, import_to_panel_toml
from mackes.gtk_common import (
    a11y, info_label, section_header, title_label,
)

logger = logging.getLogger(__name__)


def build(ctx) -> Gtk.Widget:
    """Construct the wizard page.

    Safe under degraded conditions — any failure inside ``detect()`` is
    logged and the page falls through to the "Fresh install" branch.
    """
    try:
        legacy: Optional[LegacyState] = detect()
    except Exception as exc:  # noqa: BLE001
        logger.warning(
            "legacy_import: detect() raised %s — rendering fresh-install branch",
            exc,
        )
        legacy = None

    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=14)
    box.set_margin_top(48); box.set_margin_bottom(32)
    box.set_margin_start(56); box.set_margin_end(56)

    title = title_label("Migrate from 2.x?")
    title.set_halign(Gtk.Align.START)
    box.pack_start(title, False, False, 0)

    if legacy is None:
        return _render_fresh_install(box)

    return _render_migration_summary(box, legacy)


# ---------------------------------------------------------------------------
# branches
# ---------------------------------------------------------------------------


def _render_fresh_install(box: Gtk.Box) -> Gtk.Widget:
    box.pack_start(
        info_label(
            "Fresh install — nothing to import. Mackes did not find an "
            "earlier configuration under ~/.config/mackes-shell/. "
            "Continue to pick your preset."
        ),
        False, False, 0,
    )
    return box


def _render_migration_summary(box: Gtk.Box, legacy: LegacyState) -> Gtk.Widget:
    box.pack_start(
        info_label(
            "Mackes found settings from a previous installation. Review "
            "what will be carried forward, then click Import."
        ),
        False, False, 0,
    )

    box.pack_start(section_header("To import"), False, False, 0)

    rows = _build_summary_rows(legacy)
    for row in rows:
        box.pack_start(row, False, False, 0)

    # ---- import button -------------------------------------------------
    button_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    button_row.set_margin_top(20)
    btn = Gtk.Button(label="Import")
    btn.get_style_context().add_class("suggested-action")
    btn.get_style_context().add_class("cds-button-primary")
    a11y(btn, name="Import legacy 2.x settings into panel.toml",
         tooltip="Fold legacy preset + pinned apps + wallpaper forward.")
    button_row.pack_start(btn, False, False, 0)
    status_lbl = Gtk.Label()
    status_lbl.set_xalign(0)
    status_lbl.set_line_wrap(True)
    status_lbl.get_style_context().add_class("mackes-page-subtitle")
    button_row.pack_start(status_lbl, True, True, 8)
    box.pack_start(button_row, False, False, 0)

    # ---- log view (hidden until import runs) ---------------------------
    log_buf = Gtk.TextBuffer()
    log_view = Gtk.TextView(buffer=log_buf)
    log_view.set_editable(False)
    log_view.set_cursor_visible(False)
    log_view.set_monospace(True)
    log_view.get_style_context().add_class("mackes-log-view")
    log_scroll = Gtk.ScrolledWindow()
    log_scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
    log_scroll.set_min_content_height(160)
    log_scroll.add(log_view)
    log_scroll.set_no_show_all(True)
    box.pack_start(log_scroll, True, True, 0)

    def _do_import(_btn):
        btn.set_sensitive(False)
        try:
            lines = import_to_panel_toml(legacy)
        except Exception as exc:  # noqa: BLE001
            logger.exception("legacy_import: import_to_panel_toml failed")
            status_lbl.set_text(f"Import failed: {exc}")
            btn.set_sensitive(True)
            return
        log_buf.set_text("\n".join(lines))
        log_scroll.show()
        log_scroll.set_no_show_all(False)
        status_lbl.set_text("Done — settings folded into panel.toml.")

    btn.connect("clicked", _do_import)
    return box


# ---------------------------------------------------------------------------
# summary row builders
# ---------------------------------------------------------------------------


def _build_summary_rows(legacy: LegacyState) -> list[Gtk.Widget]:
    """Render one row per non-empty field in ``legacy``.

    Rows are read-only checkboxes — they communicate "this will be
    imported" without offering per-field opt-out (Q49 lock: import is
    all-or-nothing, the user can edit panel.toml later if needed).
    """
    rows: list[Gtk.Widget] = []
    if legacy.preset_name:
        rows.append(_summary_row(
            f"Preset:    {legacy.preset_name}",
            "Recorded in state.json and panel.toml's [migration] table.",
        ))
    if legacy.wallpaper_path:
        wp = Path(legacy.wallpaper_path).expanduser()
        suffix = "" if wp.is_file() else "  (file missing — path recorded only)"
        rows.append(_summary_row(
            f"Wallpaper: {legacy.wallpaper_path}{suffix}",
            "Recorded in panel.toml's [migration] table.",
        ))
    if legacy.pinned_apps:
        preview = ", ".join(legacy.pinned_apps[:3])
        extra = (
            f" + {len(legacy.pinned_apps) - 3} more"
            if len(legacy.pinned_apps) > 3 else ""
        )
        rows.append(_summary_row(
            f"Pinned apps ({len(legacy.pinned_apps)}): {preview}{extra}",
            "Appended to dock.items; existing pins are preserved.",
        ))
    if legacy.recents:
        rows.append(_summary_row(
            f"Recents ({len(legacy.recents)}) — dropped",
            "No 1.x recents surface; entries are logged then discarded.",
        ))
    if legacy.drawer_overrides:
        known = {"show_appmenu", "status_items", "mesh_replicate",
                 "mesh_drift_seconds"}
        known_count = sum(1 for k in legacy.drawer_overrides if k in known)
        unknown_count = len(legacy.drawer_overrides) - known_count
        line = f"Drawer overrides ({known_count} mapped"
        if unknown_count:
            line += f", {unknown_count} dropped"
        line += ")"
        rows.append(_summary_row(
            line,
            "Known keys fold into top_bar.* / mesh.*; others are logged.",
        ))
    return rows


def _summary_row(headline: str, helper: str) -> Gtk.Widget:
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
    row.set_margin_top(4); row.set_margin_bottom(4)

    check = Gtk.Image.new_from_icon_name(
        "object-select-symbolic", Gtk.IconSize.MENU,
    )
    check.set_valign(Gtk.Align.START)
    check.set_margin_top(2)
    row.pack_start(check, False, False, 0)

    text_col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
    head = Gtk.Label(label=headline)
    head.set_xalign(0); head.set_line_wrap(True)
    text_col.pack_start(head, False, False, 0)
    helper_lbl = Gtk.Label(label=helper)
    helper_lbl.set_xalign(0); helper_lbl.set_line_wrap(True)
    helper_lbl.get_style_context().add_class("dim-label")
    helper_lbl.get_style_context().add_class("mackes-page-subtitle")
    text_col.pack_start(helper_lbl, False, False, 0)
    row.pack_start(text_col, True, True, 0)

    return row
