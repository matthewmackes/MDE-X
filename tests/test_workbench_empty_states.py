"""Phase 11.5 — `_common` helper coverage (empty_state / error_state /
format_probe_error).

The Bucket-A panel-level tests that used to live here
(``test_apps_installed_panel_renders_error_on_rpm_failure``, the three
``test_vpn_panel_*`` tests) were retired alongside
``mackes/workbench/apps/installed.py`` + ``mackes/workbench/network/vpn.py``
in ``EPIC-RETIRE-PY-WORKBENCH.delete-ported.batch-4`` (2026-05-26). The
Iced ``apps_installed.rs`` + ``vpn.rs`` panels carry equivalent failure-path
coverage Rust-side.

What survives here: the 5 tests that exercise the
``mackes.gtk_common`` helpers (empty_state / error_state /
format_probe_error). The ``_common`` module retires later under
``.delete-chrome`` (it's infra for the GTK panels); at that point this
test file retires with it.

Skipped when GTK or ``$DISPLAY`` (or Xvfb) is unavailable — same gate
the panel-level tests used.
"""
from __future__ import annotations

import os

import pytest


# Skip the whole module if GTK isn't importable or we have no display.
gi = pytest.importorskip("gi")
gi.require_version("Gtk", "3.0")
try:
    from gi.repository import Gtk  # noqa: E402
except (ImportError, ValueError) as exc:
    pytest.skip(f"GTK3 typelib unavailable: {exc}", allow_module_level=True)

if not (os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")):
    pytest.skip("no $DISPLAY (run under Xvfb)", allow_module_level=True)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _walk(widget: Gtk.Widget):
    """Depth-first walk over a widget tree."""
    yield widget
    if isinstance(widget, Gtk.Container):
        for child in widget.get_children():
            yield from _walk(child)


def _find_label_with(widget: Gtk.Widget, needle: str) -> Gtk.Label | None:
    """Return the first Gtk.Label in the tree containing ``needle`` in
    its visible text. Case-insensitive."""
    needle_low = needle.lower()
    for w in _walk(widget):
        if isinstance(w, Gtk.Label):
            text = (w.get_text() or "")
            if needle_low in text.lower():
                return w
    return None


def _has_class(widget: Gtk.Widget, klass: str) -> bool:
    """Recursive: is any descendant carrying the given CSS class?"""
    for w in _walk(widget):
        ctx = w.get_style_context()
        if ctx is not None and ctx.has_class(klass):
            return True
    return False


# ---------------------------------------------------------------------------
# Helper-level tests — the public empty_state / error_state primitives
# ---------------------------------------------------------------------------


def test_empty_state_renders_title_body_and_cta():
    from mackes.gtk_common import empty_state

    clicks: list[bool] = []
    widget = empty_state(
        "No snapshots yet",
        "Take one before you change something risky.",
        cta_label="Create restore point",
        on_cta=lambda: clicks.append(True),
    )

    assert _find_label_with(widget, "No snapshots yet") is not None, (
        "empty_state() must render its title as a visible Gtk.Label"
    )
    assert _find_label_with(widget, "before you change") is not None, (
        "empty_state() must render the body paragraph"
    )

    # Find the CTA button and click it.
    btn = next(
        (w for w in _walk(widget) if isinstance(w, Gtk.Button)),
        None,
    )
    assert btn is not None, "empty_state() with cta_label must produce a button"
    assert btn.get_label() == "Create restore point"
    btn.clicked()
    assert clicks == [True], "Clicking the CTA must invoke on_cta"


def test_error_state_renders_reason_and_retry():
    from mackes.gtk_common import error_state

    retries: list[bool] = []
    widget = error_state(
        "Couldn't load snapshots",
        "Permission denied: /var/lib/mackes",
        on_retry=lambda: retries.append(True),
    )

    assert _find_label_with(widget, "Couldn't load snapshots") is not None
    assert _find_label_with(widget, "Permission denied") is not None
    assert _has_class(widget, "error"), (
        "error_state() must carry the `error` style class for theming"
    )

    btn = next(
        (w for w in _walk(widget) if isinstance(w, Gtk.Button)),
        None,
    )
    assert btn is not None and btn.get_label() == "Retry"
    btn.clicked()
    assert retries == [True], "Retry button must invoke on_retry"


def test_error_state_omits_button_when_unactionable():
    """A non-recoverable error (e.g. firewall-cmd missing) should still
    render a label tile, just without a Retry button."""
    from mackes.gtk_common import error_state

    widget = error_state(
        "firewalld not available",
        "Install firewalld and reopen this panel.",
        retry_label=None,
    )
    assert _find_label_with(widget, "firewalld not available") is not None
    buttons = [w for w in _walk(widget) if isinstance(w, Gtk.Button)]
    assert buttons == [], (
        "error_state(retry_label=None) must not render a button"
    )


def test_format_probe_error_drops_class_prefix_on_oserror():
    from mackes.gtk_common import format_probe_error

    msg = format_probe_error(FileNotFoundError("rpm: not found"))
    assert "rpm: not found" in msg
    assert "FileNotFoundError" not in msg


def test_format_probe_error_keeps_class_for_unknown_exception():
    from mackes.gtk_common import format_probe_error

    class _Weird(Exception):
        pass

    msg = format_probe_error(_Weird("nope"))
    assert "_Weird" in msg and "nope" in msg
