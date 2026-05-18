"""Network → Mesh Join — Carbon panel hosting the one-button onboarding wizard.

A thin Carbon panel that wraps `mackes.wizard.pages.mesh_join.MeshJoinPage`
in the canonical breadcrumb + page_title + page_subtitle layout used by
every other Network panel. The hosted page does the real work — this
file exists so the sidebar nav has a stable entry point and the panel
mirrors the v1.1.x Carbon refresh pattern.
"""
from __future__ import annotations

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.wizard.pages.mesh_join import MeshJoinPage


def _page_title(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0)
    lab.get_style_context().add_class("mackes-page-title")
    return lab


def _page_subtitle(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0); lab.set_line_wrap(True)
    lab.get_style_context().add_class("mackes-page-subtitle")
    return lab


def _breadcrumb(parts: list[str]) -> Gtk.Widget:
    bc = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
    bc.get_style_context().add_class("mackes-breadcrumb")
    for i, p in enumerate(parts):
        lab = Gtk.Label(label=p)
        lab.set_xalign(0)
        bc.pack_start(lab, False, False, 0)
        if i != len(parts) - 1:
            sep = Gtk.Label(label="/")
            sep.set_xalign(0)
            sep.get_style_context().add_class("mackes-dot")
            bc.pack_start(sep, False, False, 0)
    return bc


class MeshJoinPanel(Gtk.Box):
    """Network → Mesh Join. Wraps the wizard page in a Carbon shell."""

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(12); outer.set_margin_bottom(0)
        outer.set_margin_start(16); outer.set_margin_end(16)

        outer.pack_start(_breadcrumb(["Mackes Shell", "Network", "Get Online"]),
                         False, False, 0)
        # The wizard page renders its own page-title + subtitle. We add a
        # small contextual subtitle ABOVE its body to call out that this is
        # the same flow you can run inside the first-run wizard.
        outer.pack_start(_page_subtitle(
            "The one-button onboarding flow. Same UX as the first-run "
            "wizard's mesh step — run it any time to bring this peer back "
            "online or to re-verify your mesh state."
        ), False, False, 0)

        self._page = MeshJoinPage(ctx=None)
        outer.pack_start(self._page, True, True, 0)

        # Whole-panel scroller so long log + QR content stays usable on
        # smaller monitors.
        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(outer)
        self.pack_start(scroller, True, True, 0)


__all__ = ["MeshJoinPanel"]
