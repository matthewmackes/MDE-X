"""Mackes Conky HUD — birthright orchestration + start/stop (v1.4.0).

Locked decisions (5-question survey, 2026-05-17):
  Q1  Top-right, 400 × ⅔ screen height
  Q2  Birthright autostart + Tweaks toggle
  Q3  Tiered refresh (1s system / 30s Mackes state / 60s rare)
  Q4  10 blocks: header / mesh / fleet / drift / storage / notifications /
      media / remote / services / hardware
  Q5  Opaque Carbon Gray 90 + 3px accent left-edge

Public API:

  render_config(*, accent_hex, height_px)  → str  (the conky config text)
  write_config(*, accent=..., height_px=...) → Path
  start()    → bool   (spawn `conky -d -c <conf>` if not already running)
  stop()     → bool   (killall conky for the user)
  is_running() → bool
  restart_with(accent=...) → bool   (regenerate config + bounce process)
  apply_tweak(enabled, *, accent=...)        (Tweaks panel toggle entry)
  install_autostart() / uninstall_autostart()
"""
from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path
from typing import Optional


CONFIG_DIR = Path(os.path.expanduser("~/.config/mackes-conky"))
USER_CONFIG = CONFIG_DIR / "mackes.conf"
AUTOSTART_FILE = Path(os.path.expanduser("~/.config/autostart/mackes-conky.desktop"))


# Per-preset accent hexes — match data/css/accents/*.css
_PRESET_ACCENTS = {
    "hashbang": "fa4d56",
    "mackes":   "f1853d",
    "daylight": "f1c21b",
    "vanilla":  "0f62fe",
    "node":     "42be65",
}


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


def _screen_height_px() -> int:
    """Detect primary monitor height. Falls back to 720 if X11 isn't reachable."""
    try:
        out = subprocess.check_output(
            ["xrandr", "--current"], text=True, timeout=4,
            stderr=subprocess.DEVNULL,
        )
        for line in out.splitlines():
            if " primary " in line or " connected primary " in line:
                # e.g. "HDMI-1 connected primary 1920x1080+0+0 ..."
                for token in line.split():
                    if "x" in token and "+" in token:
                        return int(token.split("x")[1].split("+")[0])
    except (FileNotFoundError, subprocess.CalledProcessError,
            subprocess.TimeoutExpired):
        pass
    return 720


def render_config(*, accent_hex: str, height_px: int) -> str:
    """Substitute the template's placeholders. Caller decides where to write it."""
    tmpl = _template_path()
    if tmpl is None:
        raise FileNotFoundError("mackes-conky.conf.template not found")
    text = tmpl.read_text(encoding="utf-8")
    return text.replace("{accent_hex}", accent_hex.lstrip("#")) \
               .replace("{height_px}", str(int(height_px)))


def write_config(*, accent: Optional[str] = None,
                 height_px: Optional[int] = None) -> Path:
    """Write ~/.config/mackes-conky/mackes.conf with the active accent."""
    if accent is None:
        accent = _resolve_accent_from_state()
    if height_px is None:
        # ⅔ of the primary monitor height (Q1 lock)
        height_px = max(360, int(_screen_height_px() * 2 / 3))
    text = render_config(accent_hex=accent, height_px=height_px)
    USER_CONFIG.parent.mkdir(parents=True, exist_ok=True)
    USER_CONFIG.write_text(text, encoding="utf-8")
    return USER_CONFIG


def _resolve_accent_from_state() -> str:
    """Look up the active preset and map to its accent hex."""
    try:
        from mackes.state import MackesState
        state = MackesState.load()
        preset = state.active_preset or "mackes"
    except Exception:  # noqa: BLE001
        preset = "mackes"
    return _PRESET_ACCENTS.get(preset, _PRESET_ACCENTS["mackes"])


# ---------------------------------------------------------------------------
# Process control
# ---------------------------------------------------------------------------


def is_running() -> bool:
    """True iff there's any conky process running on this user's session."""
    if shutil.which("pgrep") is None:
        return False
    try:
        r = subprocess.run(["pgrep", "-x", "-u", str(os.getuid()), "conky"],
                           capture_output=True, timeout=4)
        return r.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def start(*, force: bool = False) -> bool:
    """Spawn `conky -d -c <user-config>`. Idempotent unless force=True."""
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
        return True
    except OSError:
        return False


def stop() -> bool:
    """Send SIGTERM to any user-owned conky processes."""
    if shutil.which("pkill") is None:
        return False
    try:
        subprocess.run(["pkill", "-u", str(os.getuid()), "-x", "conky"],
                       capture_output=True, timeout=4)
        return True
    except (OSError, subprocess.TimeoutExpired):
        return False


def restart_with(*, accent: Optional[str] = None) -> bool:
    """Regenerate the config (e.g. on preset change) and bounce the process."""
    write_config(accent=accent)
    stop()
    return start(force=True)


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


def apply_tweak(enabled: bool, *, accent: Optional[str] = None) -> None:
    """Tweaks panel calls this when the user toggles 'Show Conky HUD'."""
    if enabled:
        write_config(accent=accent)
        install_autostart()
        start(force=True)
    else:
        uninstall_autostart()
        stop()
