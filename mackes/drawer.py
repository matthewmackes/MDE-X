"""Mackes Notification Drawer — right-side slide-in window (v2.2.0+).

Replaces three previous surfaces in a single unified drawer:

  * Conky HUD       (mackes/conky_hud.py — DELETED)
  * Tray icon       (mackes/tray.py     — DELETED)
  * Mini popover    (mackes.app --popover — DELETED)

Triggered by Super+M (xfce4-keyboard-shortcuts custom binding) or by
the mackes-drawer xfce4-panel plugin (C, in
data/panel-plugins/mackes-drawer/). Both spawn `mackes --drawer`;
this module's `toggle()` either opens the drawer (sliding in from the
right edge) or closes an already-open one.

Design source: docs/design/v2.2.0-notification-drawer/

Sections (top to bottom) — 1.0.7 wiring pass replaced every mocked
data source with a live probe. Sections from the original 2.2.0 spec
that depended on subsystems not yet implemented (Drift / Shared
storage / Daemons grid / Footer-power) were removed rather than
shown with placeholder data.

  1. Header           — brand · version · live active preset · admin badge
  2. Quick toggles    — Mesh (tailscale) · Bluetooth (bluetoothctl) ·
                        Do Not Disturb (xfconf-query notifyd) ·
                        Caffeine (xfce4-power-manager presentation-mode)
  3. Sliders          — Volume (pactl @DEFAULT_SINK@) ·
                        Brightness (/usr/local/bin/mackes-brightness)
  4. Mesh             — hub url + peer list (tailscale status --json)
  5. Fleet            — node grid populated from tailscale peers
  6. Services         — UNREAD · PLAYING (MPRIS DBus) · REMOTE (`who -u`)
  7. Notifications    — list with dismiss + clear-all
  8. Battery          — bar · status (/sys/class/power_supply/BAT*)
  9. Hardware         — CPU/RAM/load/clock (/proc + getloadavg)

The drawer also writes ~/.cache/mackes/drawer-state.json on open/close
and on every refresh tick — the C panel plugin reads this for the
pill's notification count + battery %.
"""
from __future__ import annotations

import json
import os
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GLib, Gtk  # noqa: E402


DRAWER_WIDTH = 420
PANEL_HEIGHT = 40
TICK_MS = 1500
STATE_PATH = Path(GLib.get_user_cache_dir()) / "mackes" / "drawer-state.json"

# Carbon-90 token palette (matches the design source)
T_BG_90      = "#262626"
T_BG_95      = "#1a1a1a"
T_BG_85      = "#2f2f2f"
T_BG_80      = "#393939"
T_TEXT       = "#f4f4f4"
T_HELPER     = "#a8a8a8"
T_HELPER_DIM = "#6f6f6f"
T_SUCCESS    = "#42be65"
T_WARNING    = "#f1c21b"
T_ERROR      = "#fa4d56"
T_ACCENT     = "#4589ff"


_CSS = f"""
.mackes-drawer {{
    background-color: {T_BG_90};
    color: {T_TEXT};
}}
.mackes-drawer-stripe {{
    background-color: {T_ACCENT};
    min-width: 3px;
}}
.mackes-drawer-sect {{
    padding: 14px 18px;
}}
.mackes-drawer-rule {{
    background-color: {T_BG_80};
    min-height: 1px;
}}
.mackes-drawer-sect-label {{
    color: {T_TEXT};
    font-family: "Red Hat Display", "IBM Plex Sans", sans-serif;
    font-weight: 700;
    font-size: 10px;
}}
.mackes-drawer-meta {{
    color: {T_HELPER};
    font-family: "JetBrains Mono", "Red Hat Mono", monospace;
    font-size: 11px;
}}
.mackes-drawer-brand {{
    font-family: "Red Hat Display", "IBM Plex Sans", sans-serif;
    font-weight: 700;
    font-size: 18px;
}}
.mackes-drawer-chip {{
    background-color: {T_BG_95};
    border: 1px solid {T_BG_80};
    border-radius: 4px;
    padding: 9px 11px;
    color: {T_HELPER};
}}
.mackes-drawer-chip-on {{
    background-color: alpha({T_ACCENT}, 0.10);
    border-color: {T_ACCENT};
    color: {T_TEXT};
}}
.mackes-drawer-chip-label {{
    color: {T_TEXT};
    font-weight: 500;
}}
.mackes-drawer-chip-status {{
    color: {T_HELPER};
    font-family: "JetBrains Mono", monospace;
    font-size: 10px;
}}
.mackes-drawer-bar trough {{
    background-color: {T_BG_80};
    min-height: 6px;
    border-radius: 1px;
}}
.mackes-drawer-bar progress {{
    background-color: {T_ACCENT};
    border-radius: 1px;
}}
.mackes-drawer-bar.warning progress {{ background-color: {T_WARNING}; }}
.mackes-drawer-bar.error progress {{ background-color: {T_ERROR}; }}
.mackes-drawer-bar.success progress {{ background-color: {T_SUCCESS}; }}
.mackes-drawer-fleet-cell {{
    background-color: {T_BG_95};
    border: 1px solid {T_BG_80};
    border-radius: 3px;
    padding: 8px 10px;
}}
.mackes-drawer-notif {{
    background-color: {T_BG_95};
    border-left: 2px solid {T_ACCENT};
    padding: 10px 12px;
}}
.mackes-drawer-notif.warn {{ border-left-color: {T_WARNING}; }}
.mackes-drawer-notif.crit {{ border-left-color: {T_ERROR}; }}
.mackes-drawer-notif-title {{
    color: {T_TEXT};
    font-weight: 600;
    font-size: 12px;
}}
.mackes-drawer-notif-body {{
    color: {T_HELPER};
    font-size: 11px;
}}
.mackes-drawer-dim {{ color: {T_HELPER}; }}
.mackes-drawer-dim-2 {{ color: {T_HELPER_DIM}; }}
.mackes-drawer-success {{ color: {T_SUCCESS}; }}
.mackes-drawer-warning {{ color: {T_WARNING}; }}
.mackes-drawer-error {{ color: {T_ERROR}; }}
.mackes-drawer-accent {{ color: {T_ACCENT}; }}
.mackes-drawer-foot-btn {{
    background-color: transparent;
    border: 1px solid {T_BG_80};
    color: {T_TEXT};
    padding: 4px 12px;
    border-radius: 3px;
}}
.mackes-drawer-mono {{
    font-family: "JetBrains Mono", "Red Hat Mono", monospace;
}}
"""


def _install_css() -> None:
    """Install drawer-specific CSS once per process."""
    provider = Gtk.CssProvider()
    provider.load_from_data(_CSS.encode("utf-8"))
    screen = Gdk.Screen.get_default()
    if screen is not None:
        Gtk.StyleContext.add_provider_for_screen(
            screen, provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION + 10,
        )


# ---------------------------------------------------------------------------
# Live-data probes — all best-effort, all fail-quietly
# ---------------------------------------------------------------------------


@dataclass
class LiveState:
    """The per-tick snapshot the drawer renders against."""
    time_str:        str = "--:--"
    date_str:        str = ""
    notif_count:     int = 0
    battery_pct:     int = 0
    battery_state:   str = "discharging"
    cpu_pct:         int = 0
    ram_pct:         int = 0
    load_avg:        tuple[float, float, float] = (0.0, 0.0, 0.0)
    mesh_peers:      list = field(default_factory=list)
    mesh_hub:        str = ""
    fleet_nodes:     list = field(default_factory=list)
    notifications:   list = field(default_factory=list)
    # 1.0.7 wiring pass — replaces the prior mock data sources
    volume_pct:      int = 50
    audio_muted:     bool = False
    brightness_pct:  int = 80
    bt_powered:      bool = False
    bt_paired:       int = 0
    dnd_on:          bool = False
    caffeine_on:     bool = False
    active_preset:   str = "unknown"
    is_admin:        bool = False
    playing_count:   int = 0
    remote_sessions: int = 0


def _read_battery() -> tuple[int, str]:
    """Best-effort battery snapshot via /sys/class/power_supply/."""
    try:
        base = Path("/sys/class/power_supply")
        for entry in base.iterdir():
            if entry.name.startswith("BAT"):
                cap = (entry / "capacity").read_text().strip()
                status = (entry / "status").read_text().strip().lower()
                return int(cap), status
    except (OSError, ValueError):
        pass
    return 0, ""


def _read_cpu_ram() -> tuple[int, int]:
    """Best-effort CPU + RAM percent."""
    cpu_pct, ram_pct = 0, 0
    try:
        with open("/proc/stat") as f:
            line = f.readline()
        parts = line.split()
        if parts[0] == "cpu":
            user, nice, system, idle = (int(parts[i]) for i in range(1, 5))
            total = user + nice + system + idle
            busy = user + nice + system
            cpu_pct = int(100 * busy / total) if total else 0
    except (OSError, ValueError):
        pass
    try:
        with open("/proc/meminfo") as f:
            mem = {}
            for line in f:
                k, _, rest = line.partition(":")
                v = rest.strip().split()
                if v:
                    mem[k] = int(v[0])
        total = mem.get("MemTotal", 0)
        avail = mem.get("MemAvailable", 0)
        if total:
            ram_pct = int(100 * (total - avail) / total)
    except (OSError, ValueError):
        pass
    return cpu_pct, ram_pct


def _read_load_avg() -> tuple[float, float, float]:
    try:
        a, b, c = os.getloadavg()
        return a, b, c
    except OSError:
        return 0.0, 0.0, 0.0


def _read_mesh() -> tuple[list, str]:
    """Mesh peers from `tailscale status --json`, best-effort."""
    try:
        from mackes.mesh_vpn import tailscale_status
        s = tailscale_status()
        return s.get("peers") or [], s.get("mesh_ip") or ""
    except Exception:  # noqa: BLE001
        return [], ""


def _run_cmd(argv: list[str], timeout: float = 2.0) -> tuple[int, str]:
    """Spawn a short-lived subprocess and return (returncode, stdout).
    All drawer probes are fire-and-forget; failures degrade silently."""
    try:
        proc = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        return proc.returncode, proc.stdout
    except (OSError, subprocess.TimeoutExpired):
        return -1, ""


def _audio_volume() -> tuple[int, bool]:
    """Current sink volume (0–100) and mute state from pactl. Returns
    (50, False) on probe failure so the slider has a sane default."""
    rc, out = _run_cmd(["pactl", "get-sink-volume", "@DEFAULT_SINK@"])
    pct = 50
    if rc == 0:
        # Format: "Volume: front-left: 49151 /  75% / -7.50 dB, ..."
        import re
        m = re.search(r"(\d+)\s*%", out)
        if m:
            pct = int(m.group(1))
    rc2, out2 = _run_cmd(["pactl", "get-sink-mute", "@DEFAULT_SINK@"])
    muted = rc2 == 0 and "yes" in out2.lower()
    return pct, muted


def _audio_set_volume(pct: int) -> None:
    pct = max(0, min(150, pct))
    _run_cmd(["pactl", "set-sink-volume", "@DEFAULT_SINK@", f"{pct}%"])


def _audio_toggle_mute() -> None:
    _run_cmd(["pactl", "set-sink-mute", "@DEFAULT_SINK@", "toggle"])


def _brightness() -> int:
    """Current brightness (0–100) via the mackes-brightness helper.
    Returns 80 on probe failure so the slider isn't stuck at zero."""
    rc, out = _run_cmd(["/usr/local/bin/mackes-brightness", "get"])
    if rc == 0:
        try:
            return int(out.strip())
        except ValueError:
            pass
    return 80


def _brightness_set(pct: int) -> None:
    pct = max(1, min(100, pct))
    _run_cmd(["/usr/local/bin/mackes-brightness", "set", str(pct)])


def _bluetooth_state() -> tuple[bool, int]:
    """Return (powered, paired_count). Powered=False also when adapter
    is missing — the chip ships either way."""
    rc, out = _run_cmd(["bluetoothctl", "show"])
    if rc != 0:
        return False, 0
    powered = False
    for line in out.splitlines():
        if line.strip().startswith("Powered:"):
            powered = "yes" in line.lower()
            break
    rc2, out2 = _run_cmd(["bluetoothctl", "paired-devices"])
    paired = len([l for l in out2.splitlines() if l.strip().startswith("Device")]) if rc2 == 0 else 0
    return powered, paired


def _bluetooth_toggle() -> None:
    powered, _ = _bluetooth_state()
    _run_cmd(["bluetoothctl", "power", "off" if powered else "on"])


def _dnd_state() -> bool:
    """Do-Not-Disturb — v2.0.0 Phase F.9 reads the MDE flag file at
    `$XDG_CACHE_HOME/mde/notifications-dnd`. The notifications_server
    worker (Phase B.10) honors the same file."""
    from mackes.mde_settings_bridge import sidecar_path
    return sidecar_path("notifications-dnd").exists()


def _dnd_toggle() -> None:
    """Flip DND on/off by writing or removing the flag file."""
    from mackes.mde_settings_bridge import sidecar_path
    path = sidecar_path("notifications-dnd")
    if path.exists():
        try:
            path.unlink()
        except OSError:
            pass
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        try:
            path.write_text("")
        except OSError:
            pass


def _caffeine_state() -> bool:
    """Caffeine — v2.0.0 Phase F.9 reads the MDE flag file at
    `$XDG_CACHE_HOME/mde/power-caffeine` (written by the
    PowerPresentationMode applier, Phase C.4). mde-session inhibits
    idle/lock via swayidle drop-in when the file is present."""
    from mackes.mde_settings_bridge import sidecar_path
    return sidecar_path("power-caffeine").exists()


def _caffeine_toggle() -> None:
    """Flip caffeine on/off by writing or removing the flag file."""
    from mackes.mde_settings_bridge import sidecar_path
    path = sidecar_path("power-caffeine")
    if path.exists():
        try:
            path.unlink()
        except OSError:
            pass
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        try:
            path.write_text("")
        except OSError:
            pass


def _mesh_toggle(on: bool) -> None:
    """`tailscale up` (with mesh_perf flags applied via mesh_vpn) or
    `tailscale down`. The drawer doesn't wait for completion — the
    button shows the new state on the next tick."""
    try:
        if on:
            from mackes.mesh_vpn import tailscale_up_via_headscale
            tailscale_up_via_headscale()
        else:
            _run_cmd(["tailscale", "down"])
    except Exception:  # noqa: BLE001
        pass


def _active_preset() -> str:
    """Active preset name from MackesState. Falls back to 'unknown' if
    state.json is missing or unreadable."""
    try:
        from mackes.state import MackesState
        return (MackesState.load().active_preset or "unknown")
    except Exception:  # noqa: BLE001
        return "unknown"


def _is_admin() -> bool:
    """Show the admin badge when the user is in the `wheel` group
    (Fedora's sudoers default). We don't check the live sudo token —
    membership is what unlocks the Workbench's privileged panels."""
    try:
        import grp
        return os.environ.get("USER", "") in grp.getgrnam("wheel").gr_mem
    except (KeyError, OSError):
        return os.geteuid() == 0


def _mpris_playing() -> int:
    """Number of MPRIS players currently in PlaybackStatus=Playing.
    Iterates the well-known names on the session bus via gdbus — cheap
    enough for a 1.5 s drawer tick (~3 short subprocess calls)."""
    rc, out = _run_cmd([
        "gdbus", "call", "--session",
        "--dest", "org.freedesktop.DBus",
        "--object-path", "/org/freedesktop/DBus",
        "--method", "org.freedesktop.DBus.ListNames",
    ])
    if rc != 0:
        return 0
    import re
    names = re.findall(r"'(org\.mpris\.MediaPlayer2\.[^']+)'", out)
    playing = 0
    for name in names:
        rc2, out2 = _run_cmd([
            "gdbus", "call", "--session",
            "--dest", name,
            "--object-path", "/org/mpris/MediaPlayer2",
            "--method", "org.freedesktop.DBus.Properties.Get",
            "org.mpris.MediaPlayer2.Player", "PlaybackStatus",
        ])
        if rc2 == 0 and "Playing" in out2:
            playing += 1
    return playing


def _remote_sessions() -> int:
    """SSH / remote sessions: `who -u` lines whose tty isn't a local
    console. Local users (tty :0, seat0) are excluded; SSH connections
    show as pts/N with the source host in parens."""
    rc, out = _run_cmd(["who", "-u"])
    if rc != 0:
        return 0
    count = 0
    for line in out.splitlines():
        if not line.strip():
            continue
        # Local console lines contain ":0" or "seat0"
        if ":0" in line or "seat0" in line or "tty1" in line:
            continue
        # SSH lines mention pts/ and have a host in parens or pure IP
        if "pts/" in line:
            count += 1
    return count


def collect_state() -> LiveState:
    """One snapshot of everything the drawer renders."""
    now = time.localtime()
    bp, bs = _read_battery()
    cp, rp = _read_cpu_ram()
    peers, hub = _read_mesh()
    vol, muted = _audio_volume()
    bt_on, bt_paired = _bluetooth_state()
    notifs = _load_pending_notifications()
    return LiveState(
        time_str=time.strftime("%H:%M", now),
        date_str=time.strftime("%a %b %e", now),
        notif_count=len(notifs),
        battery_pct=bp,
        battery_state=bs,
        cpu_pct=cp,
        ram_pct=rp,
        load_avg=_read_load_avg(),
        mesh_peers=peers,
        mesh_hub=hub,
        # Fleet derives from the same tailscale snapshot — re-using the
        # peers list (1.0.7) replaces the prior _load_fleet_nodes() mock
        # fallback that wrote hardcoded helios/oracle/forge/cinder.
        fleet_nodes=peers,
        notifications=notifs,
        volume_pct=vol,
        audio_muted=muted,
        brightness_pct=_brightness(),
        bt_powered=bt_on,
        bt_paired=bt_paired,
        dnd_on=_dnd_state(),
        caffeine_on=_caffeine_state(),
        active_preset=_active_preset(),
        is_admin=_is_admin(),
        playing_count=_mpris_playing(),
        remote_sessions=_remote_sessions(),
    )


def _cache_root() -> Path:
    """Resolve the cache root, honoring `$XDG_CACHE_HOME` explicitly
    so tests can redirect via environment variable (GLib's resolver
    memoizes the first call and ignores later env changes)."""
    import os
    env = os.environ.get("XDG_CACHE_HOME")
    if env:
        return Path(env)
    return Path(GLib.get_user_cache_dir())


def _load_pending_notifications() -> list:
    """Read from ~/.cache/mackes/notifications.json (written by other
    Mackes services as they emit events). Empty list when there's
    nothing to surface.

    KDC2-5.10 (v2.1+) — the Phase 13.4 KDE Connect mirrored-
    notifications merge is retired. Phone notifications now flow
    directly into the local notification stack via mako (the
    Wayland-native notification daemon) over the
    `dev.mackes.MDE.Connect` D-Bus signal surface, and the
    Iced applet at `crates/mde-applets/notifications/` renders
    the phone glyph badge (KDC2-5.11). The drawer's only
    notification source is now the local
    `notifications.json` snapshot.
    """
    notes = []
    cache = _cache_root()
    path = cache / "mackes" / "notifications.json"
    if path.is_file():
        try:
            notes = list(json.loads(path.read_text(encoding="utf-8")))
        except (OSError, json.JSONDecodeError):
            notes = []
    return notes


def write_state_file(state: LiveState, *, drawer_open: bool) -> None:
    """Write the C panel plugin's state file."""
    try:
        STATE_PATH.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            "time":        state.time_str,
            "date":        state.date_str,
            "notif_count": state.notif_count,
            "battery_pct": state.battery_pct,
            "drawer_open": drawer_open,
        }
        STATE_PATH.write_text(json.dumps(payload), encoding="utf-8")
    except OSError:
        pass


# ---------------------------------------------------------------------------
# UI primitives
# ---------------------------------------------------------------------------


def _label(text: str, *, classes: tuple[str, ...] = (),
           markup: bool = False, xalign: float = 0.0) -> Gtk.Label:
    lbl = Gtk.Label()
    if markup:
        lbl.set_markup(text)
    else:
        lbl.set_text(text)
    lbl.set_xalign(xalign)
    ctx = lbl.get_style_context()
    for c in classes:
        ctx.add_class(c)
    return lbl


def _section(label_text: str, *, right_text: str = "",
             right_classes: tuple[str, ...] = ("mackes-drawer-meta",)
             ) -> tuple[Gtk.Box, Gtk.Box]:
    """Return (outer, body) — outer wraps the section header + body box."""
    outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
    outer.get_style_context().add_class("mackes-drawer-sect")

    head = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    head.pack_start(_label(label_text.upper(),
                            classes=("mackes-drawer-sect-label",)),
                     True, True, 0)
    if right_text:
        head.pack_end(_label(right_text, classes=right_classes,
                              xalign=1.0), False, False, 0)
    outer.pack_start(head, False, False, 0)

    body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
    outer.pack_start(body, False, False, 0)
    return outer, body


def _rule() -> Gtk.Widget:
    sep = Gtk.Box()
    sep.get_style_context().add_class("mackes-drawer-rule")
    sep.set_margin_start(18)
    sep.set_margin_end(18)
    return sep


def _bar(percent: int, *, classes: tuple[str, ...] = ()) -> Gtk.ProgressBar:
    pb = Gtk.ProgressBar()
    pb.set_fraction(max(0.0, min(1.0, percent / 100)))
    pb.set_valign(Gtk.Align.CENTER)
    pb.get_style_context().add_class("mackes-drawer-bar")
    for c in classes:
        pb.get_style_context().add_class(c)
    return pb


# ---------------------------------------------------------------------------
# Section builders
# ---------------------------------------------------------------------------


def _header(state: LiveState, on_close) -> Gtk.Widget:
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
    box.set_margin_top(16); box.set_margin_bottom(14)
    box.set_margin_start(18); box.set_margin_end(18)

    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
    row.pack_start(_label("▤", classes=("mackes-drawer-accent",),
                           markup=False), False, False, 0)
    row.pack_start(_label("<b>Mackes</b> <span color=\"#a8a8a8\">Shell</span>",
                           classes=("mackes-drawer-brand",),
                           markup=True), True, True, 0)
    close = Gtk.Button(label="✕")
    close.set_relief(Gtk.ReliefStyle.NONE)
    close.connect("clicked", lambda *_: on_close())
    close.set_tooltip_text("Close (Esc)")
    _ax = close.get_accessible()
    if _ax is not None:
        _ax.set_name("Close the notification drawer")
    row.pack_end(close, False, False, 0)
    box.pack_start(row, False, False, 0)

    try:
        from mackes import __version__
    except ImportError:
        __version__ = "?"
    meta = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
    meta.set_margin_start(28)
    meta.pack_start(_label(f"v{__version__}",
                            classes=("mackes-drawer-meta",)),
                     False, False, 0)
    meta.pack_start(_label("·", classes=("mackes-drawer-dim-2",)),
                     False, False, 0)
    # Active preset (live from MackesState — replaces the prior
    # hardcoded "hashbang" label).
    meta.pack_start(_label(state.active_preset,
                            classes=("mackes-drawer-accent",
                                     "mackes-drawer-meta")),
                     False, False, 0)
    if state.is_admin:
        meta.pack_start(_label("·", classes=("mackes-drawer-dim-2",)),
                         False, False, 0)
        meta.pack_start(_label("admin",
                                classes=("mackes-drawer-success",
                                         "mackes-drawer-meta")),
                         False, False, 0)
    box.pack_start(meta, False, False, 0)
    return box


def _quick_toggles_section(state: LiveState) -> Gtk.Widget:
    outer, body = _section("Quick toggles")
    grid = Gtk.Grid(column_spacing=6, row_spacing=6,
                     column_homogeneous=True)
    # Each chip: (label, status text, on?, click handler).
    # Click handlers run synchronously then the next 1.5 s tick will
    # repaint with the new state. No await needed.
    mesh_on = bool(state.mesh_hub)
    chips: tuple = (
        ("Mesh",
         f"tailscale {state.mesh_hub}" if mesh_on else "off",
         mesh_on,
         lambda: _mesh_toggle(not mesh_on)),
        ("Bluetooth",
         (f"{state.bt_paired} paired" if state.bt_powered else "off"),
         state.bt_powered,
         _bluetooth_toggle),
        ("Do not disturb",
         "on" if state.dnd_on else "off",
         state.dnd_on,
         _dnd_toggle),
        ("Caffeine",
         "on" if state.caffeine_on else "off",
         state.caffeine_on,
         _caffeine_toggle),
    )
    for i, (label, status, on, handler) in enumerate(chips):
        chip = Gtk.EventBox()
        chip.set_visible_window(True)
        inner = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        chip.get_style_context().add_class("mackes-drawer-chip")
        if on:
            chip.get_style_context().add_class("mackes-drawer-chip-on")
        inner.pack_start(_label(label,
                                 classes=("mackes-drawer-chip-label",)),
                          False, False, 0)
        inner.pack_start(_label(status,
                                 classes=("mackes-drawer-chip-status",)),
                          False, False, 0)
        chip.add(inner)
        chip.set_tooltip_text(f"{label}: {status} — click to toggle")
        _ax = chip.get_accessible()
        if _ax is not None:
            state_word = "on" if on else "off"
            _ax.set_name(f"{label} toggle, currently {state_word} ({status})")
        # Capture handler in closure (Python default-arg trick to avoid
        # the classic "last handler wins" loop-binding bug).
        chip.connect("button-press-event",
                     lambda _w, _e, h=handler: (h(), False)[1])
        grid.attach(chip, i % 2, i // 2, 1, 1)
    body.pack_start(grid, False, False, 0)

    # Sliders — Volume (pactl) + Brightness (mackes-brightness).
    # We tag a flag on the Adjustment so the value-changed handler only
    # fires for user drag, not for the programmatic set we do at build
    # time. Without the flag, building the slider would immediately
    # write the displayed value back to the system.
    sliders = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
    sliders.set_margin_top(10)
    for name, value, setter in (
        ("Volume", state.volume_pct, _audio_set_volume),
        ("Brightness", state.brightness_pct, _brightness_set),
    ):
        srow = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        srow.pack_start(_label(name, classes=("mackes-drawer-dim",)),
                         False, False, 0)
        adj = Gtk.Adjustment(value=value, lower=0, upper=100,
                              step_increment=5, page_increment=10)
        scale = Gtk.Scale(orientation=Gtk.Orientation.HORIZONTAL,
                           adjustment=adj)
        scale.set_draw_value(False)
        scale.set_hexpand(True)
        scale.set_tooltip_text(f"{name} — drag to adjust (0–100%)")
        _ax_scale = scale.get_accessible()
        if _ax_scale is not None:
            _ax_scale.set_name(f"{name} level slider")
        # Mute indicator on the Volume row: clicking the row label
        # toggles mute. Saves screen real-estate vs. a separate icon.
        if name == "Volume" and state.audio_muted:
            scale.set_sensitive(False)
        value_label = _label(f"{value}%",
                              classes=("mackes-drawer-meta",
                                       "mackes-drawer-mono"),
                              xalign=1.0)

        def on_value_changed(s, _setter=setter, _vl=value_label):
            pct = int(s.get_value())
            _vl.set_text(f"{pct}%")
            _setter(pct)
        # Debounce: only write on button-release (drag end) not every
        # pixel of motion, otherwise pactl gets hammered.
        scale.connect("button-release-event",
                       lambda s, _e, cb=on_value_changed: (cb(s), False)[1])
        scale.connect("scroll-event",
                       lambda s, _e, cb=on_value_changed: (cb(s), False)[1])
        srow.pack_start(scale, True, True, 0)
        srow.pack_end(value_label, False, False, 0)
        sliders.pack_start(srow, False, False, 0)
    body.pack_start(sliders, False, False, 0)
    return outer


def _mesh_section(state: LiveState) -> Gtk.Widget:
    outer, body = _section(
        "Mesh",
        right_text=(f"{len(state.mesh_peers)} peers · "
                    f"{state.mesh_hub or '—'}"),
    )
    hub_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    hub_row.pack_start(_label("hub", classes=("mackes-drawer-dim",
                                                "mackes-drawer-mono")),
                        False, False, 0)
    hub_row.pack_start(_label(state.mesh_hub or "not joined",
                                classes=("mackes-drawer-mono",)),
                        False, False, 0)
    body.pack_start(hub_row, False, False, 0)

    if state.mesh_peers:
        for p in state.mesh_peers[:6]:
            row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
            online = bool(p.get("online"))
            dot_class = ("mackes-drawer-success" if online
                          else "mackes-drawer-warning")
            row.pack_start(_label("●", classes=(dot_class,)),
                            False, False, 0)
            row.pack_start(_label(p.get("name") or "?",
                                    classes=("mackes-drawer-mono",)),
                            True, True, 0)
            rtt = p.get("rtt_ms")
            row.pack_end(_label(f"{rtt}ms" if rtt else "—",
                                  classes=("mackes-drawer-dim",
                                           "mackes-drawer-mono"),
                                  xalign=1.0), False, False, 0)
            body.pack_start(row, False, False, 0)
    else:
        body.pack_start(_label("No peers reachable",
                                classes=("mackes-drawer-dim-2",
                                         "mackes-drawer-mono")),
                         False, False, 0)
    return outer


def _fleet_section(state: LiveState) -> Gtk.Widget:
    """Fleet view — live tailscale peers, up to 4 visible in a 2×2 grid.
    A peer's status is one of:
      - "ok" (green): peer is Online
      - "idle" (grey): peer is in the tailnet but Offline
    Tailscale's status JSON doesn't expose a "sync" intermediate state,
    so the old mock fallback's three colors collapse to two real ones."""
    nodes = state.fleet_nodes  # already comes from tailscale_status()'s peers
    reachable = sum(1 for n in nodes if n.get("online"))
    outer, body = _section(
        "Fleet",
        right_text=(
            f"{reachable} / {len(nodes)} reachable"
            if nodes else "no peers"
        ),
    )

    if not nodes:
        body.pack_start(
            _label(
                "Join a mesh from Workbench → Network → Mesh VPN to "
                "populate this list.",
                classes=("mackes-drawer-dim-2", "mackes-drawer-mono"),
            ),
            False, False, 0,
        )
        return outer

    grid = Gtk.Grid(column_spacing=6, row_spacing=6,
                     column_homogeneous=True)
    for i, n in enumerate(nodes[:4]):
        cell = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        cell.get_style_context().add_class("mackes-drawer-fleet-cell")
        top = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        online = bool(n.get("online"))
        dot_class = ("mackes-drawer-success" if online
                      else "mackes-drawer-dim-2")
        top.pack_start(_label("●", classes=(dot_class,)),
                        False, False, 0)
        top.pack_start(_label(n.get("name") or "?"), True, True, 0)
        cell.pack_start(top, False, False, 0)
        bot = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        bot.pack_start(_label(n.get("mesh_ip", ""),
                                classes=("mackes-drawer-dim",
                                         "mackes-drawer-mono")),
                        True, True, 0)
        status = "OK" if online else "IDLE"
        bot.pack_end(_label(status,
                              classes=(dot_class, "mackes-drawer-mono"),
                              xalign=1.0), False, False, 0)
        cell.pack_start(bot, False, False, 0)
        grid.attach(cell, i % 2, i // 2, 1, 1)
    body.pack_start(grid, False, False, 0)

    if len(nodes) > 4:
        body.pack_start(
            _label(
                f"+{len(nodes) - 4} more — open Workbench → Network "
                f"→ Mesh Health to see all",
                classes=("mackes-drawer-dim-2", "mackes-drawer-mono"),
            ),
            False, False, 0,
        )
    return outer


def _services_section(state: LiveState) -> Gtk.Widget:
    outer, body = _section("Services")
    grid = Gtk.Grid(column_spacing=6, column_homogeneous=True)
    for label, count, glyph in (
        ("UNREAD",  str(state.notif_count),     "◐"),
        ("PLAYING", str(state.playing_count),   "♫"),
        ("REMOTE",  str(state.remote_sessions), "↪"),
    ):
        cell = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        cell.get_style_context().add_class("mackes-drawer-fleet-cell")
        cell.pack_start(_label(glyph, classes=("mackes-drawer-accent",)),
                         False, False, 0)
        text = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        text.pack_start(_label(count, classes=("mackes-drawer-brand",)),
                         False, False, 0)
        text.pack_start(_label(label, classes=("mackes-drawer-meta",
                                                "mackes-drawer-mono")),
                         False, False, 0)
        cell.pack_start(text, False, False, 0)
        grid.attach(cell, len(grid.get_children()), 0, 1, 1)
    body.pack_start(grid, False, False, 0)
    return outer


def _notifications_section(state: LiveState, on_clear) -> Gtk.Widget:
    if not state.notifications:
        outer, body = _section("Notifications")
        body.pack_start(_label("All clear.",
                                classes=("mackes-drawer-dim-2",
                                         "mackes-drawer-mono")),
                         False, False, 0)
        return outer
    outer, body = _section("Notifications")
    # Re-add a CLEAR-ALL action in the right-end of the header — the
    # _section helper doesn't expose this, so we patch the head row.
    head = outer.get_children()[0]
    clear_btn = Gtk.Button(label="clear all")
    clear_btn.set_relief(Gtk.ReliefStyle.NONE)
    clear_btn.connect("clicked", lambda *_: on_clear())
    clear_btn.set_tooltip_text("Clear every pending notification")
    _ax_clear = clear_btn.get_accessible()
    if _ax_clear is not None:
        _ax_clear.set_name("Clear all pending notifications")
    head.pack_end(clear_btn, False, False, 0)

    for n in state.notifications[:8]:
        u = n.get("urgency", "info")
        nbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        nbox.get_style_context().add_class("mackes-drawer-notif")
        if u in ("warn", "crit"):
            nbox.get_style_context().add_class(u)
        # KDC2-5.10 (v2.1+) — the Phase 13.4 phone-origin badge
        # is retired here. Phone notifications now arrive through
        # mako (Wayland-native daemon) and the Iced applet at
        # `crates/mde-applets/notifications/` paints the badge
        # (KDC2-5.11). The drawer renders the local stack
        # uniformly without a phone-specific branch.
        app_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        app_row.pack_start(_label(n.get("app", "system"),
                                    classes=("mackes-drawer-meta",
                                             "mackes-drawer-mono")),
                            True, True, 0)
        app_row.pack_end(_label(n.get("when", "now"),
                                  classes=("mackes-drawer-meta",
                                           "mackes-drawer-mono"),
                                  xalign=1.0), False, False, 0)
        nbox.pack_start(app_row, False, False, 0)
        nbox.pack_start(_label(n.get("title", "—"),
                                classes=("mackes-drawer-notif-title",)),
                         False, False, 0)
        if n.get("body"):
            nbox.pack_start(_label(n["body"],
                                    classes=("mackes-drawer-notif-body",)),
                             False, False, 0)
        body.pack_start(nbox, False, False, 0)
    return outer


def _battery_section(state: LiveState) -> Gtk.Widget:
    pct = state.battery_pct
    cls = ("error" if pct < 15 else "warning" if pct < 30
            else "success" if pct else "")
    outer, body = _section("Battery",
                            right_text=f"{pct}%" if pct else "—")
    bar = _bar(pct, classes=(cls,) if cls else ())
    bar.set_size_request(220, 6)
    bar.set_hexpand(True)
    body.pack_start(bar, False, False, 0)

    meta = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
    meta.pack_start(_label(state.battery_state or "—",
                            classes=("mackes-drawer-dim",
                                     "mackes-drawer-mono")),
                     True, True, 0)
    body.pack_start(meta, False, False, 0)
    return outer


def _hardware_section(state: LiveState) -> Gtk.Widget:
    outer, body = _section("Hardware",
                            right_text=os.uname().nodename)
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    row.pack_start(_label("CPU", classes=("mackes-drawer-dim",
                                            "mackes-drawer-mono")),
                    False, False, 0)
    row.pack_start(_label(f"{state.cpu_pct}%",
                            classes=("mackes-drawer-accent",
                                     "mackes-drawer-mono"),
                            xalign=1.0), False, False, 0)
    bar_cpu = _bar(state.cpu_pct)
    bar_cpu.set_size_request(80, 6)
    row.pack_start(bar_cpu, True, True, 0)
    row.pack_start(_label("RAM", classes=("mackes-drawer-dim",
                                            "mackes-drawer-mono")),
                    False, False, 0)
    row.pack_start(_label(f"{state.ram_pct}%",
                            classes=("mackes-drawer-accent",
                                     "mackes-drawer-mono"),
                            xalign=1.0), False, False, 0)
    bar_ram = _bar(state.ram_pct)
    bar_ram.set_size_request(80, 6)
    row.pack_start(bar_ram, True, True, 0)
    body.pack_start(row, False, False, 0)

    load_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    load_row.pack_start(_label("load",
                                  classes=("mackes-drawer-dim",
                                           "mackes-drawer-mono")),
                          False, False, 0)
    load_row.pack_start(_label(
        f"{state.load_avg[0]:.2f}  {state.load_avg[1]:.2f}  {state.load_avg[2]:.2f}",
        classes=("mackes-drawer-mono",)),
                          True, True, 0)
    load_row.pack_end(_label(state.time_str,
                                classes=("mackes-drawer-dim",
                                         "mackes-drawer-mono"),
                                xalign=1.0), False, False, 0)
    body.pack_start(load_row, False, False, 0)
    return outer


# ---------------------------------------------------------------------------
# The window
# ---------------------------------------------------------------------------


class DrawerWindow(Gtk.Window):
    """The slide-in drawer. Toplevel POPUP window anchored to the screen
    edge opposite the panel."""

    _singleton: Optional["DrawerWindow"] = None

    def __init__(self) -> None:
        super().__init__(type=Gtk.WindowType.POPUP)
        _install_css()
        self.set_default_size(DRAWER_WIDTH, 600)
        self.set_decorated(False)
        self.set_skip_taskbar_hint(True)
        self.set_skip_pager_hint(True)
        self.set_keep_above(True)
        self.set_app_paintable(True)
        self.get_style_context().add_class("mackes-drawer")
        self.connect("key-press-event", self._on_key)
        self.connect("focus-out-event", lambda *_: self.close_drawer())

        # Position: right-anchored, full screen height minus panel.
        display = Gdk.Display.get_default()
        mon = display.get_primary_monitor() or display.get_monitor(0)
        geom = mon.get_geometry() if mon else None
        if geom is not None:
            self._screen_h = geom.height
            self._screen_w = geom.width
            self.move(geom.x + geom.width - DRAWER_WIDTH,
                       geom.y)
            self.set_default_size(DRAWER_WIDTH,
                                    max(400, geom.height - PANEL_HEIGHT))
        else:
            self._screen_h, self._screen_w = 900, 1600

        self._tick_id: Optional[int] = None
        self._body_box: Optional[Gtk.Box] = None
        self._rebuild()

    def _rebuild(self) -> None:
        for c in self.get_children():
            self.remove(c)
        outer = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        outer.get_style_context().add_class("mackes-drawer")

        stripe = Gtk.Box()
        stripe.get_style_context().add_class("mackes-drawer-stripe")
        outer.pack_start(stripe, False, False, 0)

        scroll = Gtk.ScrolledWindow()
        scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroll.set_hexpand(True); scroll.set_vexpand(True)
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._body_box = body
        scroll.add(body)
        outer.pack_start(scroll, True, True, 0)

        self.add(outer)
        self._refresh_body(collect_state())

    def _refresh_body(self, state: LiveState) -> None:
        if self._body_box is None:
            return
        for c in self._body_box.get_children():
            self._body_box.remove(c)
        b = self._body_box
        b.pack_start(_header(state, self.close_drawer), False, False, 0)
        b.pack_start(_rule(), False, False, 0)
        b.pack_start(_quick_toggles_section(state), False, False, 0)
        b.pack_start(_rule(), False, False, 0)
        b.pack_start(_mesh_section(state), False, False, 0)
        b.pack_start(_rule(), False, False, 0)
        b.pack_start(_fleet_section(state), False, False, 0)
        b.pack_start(_rule(), False, False, 0)
        b.pack_start(_services_section(state), False, False, 0)
        b.pack_start(_rule(), False, False, 0)
        b.pack_start(_notifications_section(state, self._clear_notifs),
                      False, False, 0)
        b.pack_start(_rule(), False, False, 0)
        b.pack_start(_battery_section(state), False, False, 0)
        b.pack_start(_rule(), False, False, 0)
        b.pack_start(_hardware_section(state), False, False, 0)
        b.show_all()
        write_state_file(state, drawer_open=True)

    def _clear_notifs(self) -> None:
        path = Path(GLib.get_user_cache_dir()) / "mackes" / "notifications.json"
        try:
            path.write_text("[]", encoding="utf-8")
        except OSError:
            pass
        self._refresh_body(collect_state())

    def _on_key(self, _widget, event) -> bool:
        if event.keyval == Gdk.KEY_Escape:
            self.close_drawer()
            return True
        return False

    def _tick(self) -> bool:
        self._refresh_body(collect_state())
        return True

    def open_drawer(self) -> None:
        self.show_all()
        self.present()
        self.grab_focus()
        if self._tick_id is None:
            self._tick_id = GLib.timeout_add(TICK_MS, self._tick)
        write_state_file(collect_state(), drawer_open=True)

    def close_drawer(self) -> None:
        if self._tick_id is not None:
            GLib.source_remove(self._tick_id)
            self._tick_id = None
        write_state_file(collect_state(), drawer_open=False)
        self.hide()


def toggle() -> None:
    """Open the drawer if not visible, close it if it is. Called from
    `mackes-shell --drawer` (which the C panel plugin spawns on click)."""
    inst = DrawerWindow._singleton
    if inst is None:
        inst = DrawerWindow()
        DrawerWindow._singleton = inst
    if inst.get_visible():
        inst.close_drawer()
    else:
        inst.open_drawer()


def main() -> int:
    """`mackes-shell --drawer` entry point — open the drawer + run the
    GTK main loop until the user closes it."""
    toggle()
    Gtk.main()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
