"""Help-doc discovery + plain-text rendering.

Extracted from `mackes/workbench/help.py` as part of
`EPIC-RETIRE-PY-WORKBENCH.delete-ported.batch-3` (2026-05-26).
The workbench's GTK HelpPanel retires under the EPIC, but the
pure-text discovery + render helpers stay alive — they're consumed
outside the workbench tree (`mackes/headless/cli.py` for the
`mackes help` subcommand; `mackes/tui/screens/help_screen.py` for
the TUI's help screen).

What did NOT migrate from the original help.py:
  - The Pango/GTK markdown renderer (`_render_markdown`, `_inline`,
    `_escape`, `_HEADER_RE` and its sibling regexes, `HelpPanel`
    class). Those are GTK-only — they retire with the GTK chrome.

`render_topic_plain` strips YAML frontmatter from the raw markdown +
returns the body verbatim; it does NOT apply markdown→ANSI styling.
Callers that want styled CLI output pre/post-process the returned
text themselves.
"""
from __future__ import annotations

from pathlib import Path
from typing import Optional


# ---------------------------------------------------------------------------
# Topic discovery + ordering
# ---------------------------------------------------------------------------

# Help docs ship under /usr/share/mackes-shell/help in the RPM; in
# a dev tree they live at <repo>/docs/help. Try both, first one
# wins.
_HELP_ROOTS = (
    Path("/usr/share/mackes-shell/help"),
    Path(__file__).resolve().parent.parent / "docs" / "help",
)


# Topic ordering — matches the structure in docs/help/index.md so the
# sidebar reads top-down in the same logical order. Topics not in this
# list appear after, alphabetized.
_TOPIC_ORDER = [
    "index",
    "getting-started",
    "dashboard",
    "look-and-feel",
    "devices",
    "network",
    "system",
    "apps",
    "maintain",
    "mesh",
    "mesh-vpn",
    "mesh-thunar",
    "mesh-ssh",
    "mesh-services",
    "headless",
    "wayland",
    "presets",
    "keybindings",
    "cli-reference",
    "troubleshooting",
]


# Human-readable labels for the sidebar.
_TOPIC_LABELS = {
    "index":           "Welcome",
    "getting-started": "Getting Started",
    "dashboard":       "Dashboard",
    "look-and-feel":   "Look & Feel",
    "devices":         "Devices",
    "network":         "Network",
    "system":          "System",
    "apps":            "Apps",
    "maintain":        "Maintain",
    "mesh":            "Mesh — Overview",
    "mesh-vpn":        "Mesh VPN",
    "mesh-thunar":     "Mesh in Thunar",
    "mesh-ssh":        "Mesh SSH",
    "mesh-services":   "Mesh Services",
    "headless":        "Headless Node Mode",
    "wayland":         "Wayland support",
    "presets":         "Presets",
    "keybindings":     "Keyboard shortcuts",
    "cli-reference":   "CLI reference",
    "troubleshooting": "Troubleshooting",
}


def _help_root() -> Optional[Path]:
    for r in _HELP_ROOTS:
        if r.is_dir():
            return r
    return None


def _discover_topics() -> list[tuple[str, str, Path]]:
    """Return [(topic_id, label, path)] sorted per _TOPIC_ORDER then alpha."""
    root = _help_root()
    if root is None:
        return []
    found: dict[str, Path] = {}
    for p in root.glob("*.md"):
        found[p.stem] = p
    ordered: list[tuple[str, str, Path]] = []
    for tid in _TOPIC_ORDER:
        if tid in found:
            ordered.append((tid, _TOPIC_LABELS.get(tid, tid), found.pop(tid)))
    # Trailing alphabetized stragglers.
    for tid, path in sorted(found.items()):
        ordered.append((tid, _TOPIC_LABELS.get(tid, tid), path))
    return ordered


# ---------------------------------------------------------------------------
# Plain-text renderer (for `mackes help` CLI / TUI help screen)
# ---------------------------------------------------------------------------


def render_topic_plain(topic_id: str) -> str:
    """Render a topic as plain text. Strips YAML frontmatter; returns
    the raw markdown body verbatim — callers apply their own styling.
    """
    root = _help_root()
    if root is None:
        return f"(help docs not found in any of: {[str(r) for r in _HELP_ROOTS]})"
    path = root / f"{topic_id}.md"
    if not path.exists():
        return f"(no such help topic: {topic_id!r}; try `mackes help` for the list)"
    text = path.read_text(encoding="utf-8")
    # Strip YAML frontmatter.
    if text.startswith("---\n"):
        end = text.find("\n---\n", 4)
        if end != -1:
            text = text[end + 5:]
    return text


def list_topics_plain() -> str:
    """Return a plain-text list of available topics. Used by `mackes help`."""
    topics = _discover_topics()
    if not topics:
        return "(no help topics found)"
    width = max(len(tid) for tid, _, _ in topics) + 2
    lines = ["Available help topics:", ""]
    for tid, label, _ in topics:
        lines.append(f"  {tid:<{width}} {label}")
    lines.extend(["", "Usage:  mackes help <topic>"])
    return "\n".join(lines)
