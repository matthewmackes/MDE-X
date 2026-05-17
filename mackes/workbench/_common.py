"""Shared widget helpers used across workbench panels.

v1.4.0: rewritten to emit Carbon-refresh widgets (the pattern established
by `[[project_v1_1_0_design]]` and used verbatim in mesh_ssh.py /
mesh_services.py / remote_desktop.py / snapshots.py). Legacy panels
that import these helpers automatically pick up the Carbon styling
without per-panel rewrites.

Helpers map as follows:

  panel_box()       → vertically-stacking content frame with Carbon
                      page padding (32 / 40) — the standard `outer` box
  title_label(t)    → `.mackes-page-title` Label
  info_label(t)     → `.mackes-page-subtitle` Label (wraps, dim)
  section_header(t) → `.mackes-section-title` Label inside a top-margin
                      row (matches the section divider on every other
                      Carbon panel)
  labeled_row(...)  → `.form-row` style — label + helper stacked on
                      the left, control on the right
  error_label(t)    → `.mackes-notif.error` Tile

The original v1.0 class names are preserved on the same widgets so any
CSS rule that previously targeted `mackes-panel-title` /
`mackes-section-header` / `mackes-info` keeps applying — no visual
regressions on the panels that already use these helpers.
"""
from __future__ import annotations

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402


LABEL_COL_WIDTH = 180


def section_header(title: str) -> Gtk.Widget:
    """Carbon section divider — uppercase title + accent-meta margin.

    Matches the `_section_title` helper used in the v1.1.x panels.
    """
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    row.set_margin_top(28); row.set_margin_bottom(8)
    lbl = Gtk.Label(label=title)
    lbl.set_xalign(0)
    ctx = lbl.get_style_context()
    ctx.add_class("mackes-section-title")
    # Keep v1.0 classes for backward CSS compat
    ctx.add_class("mackes-section-header")
    row.pack_start(lbl, True, True, 0)
    return row


def labeled_row(label_text: str, widget: Gtk.Widget) -> Gtk.Widget:
    """Two-column row — label left, control right. Carbon `.form-row` pattern."""
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
    row.set_margin_top(4); row.set_margin_bottom(4)
    lbl = Gtk.Label(label=label_text)
    lbl.set_xalign(0)
    lbl.set_size_request(LABEL_COL_WIDTH, -1)
    ctx = lbl.get_style_context()
    ctx.add_class("form-label")
    # Keep v1.0 class for back-compat
    ctx.add_class("mackes-row-label")
    row.pack_start(lbl, False, False, 0)
    widget.set_halign(Gtk.Align.END)
    row.pack_start(widget, True, True, 0)
    return row


def panel_box(margin: int = 0) -> Gtk.Box:
    """Top-level page container with Carbon page padding (32/40).

    The `margin` argument is ignored — Carbon panels use fixed 32px top /
    40px sides as locked in `[[project_v1_1_0_design]]`. The argument is
    kept in the signature to avoid breaking callers that pass it.
    """
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
    box.set_margin_top(32); box.set_margin_bottom(32)
    box.set_margin_start(40); box.set_margin_end(40)
    return box


def info_label(text: str) -> Gtk.Widget:
    """Carbon page-subtitle — the dim, wrapped descriptive paragraph
    that sits under the page title."""
    lbl = Gtk.Label(label=text)
    lbl.set_xalign(0)
    lbl.set_line_wrap(True)
    ctx = lbl.get_style_context()
    ctx.add_class("mackes-page-subtitle")
    # v1.0 back-compat classes
    ctx.add_class("dim-label")
    ctx.add_class("mackes-info")
    return lbl


def error_label(text: str) -> Gtk.Widget:
    """Carbon error notification — used by panels that can't initialize."""
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
    box.get_style_context().add_class("mackes-notif")
    box.get_style_context().add_class("error")
    lbl = Gtk.Label(label=text)
    lbl.set_xalign(0); lbl.set_line_wrap(True)
    lbl.get_style_context().add_class("mackes-notif-body")
    # v1.0 back-compat class
    lbl.get_style_context().add_class("error")
    box.pack_start(lbl, False, False, 0)
    return box


def title_label(text: str) -> Gtk.Widget:
    """Carbon page title — the big top heading on a panel."""
    lbl = Gtk.Label(label=text)
    lbl.set_xalign(0)
    ctx = lbl.get_style_context()
    ctx.add_class("mackes-page-title")
    # v1.0 back-compat
    ctx.add_class("title-2")
    ctx.add_class("mackes-panel-title")
    return lbl
