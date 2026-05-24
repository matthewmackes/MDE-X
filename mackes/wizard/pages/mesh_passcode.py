"""Wizard screen — Mesh Passcode (Phase 12.8.4).

First-launch step that captures the 16-character mesh passcode every
peer needs to enroll. The page offers two flows:

  * Generate — call ``mackesd generate-passcode``, display the new
    code with a copy-to-clipboard button, and instruct the operator
    to store it in libsecret (auto-run when the operator is
    comfortable with the shell-out).
  * Paste — accept an existing passcode (16 URL-safe chars,
    validated against ``passcode::looks_valid`` via the bridge).

The page is non-blocking: if mackesd isn't installed yet, the wizard
shows a friendly skip-link and moves on.
"""
from __future__ import annotations

import logging
import shutil
import subprocess

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.workbench._common import a11y

logger = logging.getLogger(__name__)


_VALID_PASSCODE_LEN = 16
_VALID_PASSCODE_CHARS = set(
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    "abcdefghijklmnopqrstuvwxyz"
    "0123456789-_"
)


def passcode_is_valid(passcode: str) -> bool:
    """Pure helper: matches the Rust ``passcode::looks_valid`` shape
    (16 chars, URL-safe alphabet). Lifted out of the page so it's
    unit-testable without a GTK display."""
    if len(passcode) != _VALID_PASSCODE_LEN:
        return False
    return all(c in _VALID_PASSCODE_CHARS for c in passcode)


# ─────────────────────────────────────────────────────────────────
# NF-7.2 + NF-14.2 (v2.5) — Nebula join-token format.
# ─────────────────────────────────────────────────────────────────
#
# Wire shape: `mesh:<mesh_id>@<lighthouse_ip>:<port>#<bearer>`
#
#   mesh_id        ≥ 1 char, URL-safe (no '@' / ':' / '#' / '/')
#   lighthouse_ip  IPv4 dotted-quad (overlay or public; the wizard
#                  rejects empty)
#   port           1..=65535 integer
#   bearer         base32-encoded 64-byte enrollment token (~104
#                  chars including padding); URL-safe charset.
#
# Compact (target ≤ 120 chars), copy-pasteable, QR-code-friendly
# for the kiosk wizard. Locked per the design doc's
# `docs/design/v2.5-nebula-fabric.md` NF-7.2 section.

import re as _re
from dataclasses import dataclass as _dc


# Locked target so the join token fits inside a standard 1KB QR
# without splitting.
JOIN_TOKEN_MAX_LEN = 120

# Single regex for fast validation. Each named group surfaces a
# parsed component so the apply step doesn't need a parallel
# parser.
_JOIN_TOKEN_RE = _re.compile(
    r"^mesh:"
    r"(?P<mesh_id>[A-Za-z0-9._-]+)"
    r"@"
    r"(?P<lighthouse>[0-9.]+)"
    r":"
    r"(?P<port>[0-9]+)"
    r"#"
    r"(?P<bearer>[A-Za-z0-9_=-]+)"
    r"$"
)


@_dc(frozen=True)
class JoinToken:
    """One parsed join token. Returned by ``parse_join_token``."""

    mesh_id: str
    lighthouse: str
    port: int
    bearer: str

    def encode(self) -> str:
        """Round-trip back to the wire form."""
        return f"mesh:{self.mesh_id}@{self.lighthouse}:{self.port}#{self.bearer}"


def parse_join_token(raw: str) -> JoinToken | None:
    """Pure parser. Returns a :class:`JoinToken` on success, or
    ``None`` when the input doesn't match the locked shape. Caller
    is the wizard's apply step + the ``mackesd enroll`` CLI.
    """
    if not raw or len(raw) > JOIN_TOKEN_MAX_LEN:
        return None
    m = _JOIN_TOKEN_RE.match(raw)
    if not m:
        return None
    try:
        port = int(m.group("port"))
    except ValueError:
        return None
    if not 1 <= port <= 65535:
        return None
    # IPv4 dotted-quad sanity. Reject anything that doesn't parse
    # via the std-library socket helpers (covers IPv6 + hostnames;
    # the locked shape is IPv4 overlay/public only).
    import socket as _sk
    try:
        _sk.inet_pton(_sk.AF_INET, m.group("lighthouse"))
    except OSError:
        return None
    return JoinToken(
        mesh_id=m.group("mesh_id"),
        lighthouse=m.group("lighthouse"),
        port=port,
        bearer=m.group("bearer"),
    )


def join_token_is_valid(raw: str) -> bool:
    """Pure predicate. True when ``raw`` parses cleanly.
    Wizard validator + CLI sanity-check both call this.
    """
    return parse_join_token(raw) is not None


def _mackesd_available() -> bool:
    return shutil.which("mackesd") is not None


def _generate_passcode_via_mackesd() -> str | None:
    """Shell out to ``mackesd generate-passcode``. Returns the
    passcode string on success, ``None`` on any failure (binary
    missing, non-zero exit, garbled stdout)."""
    if not _mackesd_available():
        return None
    try:
        result = subprocess.run(
            ["mackesd", "generate-passcode"],
            check=True, capture_output=True, text=True, timeout=5,
        )
    except (subprocess.SubprocessError, OSError) as exc:
        logger.warning("mackesd generate-passcode failed: %s", exc)
        return None
    out = result.stdout.strip()
    return out if passcode_is_valid(out) else None


def build(ctx) -> Gtk.Widget:
    """Construct the wizard page widget."""
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=14)
    box.set_margin_top(24); box.set_margin_bottom(24)
    box.set_margin_start(48); box.set_margin_end(48)

    title = Gtk.Label(label="Mesh passcode")
    title.set_xalign(0)
    title.get_style_context().add_class("mackes-page-title")
    box.pack_start(title, False, False, 0)

    subtitle = Gtk.Label(label=(
        "Every Mackes peer in your fleet shares one 16-character "
        "passcode. Use Generate for the first peer, then paste the "
        "same code on every subsequent peer."
    ))
    subtitle.set_xalign(0); subtitle.set_line_wrap(True)
    subtitle.get_style_context().add_class("mackes-page-subtitle")
    box.pack_start(subtitle, False, False, 0)

    if not _mackesd_available():
        warn = Gtk.Label(label=(
            "mackesd isn't installed yet. The mesh control plane "
            "ships with the mackes-shell package; skip this step and "
            "configure the passcode from Workbench → Network → Mesh "
            "Control once mackesd is running."
        ))
        warn.set_xalign(0); warn.set_line_wrap(True)
        warn.get_style_context().add_class("mackes-warning-banner")
        box.pack_start(warn, False, False, 12)
        return box

    # Generate row.
    gen_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
    gen_btn = Gtk.Button(label="Generate")
    a11y(gen_btn, "Generate a new 16-character mesh passcode", tooltip=None)
    gen_row.pack_start(gen_btn, False, False, 0)
    gen_entry = Gtk.Entry()
    gen_entry.set_editable(False)
    gen_entry.set_width_chars(_VALID_PASSCODE_LEN + 4)
    a11y(gen_entry, "Generated passcode (read-only)", tooltip=None)
    gen_row.pack_start(gen_entry, False, False, 0)
    copy_btn = Gtk.Button(label="Copy")
    a11y(copy_btn, "Copy passcode to clipboard", tooltip=None)
    gen_row.pack_start(copy_btn, False, False, 0)
    box.pack_start(gen_row, False, False, 8)

    def _on_generate(_btn):
        code = _generate_passcode_via_mackesd()
        if code:
            gen_entry.set_text(code)
            setattr(ctx, "mesh_passcode", code)
    gen_btn.connect("clicked", _on_generate)

    def _on_copy(_btn):
        clipboard = Gtk.Clipboard.get(Gtk.gdk.Atom.intern("CLIPBOARD", False))  # noqa: SLF001
        clipboard.set_text(gen_entry.get_text(), -1)
    try:
        copy_btn.connect("clicked", _on_copy)
    except Exception:  # noqa: BLE001 — Gtk.gdk vs Gdk path differences across versions
        copy_btn.set_sensitive(False)

    # Paste-existing row.
    paste_label = Gtk.Label(label="Or paste an existing passcode:")
    paste_label.set_xalign(0)
    box.pack_start(paste_label, False, False, 8)

    paste_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
    paste_entry = Gtk.Entry()
    paste_entry.set_width_chars(_VALID_PASSCODE_LEN + 4)
    paste_entry.set_placeholder_text("16 URL-safe chars")
    a11y(paste_entry, "Paste an existing mesh passcode", tooltip=None)
    paste_row.pack_start(paste_entry, False, False, 0)
    validate_btn = Gtk.Button(label="Use this passcode")
    a11y(validate_btn, "Validate and store the pasted passcode", tooltip=None)
    paste_row.pack_start(validate_btn, False, False, 0)
    paste_status = Gtk.Label(label="")
    paste_status.set_xalign(0)
    paste_row.pack_start(paste_status, True, True, 0)
    box.pack_start(paste_row, False, False, 0)

    def _on_validate(_btn):
        candidate = paste_entry.get_text().strip()
        if passcode_is_valid(candidate):
            paste_status.set_text("✓ accepted")
            paste_status.get_style_context().add_class("mackes-pill-ok")
            setattr(ctx, "mesh_passcode", candidate)
        else:
            paste_status.set_text(
                "✗ must be exactly 16 URL-safe characters (A-Z, a-z, 0-9, -, _)"
            )
            paste_status.get_style_context().remove_class("mackes-pill-ok")
            paste_status.get_style_context().add_class("mackes-pill-fail")
    validate_btn.connect("clicked", _on_validate)

    return box
