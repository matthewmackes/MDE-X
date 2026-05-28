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
  empty_state(...)  → centered "No <thing>" headline + body + optional
                      CTA button. Phase 11.5 — used wherever a probe
                      returned no rows.
  error_state(...)  → centered "Couldn't load <thing>" + error reason +
                      retry button. Phase 11.5 — used wherever a probe
                      raised. Replaces every silent except-pass on a
                      panel-rendering path.

The original v1.0 class names are preserved on the same widgets so any
CSS rule that previously targeted `mackes-panel-title` /
`mackes-section-header` / `mackes-info` keeps applying — no visual
regressions on the panels that already use these helpers.
"""
from __future__ import annotations

from typing import Callable, Optional

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402


LABEL_COL_WIDTH = 180


def section_header(title: str) -> Gtk.Widget:
    """Carbon section divider — uppercase title + accent-meta margin.

    Matches the `_section_title` helper used in the v1.1.x panels.
    """
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    row.set_margin_top(12); row.set_margin_bottom(4)
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


def panel_box(margin: int = 12) -> Gtk.Box:
    """Top-level page container with the compact Mackes panel margins.

    `margin` controls top/bottom; left/right use ⌈margin·4/3⌉ so the
    side gutters stay wider than the vertical breathing room (the
    visual hierarchy every other Mackes panel uses). Pass `margin=0`
    for embedded sub-panels that have their own outer container.
    """
    side = (margin * 4 + 2) // 3
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
    box.set_margin_top(margin); box.set_margin_bottom(margin)
    box.set_margin_start(side); box.set_margin_end(side)
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


def section_description(text: str) -> Gtk.Widget:
    """Plain-language explainer that sits above a section's content.

    Mirrors `.mackes-section-description` in carbon-layout.css. Two
    sentences max, written at a 9th-grade reading level, describing
    what the section does in user terms — never a technical caption.
    """
    lbl = Gtk.Label(label=text)
    lbl.set_xalign(0)
    lbl.set_line_wrap(True)
    ctx = lbl.get_style_context()
    ctx.add_class("mackes-section-description")
    return lbl


def versioned_title(base: str) -> str:
    """Return `<base> — MDE <version>` — the canonical titlebar
    format every MDE window uses. Read by Gtk.Window.set_title()
    callers and the header bar's set_title().

    v2.0.0 Phase 0.11 — "Mackes" rebranded to "MDE" in titlebar text
    (titlebars are short surfaces; "Mackes Desktop Environment" only
    appears on first reference in About / README / docs).
    """
    try:
        from mackes import __version__
    except Exception:  # noqa: BLE001
        __version__ = "?"
    return f"{base} — MDE {__version__}"


def set_versioned_title(window: "Gtk.Window", base: str) -> None:
    """Convenience: set window's titlebar to `<base> — Mackes <version>`."""
    window.set_title(versioned_title(base))


def a11y(widget: Gtk.Widget, name: str, tooltip: str | None = None) -> Gtk.Widget:
    """Attach a tooltip + AT-SPI accessible name to an interactive widget.

    Phase 11.2 sweep helper. The accessible name is what screen readers
    speak when focus lands on the widget — it should describe the
    widget's *purpose* in a way that's useful out of context (i.e.
    "Save Wi-Fi password" not just "Save"). The tooltip is the hover
    hint; if omitted it defaults to `name` so every interactive widget
    has a discoverable label.

    Returns `widget` so calls can chain inline with `panel.pack_start()`.
    """
    widget.set_tooltip_text(tooltip if tooltip is not None else name)
    ax = widget.get_accessible()
    if ax is not None:
        ax.set_name(name)
    return widget


def close_on_escape(window: "Gtk.Window") -> None:
    """Connect a key-press handler that destroys `window` on Escape.

    Phase 11.2 sweep helper for `Gtk.Window` subclasses that need an
    explicit Escape binding. `Gtk.Dialog` already routes Escape through
    its action area (so `Cancel`-response buttons trigger), but bare
    `Gtk.Window` toplevels we open as modeless surfaces (the slide-in
    drawer, the headscale setup, the wizard) need the binding wired
    by hand. Idempotent: calling twice connects two handlers but the
    second is a no-op since the first already destroyed the window.
    """
    import gi as _gi
    _gi.require_version("Gdk", "3.0")
    from gi.repository import Gdk as _Gdk  # noqa: PLC0415

    def _on_key(_w, event):
        if event.keyval == _Gdk.KEY_Escape:
            window.destroy()
            return True
        return False

    window.connect("key-press-event", _on_key)


def empty_state(
    title: str,
    body: Optional[str] = None,
    *,
    icon_name: str = "dialog-information-symbolic",
    cta_label: Optional[str] = None,
    on_cta: Optional[Callable[[], None]] = None,
) -> Gtk.Widget:
    """Centered empty-state tile — Phase 11.5 lock.

    Used wherever a probe legitimately returned zero rows (no peers
    joined, no snapshots taken, no autostart entries). Renders:

      <icon>
      <bold headline — `title`>
      <body paragraph — optional explanation / CTA hint>
      [optional CTA button]

    Mirrors the Carbon design system's empty-state pattern. Spacing
    + sizing chosen so a `Gtk.Box` slot of any reasonable height (≥120 px)
    looks deliberately empty rather than broken.
    """
    wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
    wrap.set_halign(Gtk.Align.CENTER)
    wrap.set_valign(Gtk.Align.CENTER)
    wrap.set_hexpand(True); wrap.set_vexpand(True)
    wrap.set_margin_top(24); wrap.set_margin_bottom(24)
    wrap.set_margin_start(16); wrap.set_margin_end(16)
    wrap.get_style_context().add_class("mackes-empty-state")

    try:
        icon = Gtk.Image.new_from_icon_name(icon_name, Gtk.IconSize.DIALOG)
        icon.set_halign(Gtk.Align.CENTER)
        icon.get_style_context().add_class("mackes-empty-state-icon")
        wrap.pack_start(icon, False, False, 0)
    except Exception:  # noqa: BLE001
        # Icon lookup failures should never break the empty state itself;
        # the headline + body still convey the message.
        pass

    head = Gtk.Label(label=title)
    head.set_xalign(0.5); head.set_line_wrap(True)
    head.get_style_context().add_class("mackes-empty-state-title")
    # Reuse the section-title class for typography parity until the
    # 11.5 CSS pass lands.
    head.get_style_context().add_class("mackes-section-title")
    wrap.pack_start(head, False, False, 0)

    if body:
        body_lbl = Gtk.Label(label=body)
        body_lbl.set_xalign(0.5); body_lbl.set_line_wrap(True)
        body_lbl.set_max_width_chars(64)
        body_lbl.get_style_context().add_class("mackes-empty-state-body")
        body_lbl.get_style_context().add_class("mackes-page-subtitle")
        body_lbl.get_style_context().add_class("dim-label")
        wrap.pack_start(body_lbl, False, False, 0)

    if cta_label and on_cta is not None:
        btn_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        btn_row.set_halign(Gtk.Align.CENTER)
        btn_row.set_margin_top(8)
        btn = Gtk.Button(label=cta_label)
        btn.get_style_context().add_class("mackes-empty-state-cta")
        btn.get_style_context().add_class("suggested-action")
        btn.connect("clicked", lambda *_: on_cta())
        a11y(btn, name=cta_label)
        btn_row.pack_start(btn, False, False, 0)
        wrap.pack_start(btn_row, False, False, 0)

    return wrap


def error_state(
    title: str,
    reason: Optional[str] = None,
    *,
    icon_name: str = "dialog-error-symbolic",
    retry_label: Optional[str] = "Retry",
    on_retry: Optional[Callable[[], None]] = None,
) -> Gtk.Widget:
    """Centered error tile — Phase 11.5 lock.

    Replaces every silent `except: pass` on a panel-rendering path.
    Renders:

      <error icon>
      <bold headline — e.g. "Couldn't load snapshots">
      <reason paragraph — the exception message in plain language>
      [Retry button — when on_retry is provided]

    The retry button is the actionable next step: if the probe is
    repeatable (firewall-cmd, nmcli, tailscale status) the user can
    click again after fixing the underlying issue (started the daemon,
    plugged in the cable, etc.) without leaving the panel.

    When `on_retry` is None the button is omitted — useful for terminal
    errors like "xfconf bridge not available" where re-running won't
    help.
    """
    wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
    wrap.set_halign(Gtk.Align.CENTER)
    wrap.set_valign(Gtk.Align.CENTER)
    wrap.set_hexpand(True); wrap.set_vexpand(True)
    wrap.set_margin_top(24); wrap.set_margin_bottom(24)
    wrap.set_margin_start(16); wrap.set_margin_end(16)
    wrap.get_style_context().add_class("mackes-empty-state")
    wrap.get_style_context().add_class("error")

    try:
        icon = Gtk.Image.new_from_icon_name(icon_name, Gtk.IconSize.DIALOG)
        icon.set_halign(Gtk.Align.CENTER)
        icon.get_style_context().add_class("mackes-empty-state-icon")
        icon.get_style_context().add_class("error")
        wrap.pack_start(icon, False, False, 0)
    except Exception:  # noqa: BLE001
        pass

    head = Gtk.Label(label=title)
    head.set_xalign(0.5); head.set_line_wrap(True)
    head.get_style_context().add_class("mackes-empty-state-title")
    head.get_style_context().add_class("mackes-section-title")
    head.get_style_context().add_class("error")
    wrap.pack_start(head, False, False, 0)

    if reason:
        # Carbon code style for the reason — it's an error message,
        # often containing a path or exit code that needs to read
        # verbatim.
        reason_lbl = Gtk.Label(label=reason)
        reason_lbl.set_xalign(0.5); reason_lbl.set_line_wrap(True)
        reason_lbl.set_max_width_chars(72)
        reason_lbl.set_selectable(True)
        reason_lbl.get_style_context().add_class("mackes-empty-state-body")
        reason_lbl.get_style_context().add_class("mackes-page-subtitle")
        reason_lbl.get_style_context().add_class("dim-label")
        wrap.pack_start(reason_lbl, False, False, 0)

    if retry_label and on_retry is not None:
        btn_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        btn_row.set_halign(Gtk.Align.CENTER)
        btn_row.set_margin_top(8)
        btn = Gtk.Button(label=retry_label)
        btn.get_style_context().add_class("mackes-empty-state-cta")
        btn.connect("clicked", lambda *_: on_retry())
        a11y(btn, name=retry_label)
        btn_row.pack_start(btn, False, False, 0)
        wrap.pack_start(btn_row, False, False, 0)

    return wrap


def format_probe_error(exc: BaseException) -> str:
    """Render an exception to a user-readable one-liner.

    Drops the leading `ModuleName.SubclassName: ` prefix from common
    exception types so the user sees "firewall-cmd timed out after 2s"
    rather than "subprocess.TimeoutExpired: firewall-cmd timed out
    after 2s". For unfamiliar exception types we keep the class name
    so the message remains debuggable.
    """
    import subprocess as _sp

    msg = str(exc).strip()
    if not msg:
        msg = exc.__class__.__name__
    # Friendly classes: drop the exception type entirely.
    friendly = (OSError, FileNotFoundError, PermissionError,
                _sp.CalledProcessError, _sp.TimeoutExpired,
                ValueError, KeyError, RuntimeError)
    if isinstance(exc, friendly):
        return msg
    return f"{exc.__class__.__name__}: {msg}"
