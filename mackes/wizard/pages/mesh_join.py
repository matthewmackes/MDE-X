"""Everything-On Mesh Onboarding wizard — v1.6.x.

A single-page wizard that gets a machine from "cold boot" to "fully joined
to the Mackes mesh" with one button click. Designed to be the antidote to
the multi-screen Headscale setup wizard for users who just want one
command that puts them on the mesh.

Flow:

  1. On entry, kick a background detection thread that probes:
       a. NetworkManager state                  (nmcli -t -f STATE general)
       b. tailscaled running                    (systemctl is-active / pidof)
       c. tailscale status                      (tailscale status --json)
       d. Headscale auth state                  (MeshState + tailscale Self)
       e. Control node reachability             (curl headscale_listen/health)
       f. QNM init state                        (qnmctl status)

  2. Render each probe as a Carbon checklist row:
       glyph · label · sub-label · per-row status pill.

  3. Single "Get me online" primary button. Pressed → on-screen one-shot
     confirmation, then run the remediation chain end-to-end on a daemon
     thread, mirroring the wizard apply-page log streaming pattern. Each
     step's output appends to the on-page log view.

  4. Idempotent: if every probe is green on entry (or after a remediation
     run), the primary button is replaced with a "✓ You're on the mesh"
     pill and a Re-check link.

All privileged calls go through `mackes.admin_session.AdminSession.run()`.
No raw pkexec. All UI mutation is funneled through `GLib.idle_add` from
worker threads — the main loop never blocks.

Design decision — auth flow:
  Tailscale-up against Headscale uses a pre-auth key (the existing
  `tailscale_up_with_headscale` helper). If no pre-auth key is yet
  cached (we have no peer's join link), we fall back to interactive
  device-auth via `tailscale up --login-server=<control>` — the URL is
  parsed out of stderr and shown in a copyable label plus optional QR
  code (rendered via `qrencode` if installed). This matches what
  `tailscale_bootstrap_login_url` already does for the seed peer.
"""
from __future__ import annotations

import re
import shutil
import subprocess
import threading
import time
from dataclasses import dataclass
from typing import Callable, List, Optional, Tuple

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf, GLib, Gtk  # noqa: E402

from mackes.admin_session import AdminSession
from mackes.logging import log_action

# NF-5.1 (v2.5 Nebula fabric): `mackes.mesh_vpn` retired with
# the Tailscale/Headscale stack. This v1.x wizard page is
# itself superseded by the Rust `mde-wizard` crate (NF-7.1)
# and only stays in-tree because `mackes/workbench/network/
# mesh_join.py` still wraps it for the legacy WorkbenchWindow
# launch path. We import the dead-stack symbols lazily through
# the shim below so module load survives `mesh_vpn.py`
# deletion; if an operator does click through to this page,
# the probes return "not ready" and the page surfaces the
# Rust wizard as the recommended entry point.
TAILSCALE_BIN = "tailscale"  # legacy CLI name; probes below
                              # check `_which` before invoking


def _legacy_mesh_state():  # type: ignore[no-untyped-def]
    """Lazy-load MeshState. Falls back to `_MissingMeshState`
    (a "not joined" stand-in) when mesh_vpn is gone."""
    try:
        from mackes.mesh_vpn import MeshState
    except ImportError:
        return _MissingMeshState
    return MeshState


def tailscale_status():  # type: ignore[no-untyped-def]
    """Lazy probe shim. Returns an empty status dict when the
    legacy `tailscale` CLI is gone (the v2.5 Nebula fabric
    doesn't need it)."""
    try:
        from mackes.mesh_vpn import tailscale_status as _ts
    except ImportError:
        return {}
    return _ts()


class _MissingMeshState:
    """Stand-in for MeshState when mesh_vpn is gone. Every
    method returns the zero / 'not joined' state so probes on
    this page render the Rust-wizard recommendation rather
    than crashing."""

    mesh_id = ""
    is_control = False
    control_peer_id = ""
    peer_count = 0

    @classmethod
    def load(cls):
        return cls()


# ---------------------------------------------------------------------------
# Probe / state machine constants
# ---------------------------------------------------------------------------


_STATE_UNKNOWN = "unknown"
_STATE_OK      = "ok"
_STATE_MISSING = "missing"
_STATE_WORKING = "working"
_STATE_FAIL    = "fail"

_PILL_LABEL = {
    _STATE_UNKNOWN: "checking…",
    _STATE_OK:      "ready",
    _STATE_MISSING: "needs attention",
    _STATE_WORKING: "working…",
    _STATE_FAIL:    "failed",
}

_PILL_CLASS = {
    _STATE_UNKNOWN: "muted",
    _STATE_OK:      "ok",
    _STATE_MISSING: "warn",
    _STATE_WORKING: "accent",
    _STATE_FAIL:    "fail",
}

_GLYPH = {
    _STATE_UNKNOWN: "○",
    _STATE_OK:      "✓",
    _STATE_MISSING: "!",
    _STATE_WORKING: "◐",
    _STATE_FAIL:    "✗",
}


# ---------------------------------------------------------------------------
# Probe records — one per checklist row
# ---------------------------------------------------------------------------


@dataclass
class _Probe:
    key:       str
    label:     str
    sub:       str = ""
    state:     str = _STATE_UNKNOWN
    # Render-time references — filled in build_row(); never read off the GUI
    # thread. The probe value itself is computed off-thread but pushed back
    # via idle_add(_set_probe_state).
    _glyph_lbl: Optional[Gtk.Label] = None
    _name_lbl:  Optional[Gtk.Label] = None
    _sub_lbl:   Optional[Gtk.Label] = None
    _pill_lbl:  Optional[Gtk.Label] = None
    _row:       Optional[Gtk.Widget] = None

    def build_row(self) -> Gtk.Widget:
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        row.set_margin_top(8); row.set_margin_bottom(8)
        row.set_margin_start(16); row.set_margin_end(16)
        row.get_style_context().add_class("mackes-side-nav-item")

        self._glyph_lbl = Gtk.Label(label=_GLYPH[self.state])
        self._glyph_lbl.set_size_request(24, -1)
        self._glyph_lbl.set_xalign(0)
        self._glyph_lbl.get_style_context().add_class("mackes-dot")
        self._glyph_lbl.get_style_context().add_class(_PILL_CLASS[self.state])
        row.pack_start(self._glyph_lbl, False, False, 0)

        text_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        self._name_lbl = Gtk.Label(label=self.label)
        self._name_lbl.set_xalign(0)
        self._name_lbl.get_style_context().add_class("mackes-section-title")
        text_box.pack_start(self._name_lbl, False, False, 0)
        self._sub_lbl = Gtk.Label(label=self.sub or _PILL_LABEL[self.state])
        self._sub_lbl.set_xalign(0); self._sub_lbl.set_line_wrap(True)
        self._sub_lbl.get_style_context().add_class("mackes-page-subtitle")
        text_box.pack_start(self._sub_lbl, False, False, 0)
        row.pack_start(text_box, True, True, 0)

        self._pill_lbl = Gtk.Label(label=_PILL_LABEL[self.state])
        self._pill_lbl.set_xalign(1)
        self._pill_lbl.get_style_context().add_class("mackes-section-meta")
        self._pill_lbl.get_style_context().add_class(_PILL_CLASS[self.state])
        row.pack_end(self._pill_lbl, False, False, 0)

        self._row = row
        return row

    def set(self, state: str, sub: Optional[str] = None) -> None:
        self.state = state
        if sub is not None:
            self.sub = sub
        if self._glyph_lbl is not None:
            self._glyph_lbl.set_text(_GLYPH[state])
            ctx = self._glyph_lbl.get_style_context()
            for v in ("ok", "warn", "fail", "muted", "accent"):
                ctx.remove_class(v)
            ctx.add_class(_PILL_CLASS[state])
        if self._sub_lbl is not None and sub is not None:
            self._sub_lbl.set_text(sub)
        if self._pill_lbl is not None:
            self._pill_lbl.set_text(_PILL_LABEL[state])
            ctx = self._pill_lbl.get_style_context()
            for v in ("ok", "warn", "fail", "muted", "accent"):
                ctx.remove_class(v)
            ctx.add_class(_PILL_CLASS[state])


# ---------------------------------------------------------------------------
# Probe helpers — pure functions, safe to call off the GUI thread
# ---------------------------------------------------------------------------


def _which(cmd: str) -> bool:
    return shutil.which(cmd) is not None


def _run_quick(cmd: List[str], *, timeout: int = 5) -> Tuple[int, str, str]:
    """Non-privileged subprocess wrapper. Never raises."""
    try:
        r = subprocess.run(
            cmd, capture_output=True, text=True, timeout=timeout,
        )
        return r.returncode, r.stdout or "", r.stderr or ""
    except FileNotFoundError:
        return 127, "", f"not found: {cmd[0]}"
    except subprocess.TimeoutExpired:
        return 124, "", f"timeout: {' '.join(cmd)}"
    except OSError as e:
        return 1, "", str(e)


def _probe_network_manager() -> Tuple[str, str]:
    """(state, sub) for NetworkManager.

    Three cases:
      * `nmcli` missing → MISSING, "NetworkManager not installed"
      * connected      → OK, "connected via <device>" or "connected"
      * else           → MISSING, "<reported state>"
    """
    if not _which("nmcli"):
        return _STATE_MISSING, "NetworkManager (nmcli) not installed"
    rc, out, _ = _run_quick(["nmcli", "-t", "-f", "STATE", "general"])
    state = (out.strip().splitlines() or [""])[0].strip().lower()
    if rc != 0 or not state:
        return _STATE_MISSING, "nmcli could not report state"
    if state == "connected":
        # Try to grab the active device for a friendlier sub-label.
        _rc2, out2, _ = _run_quick(
            ["nmcli", "-t", "-f", "NAME,TYPE,DEVICE", "connection", "show",
             "--active"],
        )
        line = (out2.strip().splitlines() or [""])[0]
        if line:
            parts = line.split(":")
            if len(parts) >= 3:
                return _STATE_OK, f"connected via {parts[2]} ({parts[1]})"
        return _STATE_OK, "connected"
    return _STATE_MISSING, f"NetworkManager state: {state}"


def _probe_tailscaled() -> Tuple[str, str]:
    """tailscaled process / systemd unit status."""
    if not _which(TAILSCALE_BIN) and not _which("tailscale"):
        return _STATE_MISSING, "tailscale CLI not installed"
    # Prefer systemctl is-active — works whether or not we can sudo.
    rc, out, _ = _run_quick(["systemctl", "is-active", "tailscaled"])
    state = out.strip()
    if rc == 0 and state == "active":
        return _STATE_OK, "tailscaled.service is active"
    # Fall back to a pidof-style probe (no privilege needed).
    rc2, out2, _ = _run_quick(["pidof", "tailscaled"])
    if rc2 == 0 and out2.strip():
        return _STATE_OK, "tailscaled running (no systemd unit reported)"
    return _STATE_MISSING, f"tailscaled not running ({state or 'inactive'})"


def _probe_tailscale_authed() -> Tuple[str, str]:
    """Is the local Tailscale client authed to a control plane?"""
    ts = tailscale_status()
    if not ts.get("installed"):
        return _STATE_MISSING, "tailscale CLI not installed"
    if ts.get("online"):
        ip = ts.get("mesh_ip") or "—"
        peers = len(ts.get("peers") or [])
        return _STATE_OK, f"online · mesh IP {ip} · {peers} peer(s) visible"
    # Check whether MeshState says we *should* be joined — that tells the
    # user "needs auth" vs "never joined".
    st = _legacy_mesh_state().load()
    if st.mesh_id:
        return _STATE_MISSING, (
            f"mesh state present (mesh-id {st.mesh_id[:8]}) but tailscale "
            "is not reporting online — needs auth"
        )
    return _STATE_MISSING, "not joined to any mesh yet"


def _probe_control_reachable() -> Tuple[str, str]:
    """Can we hit the control node's /health?"""
    st = _legacy_mesh_state().load()
    target = (st.headscale_listen or "").rstrip("/")
    if not target:
        return _STATE_MISSING, "no control node recorded yet"
    if not _which("curl"):
        return _STATE_MISSING, "curl not installed (needed for reachability check)"
    rc, _, _ = _run_quick(
        ["curl", "-fsS", "-m", "3", f"{target}/health"], timeout=5,
    )
    if rc == 0:
        return _STATE_OK, f"control node reachable at {target}"
    return _STATE_MISSING, f"control node {target} unreachable"


def _probe_qnm() -> Tuple[str, str]:
    """qnmctl status — initialized?"""
    from mackes import qnm_bridge
    if not qnm_bridge.have_qnm():
        return _STATE_MISSING, "qnmctl not installed"
    s = qnm_bridge.status() or {}
    if "error" in s:
        return _STATE_MISSING, f"qnmctl status failed: {s['error'][:80]}"
    # Heuristic — qnmctl status returns a parsed dict with "raw"; if raw
    # is empty or mentions "not initialized" we flag MISSING.
    raw = (s.get("raw") or "").lower()
    if not raw or "not initialized" in raw or "not configured" in raw:
        return _STATE_MISSING, "qnm installed but not initialized"
    return _STATE_OK, "qnm initialized"


# ---------------------------------------------------------------------------
# Wi-Fi scan helper (only invoked when NM has no active connection)
# ---------------------------------------------------------------------------


def _scan_wifi_ssids() -> List[Tuple[str, str, str]]:
    """Return [(ssid, signal, security), …] sorted by signal desc.

    Empty list if Wi-Fi is unavailable. Wrapped to never raise.
    """
    if not _which("nmcli"):
        return []
    rc, out, _ = _run_quick(
        ["nmcli", "-t", "-f", "SSID,SIGNAL,SECURITY", "device", "wifi", "list"],
        timeout=8,
    )
    if rc != 0:
        return []
    seen: set[str] = set()
    rows: List[Tuple[str, str, str]] = []
    for line in out.splitlines():
        # nmcli -t separates fields with ":" but SSIDs may contain ":";
        # split with maxsplit=2 from the right to keep SSID intact.
        parts = line.split(":")
        if len(parts) < 3:
            continue
        # Reassemble SSID if it had embedded colons (rare).
        security = parts[-1]
        signal   = parts[-2]
        ssid     = ":".join(parts[:-2])
        if not ssid or ssid in seen:
            continue
        seen.add(ssid)
        rows.append((ssid, signal, security or "Open"))
    rows.sort(key=lambda r: -int(r[1] or 0))
    return rows


# ---------------------------------------------------------------------------
# QR code rendering (optional — qrencode CLI)
# ---------------------------------------------------------------------------


def _qr_pixbuf(text: str, *, size: int = 220) -> Optional[GdkPixbuf.Pixbuf]:
    """Render `text` as a QR code via the `qrencode` CLI. Returns None if
    qrencode isn't installed or the call fails."""
    if not _which("qrencode"):
        return None
    try:
        proc = subprocess.run(
            ["qrencode", "-t", "PNG", "-s", "6", "-m", "2", "-o", "-", text],
            capture_output=True, timeout=4,
        )
        if proc.returncode != 0 or not proc.stdout:
            return None
        loader = GdkPixbuf.PixbufLoader.new_with_type("png")
        loader.write(proc.stdout)
        loader.close()
        pb = loader.get_pixbuf()
        if pb is None:
            return None
        return pb.scale_simple(size, size, GdkPixbuf.InterpType.BILINEAR)
    except (OSError, subprocess.TimeoutExpired, Exception):  # noqa: BLE001
        return None


# ---------------------------------------------------------------------------
# MeshJoinPage — the actual wizard page widget
# ---------------------------------------------------------------------------


class MeshJoinPage(Gtk.Box):
    """Single-page mesh onboarding.

    Embeddable as a wizard step (pass a `ctx` if used inside the
    first-run wizard) or as a sidebar panel (no ctx; works fine).

    Public surface:

      run()        — alias for re-running detection; no-op if already running.
      is_done()    — wizard-step compatibility; True after first detection
                     completes (we don't gate wizard progression on the
                     remediation actually being applied).
    """

    def __init__(self, ctx: Optional[object] = None,
                 *, on_complete: Optional[Callable[[], None]] = None) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.ctx = ctx
        self._on_complete = on_complete
        self._done = False
        self._detecting = False
        self._applying = False
        self._chain_cancel = threading.Event()

        # Cached probe data — populated by detection thread
        self._probes: List[_Probe] = self._initial_probes()
        self._control_url: Optional[str] = None
        self._auth_url: Optional[str] = None
        self._auth_url_pixbuf: Optional[GdkPixbuf.Pixbuf] = None

        self._build()
        # Kick off detection on entry
        self._start_detection()

    # ---- public surface --------------------------------------------------

    def is_done(self) -> bool:
        return self._done

    def run(self) -> None:
        """Wizard-step compatibility shim. Re-runs detection."""
        if not self._detecting:
            self._start_detection()

    # ---- UI construction --------------------------------------------------

    @staticmethod
    def _initial_probes() -> List[_Probe]:
        return [
            _Probe("nm",         "Internet (NetworkManager)",
                   sub="Checking…"),
            _Probe("tailscaled", "Tailscale daemon",
                   sub="Checking…"),
            _Probe("ts_auth",    "Mesh registration",
                   sub="Checking…"),
            _Probe("control",    "Control node reachable",
                   sub="Checking…"),
            _Probe("qnm",        "Quick Network Mesh (QNM)",
                   sub="Checking…"),
        ]

    def _build(self) -> None:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(32); outer.set_margin_bottom(24)
        outer.set_margin_start(40); outer.set_margin_end(40)

        # Page title + subtitle
        self._title = Gtk.Label(label="Get me on the mesh")
        self._title.set_xalign(0)
        self._title.get_style_context().add_class("mackes-page-title")
        outer.pack_start(self._title, False, False, 0)
        self._subtitle = Gtk.Label(label=(
            "One button gets this machine on a usable network and joined "
            "to the Mackes mesh. We check what's already working, show "
            "what's missing, and fix it in one go. Anything already done "
            "stays done."
        ))
        self._subtitle.set_xalign(0); self._subtitle.set_line_wrap(True)
        self._subtitle.get_style_context().add_class("mackes-page-subtitle")
        outer.pack_start(self._subtitle, False, False, 0)

        # ---- Two-column body: checklist on left, action+log on right ----
        body = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=24)
        body.set_margin_top(24)

        # LEFT — checklist
        left = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        left.set_size_request(420, -1)
        left.get_style_context().add_class("mackes-side-nav")
        hdr = Gtk.Label(label="CHECKLIST")
        hdr.set_xalign(0)
        hdr.set_margin_top(12); hdr.set_margin_bottom(4)
        hdr.set_margin_start(16); hdr.set_margin_end(16)
        hdr.get_style_context().add_class("mackes-side-nav-group-title")
        left.pack_start(hdr, False, False, 0)
        self._rows_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        for p in self._probes:
            self._rows_box.pack_start(p.build_row(), False, False, 0)
        left.pack_start(self._rows_box, False, False, 0)

        # Cross-link to the unified Mesh Health view — the wizard
        # focuses on getting you online; once you are, this is where
        # you check every layer end-to-end.
        try:
            health_btn = Gtk.LinkButton.new_with_label(
                "mackes://network/mesh_health", "View full mesh health →")
            health_btn.set_margin_top(12)
            health_btn.set_margin_start(16); health_btn.set_margin_end(16)
            health_btn.set_halign(Gtk.Align.START)
            left.pack_start(health_btn, False, False, 0)
        except Exception:  # noqa: BLE001
            pass

        body.pack_start(left, False, False, 0)

        # RIGHT — action area + log
        right = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)

        # Big action button — primary or "you're online" state
        self._action_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        self._action_status = Gtk.Label(label="Checking your machine…")
        self._action_status.set_xalign(0); self._action_status.set_line_wrap(True)
        self._action_status.get_style_context().add_class("mackes-section-title")
        self._action_box.pack_start(self._action_status, False, False, 0)

        self._action_sub = Gtk.Label(label="")
        self._action_sub.set_xalign(0); self._action_sub.set_line_wrap(True)
        self._action_sub.get_style_context().add_class("mackes-page-subtitle")
        self._action_box.pack_start(self._action_sub, False, False, 0)

        # Wi-Fi picker (created lazily, hidden until needed)
        self._wifi_combo: Optional[Gtk.ComboBoxText] = None
        self._wifi_password: Optional[Gtk.Entry] = None
        self._wifi_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        self._wifi_box.set_no_show_all(True)
        self._action_box.pack_start(self._wifi_box, False, False, 0)

        # Primary action row (button + recheck link)
        btn_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        btn_row.set_margin_top(8)
        self._primary_btn = Gtk.Button(label="Get me online")
        self._primary_btn.get_style_context().add_class("cds-button-primary")
        self._primary_btn.get_style_context().add_class("suggested-action")
        self._primary_btn.set_sensitive(False)
        self._primary_btn.connect("clicked", self._on_primary_click)
        btn_row.pack_start(self._primary_btn, False, False, 0)

        self._recheck_btn = Gtk.Button(label="Re-check")
        self._recheck_btn.get_style_context().add_class("cds-button-ghost")
        self._recheck_btn.connect("clicked", lambda *_: self._start_detection())
        btn_row.pack_start(self._recheck_btn, False, False, 0)
        self._action_box.pack_start(btn_row, False, False, 0)

        right.pack_start(self._action_box, False, False, 0)

        # Auth-URL panel (shown when tailscale up prints a device-auth URL)
        self._auth_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        self._auth_box.set_margin_top(12)
        self._auth_box.set_no_show_all(True)
        self._auth_url_label = Gtk.Entry()
        self._auth_url_label.set_editable(False)
        self._auth_url_label.set_can_focus(True)
        self._auth_url_label.get_style_context().add_class("mackes-code")
        self._auth_box.pack_start(self._auth_url_label, False, False, 0)
        self._auth_qr_image = Gtk.Image()
        self._auth_qr_image.set_no_show_all(True)
        self._auth_box.pack_start(self._auth_qr_image, False, False, 0)
        auth_actions = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        copy_btn = Gtk.Button(label="Copy URL")
        copy_btn.get_style_context().add_class("cds-button-tertiary")
        copy_btn.connect("clicked", self._on_copy_auth_url)
        auth_actions.pack_start(copy_btn, False, False, 0)
        open_btn = Gtk.Button(label="Open in browser")
        open_btn.get_style_context().add_class("cds-button-ghost")
        open_btn.connect("clicked", self._on_open_auth_url)
        auth_actions.pack_start(open_btn, False, False, 0)
        self._auth_box.pack_start(auth_actions, False, False, 0)
        right.pack_start(self._auth_box, False, False, 0)

        # Log area — last N log lines from the apply chain
        log_hdr = Gtk.Label(label="LOG")
        log_hdr.set_xalign(0); log_hdr.set_margin_top(16)
        log_hdr.get_style_context().add_class("mackes-side-nav-group-title")
        right.pack_start(log_hdr, False, False, 0)
        self._log = Gtk.TextView()
        self._log.set_editable(False); self._log.set_monospace(True)
        self._log.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        self._log.get_style_context().add_class("mackes-code")
        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(self._log)
        scroller.set_size_request(-1, 220)
        right.pack_start(scroller, True, True, 0)

        body.pack_start(right, True, True, 0)
        outer.pack_start(body, True, True, 0)
        self.pack_start(outer, True, True, 0)

    # ---- detection thread ------------------------------------------------

    def _start_detection(self) -> None:
        if self._detecting or self._applying:
            return
        self._detecting = True
        self._primary_btn.set_sensitive(False)
        self._action_status.set_text("Checking your machine…")
        self._action_sub.set_text("")
        # Reset every probe to "unknown"
        for p in self._probes:
            p.set(_STATE_UNKNOWN, sub="Checking…")
        threading.Thread(
            target=self._detect_loop, daemon=True,
            name="mackes-mesh-join-detect",
        ).start()

    def _detect_loop(self) -> None:
        """Run all probes off-thread, posting results back via idle_add."""
        results: List[Tuple[str, str, str]] = []
        for key, fn in (
            ("nm",         _probe_network_manager),
            ("tailscaled", _probe_tailscaled),
            ("ts_auth",    _probe_tailscale_authed),
            ("control",    _probe_control_reachable),
            ("qnm",        _probe_qnm),
        ):
            try:
                state, sub = fn()
            except Exception as e:  # noqa: BLE001
                state, sub = _STATE_FAIL, f"probe error: {e}"
                log_action(f"mesh_join probe {key} crashed: {e}")
            results.append((key, state, sub))
            GLib.idle_add(self._apply_probe_result, key, state, sub)
        GLib.idle_add(self._detect_finished, results)

    def _apply_probe_result(self, key: str, state: str, sub: str) -> bool:
        for p in self._probes:
            if p.key == key:
                p.set(state, sub=sub)
                break
        return False

    def _detect_finished(self, results: List[Tuple[str, str, str]]) -> bool:
        self._detecting = False
        self._done = True  # wizard-step compatibility — detection done
        missing = [k for k, st, _ in results if st != _STATE_OK]

        if not missing:
            # Everything green — replace primary with "you're on the mesh"
            self._render_all_green()
        else:
            # Surface the chain
            self._render_remediation_ready(missing)
        if self._on_complete is not None:
            try:
                self._on_complete()
            except Exception:  # noqa: BLE001
                pass
        return False

    def _render_all_green(self) -> None:
        self._action_status.set_text("✓ You're on the mesh")
        # Try to pull a concrete fact for the sub-label
        ts = tailscale_status()
        ip = ts.get("mesh_ip") or "—"
        peers = len(ts.get("peers") or [])
        self._action_sub.set_text(
            f"Mesh IP {ip} · {peers} peer(s) visible. Use Re-check to "
            "verify everything is still healthy."
        )
        self._primary_btn.set_label("Already online")
        self._primary_btn.set_sensitive(False)
        self._auth_box.set_visible(False)
        self._wifi_box.set_visible(False)

    def _render_remediation_ready(self, missing_keys: List[str]) -> None:
        self._action_status.set_text("Ready to get you online")
        n = len(missing_keys)
        labels = {p.key: p.label for p in self._probes}
        humans = ", ".join(labels.get(k, k) for k in missing_keys)
        self._action_sub.set_text(
            f"{n} item(s) need action: {humans}. Click 'Get me online' to "
            "run the full chain — every step is idempotent and logged below."
        )
        # If NM is the blocker AND we have no active connection AND Wi-Fi
        # candidates are present, surface the SSID picker.
        if "nm" in missing_keys:
            self._maybe_show_wifi_picker()
        self._primary_btn.set_sensitive(True)
        self._primary_btn.set_label("Get me online")

    def _maybe_show_wifi_picker(self) -> None:
        # Only relevant if there's no wired connection up. The presence
        # of nmcli wifi list rows is the signal.
        ssids = _scan_wifi_ssids()
        if not ssids:
            self._wifi_box.set_visible(False)
            return
        # Lazily build (only on first show)
        if self._wifi_combo is None:
            lbl = Gtk.Label(label="Pick a Wi-Fi network (optional — leave "
                                  "blank for wired or already-saved):")
            lbl.set_xalign(0); lbl.set_line_wrap(True)
            lbl.get_style_context().add_class("mackes-page-subtitle")
            self._wifi_box.pack_start(lbl, False, False, 0)
            self._wifi_combo = Gtk.ComboBoxText()
            self._wifi_combo.append_text("(skip — use whatever NM has)")
            self._wifi_box.pack_start(self._wifi_combo, False, False, 0)
            pw_lbl = Gtk.Label(label="Password (if secured — leave blank "
                                     "for open or saved):")
            pw_lbl.set_xalign(0)
            pw_lbl.get_style_context().add_class("mackes-page-subtitle")
            self._wifi_box.pack_start(pw_lbl, False, False, 0)
            self._wifi_password = Gtk.Entry()
            self._wifi_password.set_visibility(False)
            self._wifi_box.pack_start(self._wifi_password, False, False, 0)
        # Repopulate combo
        # Clear all but the first (skip) entry
        while True:
            text = self._wifi_combo.get_model()[1] if len(
                self._wifi_combo.get_model()) > 1 else None
            if text is None:
                break
            self._wifi_combo.remove(1)
        for ssid, signal, security in ssids[:20]:
            self._wifi_combo.append_text(f"{ssid}  ({signal}%, {security})")
        self._wifi_combo.set_active(0)
        self._wifi_box.set_visible(True)
        self._wifi_box.show_all()

    # ---- "Get me online" button -----------------------------------------

    def _on_primary_click(self, *_args) -> None:
        if self._applying:
            return
        self._applying = True
        self._primary_btn.set_sensitive(False)
        self._recheck_btn.set_sensitive(False)
        self._primary_btn.set_label("Working…")
        self._chain_cancel.clear()
        # Snapshot the picked SSID + password BEFORE leaving the GUI thread
        picked_ssid: Optional[str] = None
        picked_pw:   Optional[str] = None
        if self._wifi_combo is not None and self._wifi_combo.get_visible():
            idx = self._wifi_combo.get_active()
            if idx > 0:
                # Strip the "  (signal%, security)" suffix
                raw = self._wifi_combo.get_active_text() or ""
                picked_ssid = re.sub(r"\s+\(\d+%, .*?\)\s*$", "", raw).strip()
                picked_pw = (self._wifi_password.get_text()
                             if self._wifi_password is not None else None)
        threading.Thread(
            target=self._apply_chain, args=(picked_ssid, picked_pw),
            daemon=True, name="mackes-mesh-join-apply",
        ).start()

    def _apply_chain(self, picked_ssid: Optional[str],
                     picked_pw: Optional[str]) -> None:
        """Run the remediation chain — one step per missing probe."""
        admin = AdminSession.instance()
        # Snapshot which probes need work so we don't loop on stale state
        missing = {p.key for p in self._probes if p.state != _STATE_OK}

        def _step(probe_key: str, title: str, fn: Callable[[], List[str]]) -> bool:
            if self._chain_cancel.is_set():
                return False
            GLib.idle_add(self._set_probe_state, probe_key,
                          _STATE_WORKING, "running…")
            GLib.idle_add(self._log_line, f"── {title}")
            try:
                lines = fn() or []
                ok = True
            except Exception as e:  # noqa: BLE001
                lines = [f"ERROR: {e}"]
                log_action(f"mesh_join step {title} failed: {e}")
                ok = False
            for line in lines:
                GLib.idle_add(self._log_line, f"  {line}")
            return ok

        # 1. NetworkManager
        if "nm" in missing:
            _step("nm", "NetworkManager: bring up a connection",
                  lambda: self._step_nm(admin, picked_ssid, picked_pw))

        # 2. tailscaled service
        if "tailscaled" in missing:
            _step("tailscaled", "Tailscale daemon: start systemd unit",
                  lambda: self._step_tailscaled(admin))

        # 3. Tailscale auth — must come AFTER tailscaled is running
        if "ts_auth" in missing or "control" in missing:
            _step("ts_auth", "Tailscale: join the mesh",
                  lambda: self._step_tailscale_up(admin))

        # 4. QNM init
        if "qnm" in missing:
            _step("qnm", "QNM: initialize",
                  lambda: self._step_qnm_init(admin))

        # Re-probe everything from scratch to get the live truth
        GLib.idle_add(self._log_line, "── Re-running detection")
        GLib.idle_add(self._chain_finished)

    # ---- step implementations (off-thread) -------------------------------

    def _step_nm(self, admin: AdminSession, ssid: Optional[str],
                 password: Optional[str]) -> List[str]:
        lines: List[str] = []
        if not _which("nmcli"):
            lines.append("nmcli not installed; cannot bring up network")
            return lines
        # If user picked a specific SSID, try to connect to that.
        if ssid:
            cmd = ["nmcli", "device", "wifi", "connect", ssid]
            if password:
                cmd += ["password", password]
            rc, out = admin.run(cmd, timeout=45)
            lines.append(f"nmcli device wifi connect {ssid!r} rc={rc}")
            for ln in (out or "").splitlines():
                lines.append(f"  {ln}")
            if rc == 0:
                return lines
            # Falls through to generic "bring up any saved connection"
            lines.append("falling back to nmcli connection up …")

        # Generic recovery — try to bring up the first inactive connection
        rc, out, _ = _run_quick(
            ["nmcli", "-t", "-f", "NAME,STATE", "connection", "show"],
        )
        candidates: List[str] = []
        for line in (out or "").splitlines():
            parts = line.split(":")
            if len(parts) >= 2 and parts[-1].strip() != "activated":
                candidates.append(":".join(parts[:-1]))
        if not candidates:
            lines.append("no inactive connections found — NetworkManager "
                         "may already be doing the right thing")
        for cand in candidates[:3]:
            rc, out = admin.run(["nmcli", "connection", "up", cand], timeout=30)
            lines.append(f"nmcli connection up {cand!r} rc={rc}")
            for ln in (out or "").splitlines():
                lines.append(f"  {ln}")
            if rc == 0:
                break
        return lines

    def _step_tailscaled(self, admin: AdminSession) -> List[str]:
        lines: List[str] = []
        if not _which(TAILSCALE_BIN) and not _which("tailscale"):
            return ["tailscale CLI not installed — install via Maintain → "
                    "Dependencies, then re-run"]
        rc, out = admin.run(
            ["systemctl", "enable", "--now", "tailscaled"], timeout=20,
        )
        lines.append(f"systemctl enable --now tailscaled rc={rc}")
        for ln in (out or "").splitlines()[-6:]:
            lines.append(f"  {ln}")
        # Brief wait for the daemon socket
        for _ in range(20):
            r, _o, _ = _run_quick(["systemctl", "is-active", "tailscaled"])
            if r == 0:
                lines.append("tailscaled is now active")
                break
            time.sleep(0.2)
        return lines

    def _step_tailscale_up(self, admin: AdminSession) -> List[str]:
        """Auth this peer against the recorded Headscale control plane.

        Design choice: if MeshState has a control_peer_id we use that
        Headscale URL with interactive device-auth (the URL is parsed
        from stderr and surfaced on the page). We do NOT carry a
        pre-auth key here — that's reserved for the dedicated Headscale
        wizard's seed/join flow, which has the secret context.

        If MeshState has no control plane recorded, we fall back to
        the Tailscale-hosted coordination server (Tailscale's own
        device-auth). This gets the daemon running and lets the user
        complete onboarding through the Headscale wizard later.
        """
        lines: List[str] = []
        if not _which(TAILSCALE_BIN):
            return ["tailscale CLI not installed"]
        st = _legacy_mesh_state().load()
        control = (st.headscale_listen or "").rstrip("/")
        cmd: List[str] = [TAILSCALE_BIN, "up", "--accept-routes=true",
                          "--accept-dns=true", "--ssh=true", "--reset"]
        if control:
            cmd.append("--login-server=" + control)
            lines.append(f"using Headscale control plane at {control}")
        else:
            lines.append(
                "no Headscale control plane recorded — using Tailscale's "
                "hosted coordination server (login.tailscale.com). After "
                "auth completes, run the Headscale Setup Wizard to take "
                "ownership of the mesh."
            )
        # Run tailscale up with a longer timeout to allow user interaction.
        # We capture both streams and surface any device-auth URL the CLI
        # printed back to the GUI.
        rc, combined = admin.run(cmd, timeout=180)
        lines.append(f"tailscale up rc={rc}")
        url: Optional[str] = None
        for line in (combined or "").splitlines():
            lines.append(f"  {line}")
            # `tailscale up` prints lines like:
            #   To authenticate, visit:
            #     https://login.tailscale.com/a/...
            m = re.search(r"(https?://\S+)", line)
            if m and ("login.tailscale.com" in line or
                      "register" in line.lower() or
                      (control and control in line)):
                url = m.group(1)
        if url:
            GLib.idle_add(self._show_auth_url, url)
            lines.append(f"device-auth URL surfaced: {url}")
            # Poll briefly for "Online" — gives the user up to 5 minutes
            # to complete the browser auth before we give up. The polling
            # is non-blocking from the GUI thread's perspective; we're
            # already on a worker thread.
            deadline = time.monotonic() + 300
            while time.monotonic() < deadline:
                if self._chain_cancel.is_set():
                    break
                time.sleep(2)
                ts = tailscale_status()
                if ts.get("online"):
                    lines.append("tailscale reports Online=true")
                    GLib.idle_add(self._hide_auth_url)
                    return lines
            if not tailscale_status().get("online"):
                lines.append("auth did not complete within 5 minutes — "
                             "click 'Get me online' again after signing in")
        return lines

    def _step_qnm_init(self, admin: AdminSession) -> List[str]:
        lines: List[str] = []
        from mackes import qnm_bridge
        if not qnm_bridge.have_qnm():
            return ["qnmctl not installed — install via Maintain → "
                    "Dependencies"]
        # qnmctl init is typically a user-level op, but the spec routes
        # init through admin in case it touches /etc/qnm — that's safe
        # either way thanks to NOPASSWD coverage in /etc/sudoers.d/.
        rc, out = admin.run([qnm_bridge.QNMCTL, "init"], timeout=30)
        lines.append(f"qnmctl init rc={rc}")
        for ln in (out or "").splitlines()[-10:]:
            lines.append(f"  {ln}")
        if rc != 0:
            # Some qnmctl builds don't have a separate 'init'; fall back
            # to start, which lazy-initializes.
            rc2, out2 = admin.run([qnm_bridge.QNMCTL, "start"], timeout=15)
            lines.append(f"qnmctl start rc={rc2}")
            for ln in (out2 or "").splitlines()[-6:]:
                lines.append(f"  {ln}")
        return lines

    # ---- UI helpers (main thread) ----------------------------------------

    def _set_probe_state(self, key: str, state: str, sub: str) -> bool:
        for p in self._probes:
            if p.key == key:
                p.set(state, sub=sub)
                break
        return False

    def _log_line(self, text: str) -> bool:
        buf = self._log.get_buffer()
        end = buf.get_end_iter()
        buf.insert(end, text + "\n")
        end = buf.get_end_iter()
        self._log.scroll_to_iter(end, 0, False, 0, 1)
        return False

    def _chain_finished(self) -> bool:
        self._applying = False
        self._recheck_btn.set_sensitive(True)
        # Re-run detection so every row picks up the new truth
        self._start_detection()
        return False

    def _show_auth_url(self, url: str) -> bool:
        self._auth_url = url
        self._auth_url_label.set_text(url)
        pb = _qr_pixbuf(url, size=220)
        if pb is not None:
            self._auth_qr_image.set_from_pixbuf(pb)
            self._auth_qr_image.set_visible(True)
        else:
            self._auth_qr_image.set_visible(False)
        self._auth_box.set_visible(True)
        self._auth_box.show_all()
        # The QR image may have been hidden again by show_all; re-honour
        # the no_show_all guard when there's no pixbuf.
        if pb is None:
            self._auth_qr_image.set_visible(False)
        return False

    def _hide_auth_url(self) -> bool:
        self._auth_box.set_visible(False)
        return False

    def _on_copy_auth_url(self, *_args) -> None:
        if not self._auth_url:
            return
        try:
            from gi.repository import Gdk
            clip = Gtk.Clipboard.get_default(Gdk.Display.get_default())
            clip.set_text(self._auth_url, -1)
            clip.store()
            self._log_line(f"copied {self._auth_url} to clipboard")
        except Exception as e:  # noqa: BLE001
            self._log_line(f"clipboard copy failed: {e}")

    def _on_open_auth_url(self, *_args) -> None:
        if not self._auth_url:
            return
        try:
            subprocess.Popen(
                ["xdg-open", self._auth_url],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
            self._log_line(f"opened {self._auth_url} in browser")
        except OSError as e:
            self._log_line(f"xdg-open failed: {e}")


__all__ = ["MeshJoinPage"]
