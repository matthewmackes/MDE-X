"""Live topology renderer (Phase 12.9.1 + 12.9.2 + 12.9.4 + 12.9.5).

Cairo-backed Gtk.DrawingArea that paints the mesh fabric as a
force-directed graph. Refreshed every 5 s via a GLib timeout. Nodes
are colored by their health state (12.9.2). Click a node or edge for
the side-panel detail surface (12.9.4). The segmented control at the
top toggles Global view (whole mesh) vs. Node view (one peer + its
direct neighbors, 12.9.5).

The math (force-directed layout, hit-testing, view-mode filtering) is
pulled out as pure functions in this module so it's all unit-testable
without GTK/Cairo. The GTK + Cairo plumbing wraps the math; the
`MeshTopologyRender` class is the integration point.
"""
from __future__ import annotations

import math
import random
from dataclasses import dataclass, field

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

from mackes import mackesd_bridge  # noqa: F401 — used at runtime via getattr
from mackes.workbench._common import a11y


# Health-state → fill RGB (libcosmic-style palette; matches the
# status-cluster pills in the dock).
_HEALTH_FILL = {
    "healthy":     (0.27, 0.79, 0.52),   # green
    "degraded":    (0.95, 0.74, 0.20),   # amber
    "unreachable": (0.86, 0.27, 0.27),   # red
    "unknown":     (0.55, 0.55, 0.55),   # neutral grey
}

# Edge state → stroke RGB.
_EDGE_COLOR = {
    "healthy":  (0.30, 0.60, 0.95),  # blue
    "missing":  (0.86, 0.27, 0.27),  # red (drift)
    "extra":    (0.95, 0.74, 0.20),  # amber (unexpected presence)
}


@dataclass
class Node:
    """One mesh peer for rendering. Pure data — no GTK refs."""
    node_id:  str
    label:    str = ""
    health:   str = "unknown"
    region:   str | None = None
    x:        float = 0.0
    y:        float = 0.0
    vx:       float = 0.0
    vy:       float = 0.0


@dataclass
class Edge:
    """One adjacency for rendering."""
    a:      str
    b:      str
    state:  str = "healthy"  # healthy | missing | extra


@dataclass
class Layout:
    """Snapshot of computed node positions + visible edges. Pure
    data; the renderer paints from this on every Cairo expose."""
    nodes:  dict[str, Node] = field(default_factory=dict)
    edges:  list[Edge] = field(default_factory=list)


def seed_positions(nodes: list[Node], width: float, height: float,
                   seed: int = 0) -> None:
    """Place every node on a deterministic ring inside `(width, height)`.

    Deterministic so unit tests and Cairo snapshot tests stay stable
    across runs. The force-directed pass in [`relax_layout`] perturbs
    these starting positions; this just gives the simulation a sane
    initial frame.
    """
    if not nodes:
        return
    rng = random.Random(seed)
    cx, cy = width / 2, height / 2
    radius = min(width, height) * 0.35
    step = (2 * math.pi) / max(1, len(nodes))
    for i, n in enumerate(nodes):
        angle = i * step + rng.uniform(-0.05, 0.05)
        n.x = cx + radius * math.cos(angle)
        n.y = cy + radius * math.sin(angle)
        n.vx = 0.0
        n.vy = 0.0


def relax_layout(nodes: list[Node], edges: list[Edge], width: float,
                 height: float, iterations: int = 60) -> None:
    """One force-directed relaxation pass.

    Spring-electrical model:
      - every pair of nodes repels (Coulomb-like, 1/r^2)
      - every edge is a spring pulling its two ends together
      - a weak attraction pulls every node toward the canvas center
        so disconnected clusters stay visible

    Mutates `nodes` in place (xy + velocity damping). Pure-ish:
    deterministic given the same inputs and node order; no I/O.
    """
    if not nodes:
        return
    by_id = {n.node_id: n for n in nodes}
    cx, cy = width / 2, height / 2
    repulse_strength = 4500.0
    spring_strength = 0.02
    rest_length = max(60.0, min(width, height) / max(2.0, math.sqrt(len(nodes))))
    centering = 0.001
    damping = 0.85
    max_step = 18.0

    for _ in range(iterations):
        # Reset force accumulator.
        forces = {n.node_id: [0.0, 0.0] for n in nodes}
        # Repulsion between every pair.
        for i, a in enumerate(nodes):
            for j in range(i + 1, len(nodes)):
                b = nodes[j]
                dx = a.x - b.x
                dy = a.y - b.y
                d2 = dx * dx + dy * dy + 0.01
                d  = math.sqrt(d2)
                f  = repulse_strength / d2
                fx = f * dx / d
                fy = f * dy / d
                forces[a.node_id][0] += fx
                forces[a.node_id][1] += fy
                forces[b.node_id][0] -= fx
                forces[b.node_id][1] -= fy
        # Spring attraction along edges.
        for e in edges:
            if e.a not in by_id or e.b not in by_id:
                continue
            a = by_id[e.a]
            b = by_id[e.b]
            dx = b.x - a.x
            dy = b.y - a.y
            d  = math.sqrt(dx * dx + dy * dy + 0.01)
            f  = spring_strength * (d - rest_length)
            fx = f * dx / d
            fy = f * dy / d
            forces[a.node_id][0] += fx
            forces[a.node_id][1] += fy
            forces[b.node_id][0] -= fx
            forces[b.node_id][1] -= fy
        # Weak pull toward center.
        for n in nodes:
            forces[n.node_id][0] += centering * (cx - n.x)
            forces[n.node_id][1] += centering * (cy - n.y)
        # Integrate + damp.
        for n in nodes:
            fx, fy = forces[n.node_id]
            n.vx = (n.vx + fx) * damping
            n.vy = (n.vy + fy) * damping
            # Cap per-step movement so the layout can't explode.
            n.vx = max(-max_step, min(max_step, n.vx))
            n.vy = max(-max_step, min(max_step, n.vy))
            n.x = max(20.0, min(width - 20.0, n.x + n.vx))
            n.y = max(20.0, min(height - 20.0, n.y + n.vy))


def hit_test_node(nodes: dict[str, Node], x: float, y: float,
                  radius: float = 18.0) -> Node | None:
    """Return the topmost node whose circle covers `(x, y)`, or None."""
    closest: Node | None = None
    closest_d2: float = radius * radius
    for n in nodes.values():
        dx = n.x - x
        dy = n.y - y
        d2 = dx * dx + dy * dy
        if d2 <= closest_d2:
            closest = n
            closest_d2 = d2
    return closest


def hit_test_edge(layout: Layout, x: float, y: float,
                  tolerance: float = 6.0) -> Edge | None:
    """Return the edge whose line passes within `tolerance` of
    `(x, y)`, or None. Naive O(E) walk — fine at the 8-peer scale (Q3 lock; was v12 16-peer)."""
    for e in layout.edges:
        if e.a not in layout.nodes or e.b not in layout.nodes:
            continue
        a = layout.nodes[e.a]
        b = layout.nodes[e.b]
        if point_to_segment_distance(x, y, a.x, a.y, b.x, b.y) <= tolerance:
            return e
    return None


def point_to_segment_distance(px: float, py: float,
                              ax: float, ay: float,
                              bx: float, by: float) -> float:
    """Shortest distance from point `(px,py)` to segment `(ax,ay)-(bx,by)`.
    Pure geometry helper, no external deps."""
    dx = bx - ax
    dy = by - ay
    length2 = dx * dx + dy * dy
    if length2 == 0.0:
        # Degenerate segment — distance to endpoint.
        return math.hypot(px - ax, py - ay)
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / length2))
    fx = ax + t * dx
    fy = ay + t * dy
    return math.hypot(px - fx, py - fy)


def filter_for_node_view(layout: Layout, focus_node_id: str) -> Layout:
    """Phase 12.9.5 — narrow the global layout to one peer + every
    direct neighbor. Pure function. Returns a fresh Layout."""
    if focus_node_id not in layout.nodes:
        return Layout()
    keep = {focus_node_id}
    for e in layout.edges:
        if e.a == focus_node_id and e.b in layout.nodes:
            keep.add(e.b)
        elif e.b == focus_node_id and e.a in layout.nodes:
            keep.add(e.a)
    kept_nodes = {nid: n for nid, n in layout.nodes.items() if nid in keep}
    kept_edges = [
        e for e in layout.edges
        if e.a in kept_nodes and e.b in kept_nodes
    ]
    return Layout(nodes=kept_nodes, edges=kept_edges)


def fetch_topology() -> Layout:
    """Read the live mesh topology from the bridge. Returns an empty
    Layout when mackesd isn't available — the renderer handles the
    empty case explicitly."""
    fn = getattr(mackesd_bridge, "topology_snapshot", None)
    if fn is None:
        return Layout()
    raw = fn()
    if raw is None:
        return Layout()
    nodes_data = raw.get("nodes", [])
    edges_data = raw.get("edges", [])
    nodes = {
        n["node_id"]: Node(
            node_id=n["node_id"],
            label=n.get("name", n["node_id"]),
            health=n.get("health", "unknown"),
            region=n.get("region"),
        )
        for n in nodes_data
    }
    edges = [
        Edge(a=e["a"], b=e["b"], state=e.get("state", "healthy"))
        for e in edges_data
    ]
    return Layout(nodes=nodes, edges=edges)


_VIEW_GLOBAL = "global"
_VIEW_NODE = "node"


class MeshTopologyRender(Gtk.Box):
    """Cairo-backed live topology view (Phase 12.9.1).

    Layout:
        [Global | Node]    (segmented toggle — Phase 12.9.5)
        [ Cairo drawing area, click-selectable nodes + edges ]
        [ Detail side panel — populated on selection ]
    """

    REFRESH_INTERVAL_MS = 5_000

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=8)

        # View-mode segmented control.
        header = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self._view_mode = _VIEW_GLOBAL
        self._focus_node_id: str | None = None
        self._global_btn = Gtk.ToggleButton(label="Global")
        self._node_btn = Gtk.ToggleButton(label="Node")
        self._global_btn.set_active(True)
        self._global_btn.connect("toggled", self._on_view_toggle, _VIEW_GLOBAL)
        self._node_btn.connect("toggled", self._on_view_toggle, _VIEW_NODE)
        a11y(self._global_btn, "Show entire mesh", tooltip=None)
        a11y(self._node_btn, "Show focused peer + direct neighbors", tooltip=None)
        header.pack_start(self._global_btn, False, False, 0)
        header.pack_start(self._node_btn, False, False, 0)
        self.pack_start(header, False, False, 0)

        # Drawing area + detail pane in a Paned.
        paned = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL)
        self._canvas = Gtk.DrawingArea()
        self._canvas.set_size_request(640, 480)
        self._canvas.set_events(self._canvas.get_events()
                                | 0x00000100)  # Gdk.EventMask.BUTTON_PRESS_MASK
        self._canvas.connect("draw", self._on_draw)
        self._canvas.connect("button-press-event", self._on_click)
        a11y(self._canvas, "Mesh topology canvas — click a node or edge for details",
             tooltip=None)
        paned.pack1(self._canvas, resize=True, shrink=False)

        self._detail = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        self._detail.set_margin_start(12); self._detail.set_margin_top(8)
        self._detail.set_size_request(240, -1)
        empty = Gtk.Label(label="(click a node or edge for details)")
        empty.set_xalign(0); empty.set_line_wrap(True)
        self._detail.pack_start(empty, False, False, 0)
        paned.pack2(self._detail, resize=False, shrink=False)
        self.pack_start(paned, True, True, 0)

        # State.
        self._layout = Layout()
        self._global_layout = Layout()
        self._selection: tuple[str, str] | None = None
        self._canvas_size: tuple[float, float] = (640.0, 480.0)
        self._timeout_id: int | None = None

        self.connect("realize", self._on_realize)
        self.connect("unrealize", self._on_unrealize)

    # --- view-mode + selection ---------------------------------------

    def _on_view_toggle(self, button: Gtk.ToggleButton, mode: str) -> None:
        if not button.get_active():
            return
        # Single-select: untoggle the other.
        if mode == _VIEW_GLOBAL:
            self._node_btn.set_active(False)
            self._view_mode = _VIEW_GLOBAL
        else:
            self._global_btn.set_active(False)
            self._view_mode = _VIEW_NODE
        self._apply_view_mode()
        self._canvas.queue_draw()

    def _apply_view_mode(self) -> None:
        if self._view_mode == _VIEW_NODE and self._focus_node_id:
            self._layout = filter_for_node_view(
                self._global_layout, self._focus_node_id,
            )
        else:
            self._layout = self._global_layout

    # --- timer ------------------------------------------------------

    def _on_realize(self, *_a) -> None:
        self._refresh_data()
        self._timeout_id = GLib.timeout_add(
            self.REFRESH_INTERVAL_MS, self._refresh_data,
        )

    def _on_unrealize(self, *_a) -> None:
        if self._timeout_id is not None:
            GLib.source_remove(self._timeout_id)
            self._timeout_id = None

    def _refresh_data(self) -> bool:
        new_layout = fetch_topology()
        # Preserve any positions that map to previously-seen node ids
        # so the layout doesn't churn between refreshes.
        for nid, n in new_layout.nodes.items():
            prior = self._global_layout.nodes.get(nid)
            if prior is not None:
                n.x = prior.x; n.y = prior.y
        w, h = self._canvas_size
        if new_layout.nodes and (not self._global_layout.nodes
                                  or any(n.x == 0.0 and n.y == 0.0
                                         for n in new_layout.nodes.values())):
            seed_positions(list(new_layout.nodes.values()), w, h)
        relax_layout(list(new_layout.nodes.values()),
                     new_layout.edges, w, h, iterations=30)
        self._global_layout = new_layout
        self._apply_view_mode()
        self._canvas.queue_draw()
        return True  # keep ticking

    # --- drawing -----------------------------------------------------

    def _on_draw(self, _widget, cr) -> None:
        alloc = self._canvas.get_allocation()
        w, h = float(alloc.width), float(alloc.height)
        self._canvas_size = (w, h)

        # Background.
        cr.set_source_rgb(0.07, 0.07, 0.09)
        cr.rectangle(0, 0, w, h)
        cr.fill()

        # Edges first (under nodes).
        for e in self._layout.edges:
            if e.a not in self._layout.nodes or e.b not in self._layout.nodes:
                continue
            a = self._layout.nodes[e.a]
            b = self._layout.nodes[e.b]
            r, g, b_c = _EDGE_COLOR.get(e.state, (0.5, 0.5, 0.5))
            cr.set_source_rgb(r, g, b_c)
            cr.set_line_width(2.0)
            cr.move_to(a.x, a.y)
            cr.line_to(b.x, b.y)
            cr.stroke()

        # Nodes.
        for n in self._layout.nodes.values():
            r, g, b_c = _HEALTH_FILL.get(n.health, _HEALTH_FILL["unknown"])
            cr.set_source_rgb(r, g, b_c)
            cr.arc(n.x, n.y, 12.0, 0, 2 * math.pi)
            cr.fill()
            cr.set_source_rgb(1.0, 1.0, 1.0)
            cr.move_to(n.x + 14, n.y + 4)
            cr.show_text(n.label or n.node_id)

        # Selection highlight.
        if self._selection is not None:
            kind, ident = self._selection
            if kind == "node" and ident in self._layout.nodes:
                n = self._layout.nodes[ident]
                cr.set_source_rgb(1.0, 1.0, 1.0)
                cr.set_line_width(2.0)
                cr.arc(n.x, n.y, 16.0, 0, 2 * math.pi)
                cr.stroke()

    # --- interaction --------------------------------------------------

    def _on_click(self, _widget, event) -> None:
        node = hit_test_node(self._layout.nodes, event.x, event.y)
        if node is not None:
            self._selection = ("node", node.node_id)
            self._set_detail_for_node(node)
            self._focus_node_id = node.node_id
            self._canvas.queue_draw()
            return
        edge = hit_test_edge(self._layout, event.x, event.y)
        if edge is not None:
            self._selection = ("edge", f"{edge.a}-{edge.b}")
            self._set_detail_for_edge(edge)
            self._canvas.queue_draw()

    def _set_detail_for_node(self, node: Node) -> None:
        for c in self._detail.get_children():
            self._detail.remove(c)
        title = Gtk.Label(label=node.label or node.node_id)
        title.set_xalign(0); title.get_style_context().add_class("mackes-page-title")
        self._detail.pack_start(title, False, False, 0)
        meta = Gtk.Label(label=(
            f"id:     {node.node_id}\n"
            f"health: {node.health}\n"
            f"region: {node.region or '(none)'}\n"
        ))
        meta.set_xalign(0); meta.set_line_wrap(True)
        self._detail.pack_start(meta, False, False, 0)
        self._detail.show_all()

    def _set_detail_for_edge(self, edge: Edge) -> None:
        for c in self._detail.get_children():
            self._detail.remove(c)
        title = Gtk.Label(label=f"{edge.a}  ↔  {edge.b}")
        title.set_xalign(0); title.get_style_context().add_class("mackes-page-title")
        self._detail.pack_start(title, False, False, 0)
        meta = Gtk.Label(label=f"state: {edge.state}")
        meta.set_xalign(0)
        self._detail.pack_start(meta, False, False, 0)
        self._detail.show_all()
