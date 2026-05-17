"""mackes.tui — Textual TUI (v1.4.0).

Headless entry point for every Mackes command that doesn't require XFCE.
Launched automatically when:
  - no $DISPLAY / $WAYLAND_DISPLAY is set, AND
  - no subcommand was given on the CLI, AND
  - textual is importable.

Otherwise the existing argparse CLI in mackes.headless.cli still handles
subcommands verbatim. The TUI is additive — it never breaks scripting.
"""
from __future__ import annotations

__all__ = ["available", "run"]


def available() -> bool:
    """True iff textual is importable in this Python."""
    try:
        import textual  # noqa: F401
        return True
    except Exception:  # noqa: BLE001
        return False


def run() -> int:
    """Boot the Textual app. Returns the app's exit code."""
    from mackes.tui.app import MackesTUI
    return MackesTUI().run() or 0
