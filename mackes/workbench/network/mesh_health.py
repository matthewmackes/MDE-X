"""Network → Mesh Health — unified per-layer status panel.

Reads `mackes.mesh.health()` and renders one row per mesh layer:
glyph + label + state pill + detail + hint. Header has:

  Re-check        re-run every probe ignoring cache
  Copy diagnostics dump health_json() to clipboard for support tickets
  Save report    write the diagnostic to ~/QNM-Drop/mesh-health-*.txt

Mirrors the canonical mesh_ssh.py Carbon layout (breadcrumb +
page_title + page_subtitle + section_title).
"""
from __future__ import annotations

import datetime
import threading
from pathlib import Path

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

from mackes.mesh import diagnose, health, health_json, overall_state, summary
from mackes.workbench._common import (
    info_label,
    panel_box,
    section_description,
    section_header,
    title_label,
)


# State → (display label, CSS class for the pill)
_PILL_STYLES = {
    "ok":      ("OK",      "mackes-pill-ok"),
    "warn":    ("WARN",    "mackes-pill-warn"),
    "fail":    ("FAIL",    "mackes-pill-fail"),
    "missing": ("OFF",     "mackes-pill-neutral"),
}

# Layer key → (Nerd Font glyph, human label)
_LAYER_LABELS = {
    "vpn":            ("",  "Tailscale VPN"),
    "ssh":            ("",  "Mesh SSH"),
    "services":       ("",  "Service discovery"),
    "fs":             ("",  "Peer file shares (sshfs)"),
    "sync":           ("",  "Bucket sync"),
    "notifications":  ("",  "Cross-peer notifications"),
    "browser":        ("",  "Thunar mesh views"),
    "thumbnailer":    ("",  "Mesh thumbnailer"),
}


def _breadcrumb(parts: list[str]) -> Gtk.Widget:
    bc = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
    bc.get_style_context().add_class("mackes-breadcrumb")
    for i, p in enumerate(parts):
        lab = Gtk.Label(label=p); lab.set_xalign(0)
        bc.pack_start(lab, False, False, 0)
        if i != len(parts) - 1:
            sep = Gtk.Label(label="/"); sep.set_xalign(0)
            sep.get_style_context().add_class("mackes-dot")
            bc.pack_start(sep, False, False, 0)
    return bc


def _page_subtitle(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0); lab.set_line_wrap(True)
    lab.get_style_context().add_class("mackes-page-subtitle")
    return lab


class MeshHealthPanel(Gtk.Box):
    """Network → Mesh Health full-page panel."""

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)

        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(12); outer.set_margin_bottom(12)
        outer.set_margin_start(16); outer.set_margin_end(16)

        outer.pack_start(_breadcrumb(["Mackes Shell", "Network", "Mesh Health"]),
                         False, False, 0)

        title = Gtk.Label(label="Mesh Health")
        title.set_xalign(0); title.get_style_context().add_class("mackes-page-title")
        outer.pack_start(title, False, False, 0)
        outer.pack_start(_page_subtitle(
            "One page showing every part of the mesh and whether it's "
            "working. Green is good; yellow needs a look; red needs a "
            "fix. Click Re-check to re-run every probe right now."
        ), False, False, 0)

        # Overall status banner
        self._overall = Gtk.Label(label="(loading…)")
        self._overall.set_xalign(0)
        self._overall.get_style_context().add_class("mackes-section-title")
        outer.pack_start(self._overall, False, False, 0)

        self._summary = Gtk.Label(label="")
        self._summary.set_xalign(0)
        self._summary.get_style_context().add_class("mackes-page-subtitle")
        outer.pack_start(self._summary, False, False, 0)

        # Action bar
        bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        bar.set_margin_top(8); bar.set_margin_bottom(16)
        recheck = Gtk.Button(label="Re-check")
        recheck.get_style_context().add_class("suggested-action")
        recheck.connect("clicked", lambda *_: self._refresh(force=True))
        bar.pack_start(recheck, False, False, 0)
        copy = Gtk.Button(label="Copy diagnostics")
        copy.connect("clicked", lambda *_: self._copy_to_clipboard())
        bar.pack_start(copy, False, False, 0)
        save = Gtk.Button(label="Save report")
        save.connect("clicked", lambda *_: self._save_report())
        bar.pack_start(save, False, False, 0)
        outer.pack_start(bar, False, False, 0)

        # Per-layer rows go here
        outer.pack_start(section_header("By layer"), False, False, 0)
        outer.pack_start(section_description(
            "Each row is one part of the mesh. If something is wrong, "
            "the hint at the bottom of the row tells you what to do."
        ), False, False, 0)

        self._layer_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        outer.pack_start(self._layer_box, False, False, 0)

        # Diagnostic dump (collapsible)
        outer.pack_start(section_header("Raw diagnostics"), False, False, 0)
        outer.pack_start(info_label(
            "Same information formatted for copy-pasting into a support "
            "ticket or a paste-bin. Updated every time you click "
            "Re-check."
        ), False, False, 0)
        self._dump_view = Gtk.TextView()
        self._dump_view.set_editable(False)
        self._dump_view.set_monospace(True)
        self._dump_view.set_cursor_visible(False)
        self._dump_view.get_buffer().set_text("(running probes…)")
        scroll = Gtk.ScrolledWindow()
        scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroll.set_size_request(-1, 240)
        scroll.add(self._dump_view)
        outer.pack_start(scroll, False, False, 0)

        self.pack_start(outer, True, True, 0)

        # Auto-refresh on map; only tick while visible.
        self._refresh_id: int | None = None
        self.connect("map",   lambda *_: self._start_auto_refresh())
        self.connect("unmap", lambda *_: self._stop_auto_refresh())
        self._refresh(force=False)

    # ---- Auto-refresh while visible -------------------------------------

    def _start_auto_refresh(self) -> None:
        if self._refresh_id is None:
            # Re-render every 15s while visible — much cheaper than the
            # raw probes thanks to probe_cache (TTLs 5–300s).
            self._refresh_id = GLib.timeout_add_seconds(
                15, lambda: (self._refresh(force=False), True)[1])

    def _stop_auto_refresh(self) -> None:
        if self._refresh_id is not None:
            GLib.source_remove(self._refresh_id)
            self._refresh_id = None

    # ---- Render ----------------------------------------------------------

    def _refresh(self, *, force: bool) -> None:
        def worker():
            snap = health(force_refresh=force)
            dump_lines = diagnose() if force else None
            GLib.idle_add(self._apply, snap, dump_lines)
        threading.Thread(target=worker, daemon=True).start()

    def _apply(self, snap: dict, dump_lines: list[str] | None) -> bool:
        # Overall banner
        worst = overall_state(snap)
        worst_label = {
            "ok": "Mesh is healthy", "warn": "Mesh has warnings",
            "fail": "Mesh has failures", "missing": "Mesh is mostly off",
        }.get(worst, worst.title())
        self._overall.set_text(worst_label)
        self._summary.set_text(summary(snap))

        # Replace layer rows
        for c in self._layer_box.get_children():
            self._layer_box.remove(c)
        for layer_key, h in snap.items():
            self._layer_box.pack_start(self._build_row(layer_key, h),
                                       False, False, 0)
        self._layer_box.show_all()

        # Refresh raw diagnostics whenever we have new lines (force or
        # first paint). On non-force ticks we keep the existing text.
        if dump_lines is not None:
            self._dump_view.get_buffer().set_text("\n".join(dump_lines))
        elif self._dump_view.get_buffer().get_char_count() <= 30:
            # First-paint: even on a non-force refresh, populate the
            # dump from the snapshot we just rendered.
            self._dump_view.get_buffer().set_text("\n".join(diagnose()))
        return False

    def _build_row(self, layer_key: str, h) -> Gtk.Widget:
        glyph, title = _LAYER_LABELS.get(
            layer_key, ("", layer_key.title()))
        pill_text, pill_cls = _PILL_STYLES.get(
            h.state, (h.state.upper(), "mackes-pill-neutral"))

        row = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        row.get_style_context().add_class("mackes-data-row")
        row.set_margin_top(6); row.set_margin_bottom(6)

        head = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        glyph_lab = Gtk.Label(label=glyph)
        glyph_lab.get_style_context().add_class("mackes-dot")
        head.pack_start(glyph_lab, False, False, 0)
        title_lab = Gtk.Label(label=title)
        title_lab.set_xalign(0)
        title_lab.get_style_context().add_class("mackes-section-title")
        head.pack_start(title_lab, False, False, 0)
        pill = Gtk.Label(label=pill_text)
        pill.get_style_context().add_class("mackes-tag")
        pill.get_style_context().add_class(pill_cls)
        head.pack_end(pill, False, False, 0)
        if h.latency_ms is not None:
            t = Gtk.Label(label=f"{h.latency_ms:.0f} ms")
            t.get_style_context().add_class("mackes-section-meta")
            head.pack_end(t, False, False, 0)
        row.pack_start(head, False, False, 0)

        label_lab = Gtk.Label(label=h.label)
        label_lab.set_xalign(0); label_lab.set_line_wrap(True)
        row.pack_start(label_lab, False, False, 0)

        if h.detail:
            d = Gtk.Label(label=h.detail)
            d.set_xalign(0); d.set_line_wrap(True)
            d.get_style_context().add_class("mackes-section-meta")
            d.get_style_context().add_class("mackes-code")
            row.pack_start(d, False, False, 0)

        if h.hint and h.state != "ok":
            hint = Gtk.Label(label=f"→ {h.hint}")
            hint.set_xalign(0); hint.set_line_wrap(True)
            hint.get_style_context().add_class("mackes-page-subtitle")
            row.pack_start(hint, False, False, 0)

        return row

    # ---- Actions ---------------------------------------------------------

    def _copy_to_clipboard(self) -> None:
        from gi.repository import Gdk
        text = "\n".join(diagnose())
        try:
            clip = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
            clip.set_text(text, -1); clip.store()
        except Exception:  # noqa: BLE001
            pass

    def _save_report(self) -> Path | None:
        try:
            drop = Path.home() / "QNM-Drop"
            drop.mkdir(parents=True, exist_ok=True)
            stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
            path = drop / f"mesh-health-{stamp}.txt"
            path.write_text(
                "\n".join(diagnose()) + "\n\n--- JSON ---\n" + health_json(),
                encoding="utf-8",
            )
            return path
        except OSError:
            return None


__all__ = ["MeshHealthPanel"]
