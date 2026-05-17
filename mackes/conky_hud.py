"""Mackes Conky HUD — orchestration + start/stop.

v1.4.0 lock (5-question survey, 2026-05-17):
  Q1  Top-right, 400 × ⅔ screen height          → superseded by v1.6.2 (auto-height)
  Q2  Birthright autostart + Tweaks toggle
  Q3  Tiered refresh (1s system / 30s state / 60s rare)
  Q4  10 blocks: header / mesh / fleet / drift / storage / notifications /
      media / remote / services / hardware       → 6 sections in v1.6.2
                                                    (notif/media/remote merged
                                                     into one Services row)
  Q5  Opaque Carbon Gray 90 + 3px accent left-edge  → now a cairo stroke,
                                                       not a per-line glyph

v1.6.2 changes:
  • Hack Nerd Font for glyphs (Cascadia Code NF was never installed)
  • minimum_height = 0 (auto-size; Q1 lock retired)
  • Density toggle (compact / standard / full) wired through MackesState
  • Per-monitor placement (MackesState.conky_monitor)
  • Wayland detection — start() is a no-op under Wayland (follow-up:
    GTK overlay renderer to replace conky on that path)
  • SIGUSR1 hot reload — restart_with() no longer flashes the desktop
  • Version baked into the config text at render time (no daily Python
    spawn from execi)
  • Cairo left-edge stripe via lua_draw_hook_pre (replaces per-line BAR)
  • timeout 3 wraps every helper invocation
"""
from __future__ import annotations

import os
import shutil
import signal
import subprocess
from pathlib import Path
from typing import Optional


CONFIG_DIR = Path(os.path.expanduser("~/.config/mackes-conky"))
USER_CONFIG = CONFIG_DIR / "mackes.conf"
USER_LUA    = CONFIG_DIR / "mackes-conky.lua"
AUTOSTART_FILE = Path(os.path.expanduser("~/.config/autostart/mackes-conky.desktop"))


# Per-preset accent hexes — match data/css/accents/*.css
_PRESET_ACCENTS = {
    "hashbang": "fa4d56",
    "mackes":   "f1853d",
    "daylight": "f1c21b",
    "vanilla":  "0f62fe",
    "node":     "42be65",
}

DENSITY_VALUES = ("compact", "standard", "full")
DEFAULT_DENSITY = "standard"


def _data_root() -> Optional[Path]:
    """Resolve the installed-or-source data dir."""
    for p in (
        Path("/usr/share/mackes-shell/data/conky"),
        Path(__file__).resolve().parent.parent / "data" / "conky",
    ):
        if p.is_dir():
            return p
    return None


def _template_path() -> Optional[Path]:
    root = _data_root()
    if root is None:
        return None
    p = root / "mackes-conky.conf.template"
    return p if p.is_file() else None


def _lua_template_path() -> Optional[Path]:
    root = _data_root()
    if root is None:
        return None
    p = root / "mackes-conky.lua"
    return p if p.is_file() else None


# ---------------------------------------------------------------------------
# Display introspection (X11 only — Wayland is gated in start())
# ---------------------------------------------------------------------------


def _is_wayland() -> bool:
    return (os.environ.get("XDG_SESSION_TYPE", "").lower() == "wayland"
            or bool(os.environ.get("WAYLAND_DISPLAY")))


def _xrandr_outputs() -> list[dict]:
    """Return [{name, primary, w, h, x, y}, …] for every active output.

    Two sources, in order:
      1. xrandr (preferred — gives exact pixel geometry).
      2. xfconf displays channel (fallback when xrandr isn't installed
         — common on minimal Fedora; xfce4-settings owns this channel).
    """
    out = _xrandr_outputs_from_xrandr()
    if out:
        return out
    return _xrandr_outputs_from_xfconf()


def _xrandr_outputs_from_xrandr() -> list[dict]:
    out: list[dict] = []
    try:
        text = subprocess.check_output(
            ["xrandr", "--current"], text=True, timeout=4,
            stderr=subprocess.DEVNULL,
        )
    except (FileNotFoundError, subprocess.CalledProcessError,
            subprocess.TimeoutExpired):
        return out
    for line in text.splitlines():
        if " connected" not in line:
            continue
        primary = " primary " in line or " connected primary " in line
        name = line.split()[0]
        for token in line.split():
            if "x" in token and "+" in token and "/" not in token:
                try:
                    geom, x, y = token.split("+")
                    w, h = geom.split("x")
                    out.append({
                        "name": name,
                        "primary": primary,
                        "w": int(w), "h": int(h),
                        "x": int(x), "y": int(y),
                    })
                except ValueError:
                    pass
                break
    return out


def _xrandr_outputs_from_xfconf() -> list[dict]:
    """Parse xfconf-query -c displays -l -v for the active profile.

    The xfsettings `displays` channel mirrors the live X output layout:
      /Default/<output>/Active        bool
      /Default/<output>/Primary       bool
      /Default/<output>/Resolution    "WIDTHxHEIGHT"
      /Default/<output>/Position/X    int
      /Default/<output>/Position/Y    int

    Only outputs with Active=true are returned.
    """
    out: list[dict] = []
    if shutil.which("xfconf-query") is None:
        return out
    try:
        text = subprocess.check_output(
            ["xfconf-query", "-c", "displays", "-l", "-v"],
            text=True, timeout=4, stderr=subprocess.DEVNULL,
        )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
        return out
    # Bucket properties by output name (under /Default/<name>/).
    buckets: dict[str, dict] = {}
    for line in text.splitlines():
        parts = line.split(None, 1)
        if len(parts) != 2:
            continue
        key, val = parts[0], parts[1].strip()
        if not key.startswith("/Default/"):
            continue
        rest = key[len("/Default/"):]
        if "/" not in rest:
            continue
        name, sub = rest.split("/", 1)
        buckets.setdefault(name, {})[sub] = val
    for name, props in buckets.items():
        if props.get("Active", "false").lower() != "true":
            continue
        res = props.get("Resolution", "")
        if "x" not in res:
            continue
        try:
            w, h = (int(x) for x in res.split("x", 1))
            x = int(props.get("Position/X", "0"))
            y = int(props.get("Position/Y", "0"))
        except ValueError:
            continue
        out.append({
            "name": name,
            "primary": props.get("Primary", "false").lower() == "true",
            "w": w, "h": h, "x": x, "y": y,
        })
    return out


def _resolve_monitor(name: Optional[str]) -> Optional[dict]:
    """Pick an output by name, falling back to primary, then the first."""
    outputs = _xrandr_outputs()
    if not outputs:
        return None
    if name:
        for o in outputs:
            if o["name"] == name:
                return o
    for o in outputs:
        if o["primary"]:
            return o
    return outputs[0]


def _placement(monitor: Optional[dict]) -> tuple[int, int]:
    """Return (gap_x, gap_y) for the top-right of the target monitor.

    gap_x is distance from the right edge of the target output to the
    right edge of the HUD. With a multi-monitor X11 layout, conky's
    'top_right' aligns to the whole root window's top-right, so we have
    to offset by (root_width - monitor.x - monitor.w) to land on the
    target output.
    """
    gap_y = 48  # below xfce4-panel — unchanged from v1.4.0
    if monitor is None:
        return 24, gap_y
    outputs = _xrandr_outputs()
    if not outputs:
        return 24, gap_y
    root_w = max(o["x"] + o["w"] for o in outputs)
    offset = root_w - (monitor["x"] + monitor["w"])
    return 24 + offset, gap_y


# ---------------------------------------------------------------------------
# Render
# ---------------------------------------------------------------------------


def _mackes_version() -> str:
    try:
        from mackes import __version__
        return __version__
    except Exception:  # noqa: BLE001
        return "?"


def render_config(*, accent_hex: str, density: str,
                  gap_x: int, gap_y: int,
                  version: Optional[str] = None,
                  lua_path: Optional[Path] = None) -> str:
    """Substitute the template's placeholders. Caller decides where to write it."""
    tmpl = _template_path()
    if tmpl is None:
        raise FileNotFoundError("mackes-conky.conf.template not found")
    text = tmpl.read_text(encoding="utf-8")
    return (text
        .replace("{accent_hex}", accent_hex.lstrip("#"))
        .replace("{density}",    density)
        .replace("{gap_x}",      str(int(gap_x)))
        .replace("{gap_y}",      str(int(gap_y)))
        .replace("{version}",    version or _mackes_version())
        .replace("{height_px}",  "0")   # retired Q1 lock
        .replace("{lua_path}",   str(lua_path or USER_LUA)))


def _render_lua(*, accent_hex: str) -> Optional[str]:
    src = _lua_template_path()
    if src is None:
        return None
    return src.read_text(encoding="utf-8").replace(
        "{accent_hex}", accent_hex.lstrip("#"))


def write_config(*, accent: Optional[str] = None,
                 density: Optional[str] = None,
                 monitor: Optional[str] = None,
                 height_px: Optional[int] = None) -> Path:
    """Write ~/.config/mackes-conky/mackes.conf + the cairo stripe Lua.

    height_px is accepted for backwards compat but ignored — v1.6.2
    auto-sizes via minimum_height=0.
    """
    if accent is None:
        accent = _resolve_accent_from_state()
    if density is None:
        density = _resolve_density_from_state()
    if monitor is None:
        monitor = _resolve_monitor_from_state()

    mon = _resolve_monitor(monitor)
    gap_x, gap_y = _placement(mon)

    USER_CONFIG.parent.mkdir(parents=True, exist_ok=True)

    # Write the cairo Lua first so the conf's lua_load resolves on read.
    lua_text = _render_lua(accent_hex=accent)
    if lua_text is not None:
        USER_LUA.write_text(lua_text, encoding="utf-8")

    text = render_config(
        accent_hex=accent, density=density,
        gap_x=gap_x, gap_y=gap_y,
        lua_path=USER_LUA,
    )
    USER_CONFIG.write_text(text, encoding="utf-8")
    return USER_CONFIG


def _resolve_accent_from_state() -> str:
    try:
        from mackes.state import MackesState
        state = MackesState.load()
        preset = state.active_preset or "mackes"
    except Exception:  # noqa: BLE001
        preset = "mackes"
    return _PRESET_ACCENTS.get(preset, _PRESET_ACCENTS["mackes"])


def _tweaks_path() -> Path:
    """Resolve ~/.config/mackes-shell/tweaks.json (shell's UI preferences)."""
    from mackes.state import CONFIG_DIR
    return CONFIG_DIR / "tweaks.json"


def _read_tweaks() -> dict:
    import json
    try:
        return json.loads(_tweaks_path().read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def _resolve_density_from_state() -> str:
    d = _read_tweaks().get("conky_density")
    return d if d in DENSITY_VALUES else DEFAULT_DENSITY


def _resolve_monitor_from_state() -> Optional[str]:
    m = _read_tweaks().get("conky_monitor")
    return m or None


# ---------------------------------------------------------------------------
# Process control
# ---------------------------------------------------------------------------


def _conky_pid() -> Optional[int]:
    """Return the user's conky pid (first one), or None."""
    if shutil.which("pgrep") is None:
        return None
    try:
        r = subprocess.run(["pgrep", "-x", "-u", str(os.getuid()), "conky"],
                           capture_output=True, text=True, timeout=4)
        if r.returncode != 0:
            return None
        first = r.stdout.strip().splitlines()[:1]
        return int(first[0]) if first else None
    except (OSError, subprocess.TimeoutExpired, ValueError):
        return None


def is_running() -> bool:
    return _conky_pid() is not None


def start(*, force: bool = False) -> bool:
    """Spawn `conky -d -c <user-config>`. Idempotent unless force=True.

    Returns False (without raising) under Wayland — conky is X11-only
    and the GTK overlay replacement is captured as a follow-up task.
    """
    if _is_wayland():
        # Don't try; conky on Wayland either silently no-ops or spams
        # the journal. The Wayland HUD is a follow-up (see task #16
        # "Wayland session detection (foundation only)").
        return False
    if shutil.which("conky") is None:
        return False
    if is_running() and not force:
        return True
    if not USER_CONFIG.is_file():
        try:
            write_config()
        except FileNotFoundError:
            return False
    try:
        subprocess.Popen(
            ["conky", "-d", "-c", str(USER_CONFIG)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError:
        return False
    # Best-effort: after conky maps its window, clear its X11 SHAPE
    # *input* region so mouse events pass through to xfdesktop. Done
    # in a background thread so spawn returns immediately.
    import threading
    threading.Thread(target=_clear_input_shape_when_mapped,
                     daemon=True).start()
    return True


# ---------------------------------------------------------------------------
# Click-through (X SHAPE input region)
# ---------------------------------------------------------------------------


def _clear_input_shape_when_mapped(retries: int = 20,
                                   delay_s: float = 0.15) -> bool:
    """Poll for the mackes-conky X window, then clear its input shape.

    Returns True if the input region was cleared, False otherwise.
    Pure ctypes — no python-xlib dep.
    """
    import time
    for _ in range(retries):
        wid = _find_conky_window()
        if wid is not None:
            return _clear_input_shape(wid)
        time.sleep(delay_s)
    return False


def _find_conky_window() -> Optional[int]:
    """Locate the mackes-conky window by WM_CLASS. Uses xdotool if
    present, falls back to xwininfo+grep."""
    if shutil.which("xdotool") is not None:
        try:
            r = subprocess.run(
                ["xdotool", "search", "--class", "mackes-conky"],
                capture_output=True, text=True, timeout=3,
            )
            line = (r.stdout or "").strip().splitlines()
            if line:
                return int(line[0], 0)
        except (OSError, subprocess.TimeoutExpired, ValueError):
            pass
    if shutil.which("xprop") is not None and shutil.which("xwininfo") is not None:
        try:
            r = subprocess.run(["xwininfo", "-root", "-tree"],
                               capture_output=True, text=True, timeout=3)
            for line in r.stdout.splitlines():
                if "mackes-conky" in line.lower():
                    # leading hex window id, e.g. "     0x3400001 (...)"
                    parts = line.strip().split()
                    if parts and parts[0].startswith("0x"):
                        try:
                            return int(parts[0], 16)
                        except ValueError:
                            continue
        except (OSError, subprocess.TimeoutExpired):
            pass
    return None


def _clear_input_shape(window_id: int) -> bool:
    """XShapeCombineRectangles(window, ShapeInput, 0,0, NULL, 0, ShapeSet, 0).

    An empty input shape = all events pass through to the window below.
    """
    import ctypes
    import ctypes.util
    libx11 = ctypes.util.find_library("X11")
    libxext = ctypes.util.find_library("Xext")
    if not libx11 or not libxext:
        return False
    try:
        X11 = ctypes.CDLL(libx11)
        Xext = ctypes.CDLL(libxext)
    except OSError:
        return False
    X11.XOpenDisplay.restype = ctypes.c_void_p
    X11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    dpy = X11.XOpenDisplay(None)
    if not dpy:
        return False
    try:
        # ShapeInput = 2, ShapeSet = 0 — from X11/extensions/shapeconst.h
        Xext.XShapeCombineRectangles.restype = None
        Xext.XShapeCombineRectangles.argtypes = [
            ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int,
            ctypes.c_int, ctypes.c_int, ctypes.c_void_p,
            ctypes.c_int, ctypes.c_int, ctypes.c_int,
        ]
        Xext.XShapeCombineRectangles(
            dpy, ctypes.c_ulong(window_id), 2,
            0, 0, None, 0, 0, 0,
        )
        X11.XFlush.argtypes = [ctypes.c_void_p]
        X11.XFlush(dpy)
        return True
    finally:
        X11.XCloseDisplay.argtypes = [ctypes.c_void_p]
        X11.XCloseDisplay(dpy)


def stop() -> bool:
    if shutil.which("pkill") is None:
        return False
    try:
        subprocess.run(["pkill", "-u", str(os.getuid()), "-x", "conky"],
                       capture_output=True, timeout=4)
        return True
    except (OSError, subprocess.TimeoutExpired):
        return False


def reload() -> bool:
    """SIGUSR1 the running conky — picks up the new config without flashing.

    Conky 1.10+ reloads on SIGUSR1. Falls back to stop+start when the
    process isn't found.
    """
    pid = _conky_pid()
    if pid is None:
        return start(force=True)
    try:
        os.kill(pid, signal.SIGUSR1)
        return True
    except OSError:
        # Process gone between the pgrep and the kill — bounce.
        return start(force=True)


def restart_with(*, accent: Optional[str] = None,
                 density: Optional[str] = None,
                 monitor: Optional[str] = None) -> bool:
    """Regenerate config (preset/density/monitor swap) and SIGUSR1 reload."""
    write_config(accent=accent, density=density, monitor=monitor)
    return reload()


# ---------------------------------------------------------------------------
# Tweaks integration
# ---------------------------------------------------------------------------


def install_autostart() -> Path:
    """Write ~/.config/autostart/mackes-conky.desktop."""
    src = None
    root = _data_root()
    if root is not None:
        candidate = root.parent / "applications" / "mackes-conky.desktop"
        if candidate.is_file():
            src = candidate
    if src is None:
        for p in (Path("/usr/share/applications/mackes-conky.desktop"),):
            if p.is_file():
                src = p
                break
    AUTOSTART_FILE.parent.mkdir(parents=True, exist_ok=True)
    if src is not None:
        AUTOSTART_FILE.write_text(src.read_text(encoding="utf-8"),
                                  encoding="utf-8")
    else:
        AUTOSTART_FILE.write_text(
            "[Desktop Entry]\n"
            "Type=Application\n"
            "Name=Mackes Conky HUD\n"
            f"Exec=conky -d -c {USER_CONFIG}\n"
            "X-GNOME-Autostart-enabled=true\n"
            "X-Mackes-Managed=1\n",
            encoding="utf-8",
        )
    return AUTOSTART_FILE


def uninstall_autostart() -> bool:
    if AUTOSTART_FILE.exists():
        try:
            AUTOSTART_FILE.unlink()
            return True
        except OSError:
            return False
    return True


def apply_tweak(enabled: bool, *, accent: Optional[str] = None,
                density: Optional[str] = None,
                monitor: Optional[str] = None) -> None:
    """Tweaks panel calls this when the user toggles 'Show Conky HUD'."""
    if enabled:
        write_config(accent=accent, density=density, monitor=monitor)
        install_autostart()
        start(force=True)
    else:
        uninstall_autostart()
        stop()
