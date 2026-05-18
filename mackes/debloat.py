"""XFCE debloat — 5 levels of removal, L1 (light) → L5 (minimal viable).

v1.4.0 task #95. Each tier is an idempotent dnf-remove set + (optionally)
an xfconf reset list. Tiers are cumulative: applying L3 implies L1 + L2
+ L3 buckets all run. Apply via `apply_level(N)` from the panel.

The bucket definitions follow the general "Arch minimalism / debloat-XFCE"
cultural pattern. The user referenced a /btw discussion which I don't
have direct context on — these can be adjusted in a follow-up once that
source surfaces.

Public API:

  LEVELS                       : list[DebloatLevel]
  describe_level(n)            : DebloatLevel
  preview(level_n)             : dict {removable: list[str], absent: list[str],
                                       xfconf_resets: list[str]}
  apply_level(level_n)         : list[str] action log
"""
from __future__ import annotations

import shutil
import subprocess
from dataclasses import dataclass, field
from typing import List

from mackes.logging import log_action


# ---------------------------------------------------------------------------
# Tier definitions
# ---------------------------------------------------------------------------


@dataclass
class DebloatLevel:
    n: int                         # 1..5
    name: str                      # short tier name
    blurb: str                     # one-line description
    description: str               # multi-line longer description
    packages: List[str]            # dnf names to remove at this tier
    xfconf_resets: List[tuple[str, str]] = field(default_factory=list)
    notes: List[str] = field(default_factory=list)


LEVELS: List[DebloatLevel] = [
    DebloatLevel(
        n=1, name="Light",
        blurb="Remove obvious app-level bloat that almost no one uses on a "
              "Mackes box.",
        description=(
            "Strips the heaviest Fedora desktop preinstalls that aren't part "
            "of the XFCE shell itself: LibreOffice (use a Flatpak if needed), "
            "GNOME Software (Mackes has its own Apps panel), and a handful "
            "of GNOME accessory apps that ship with Fedora Workstation. Zero "
            "impact on the XFCE shell."
        ),
        packages=[
            "libreoffice-*",
            "gnome-software",
            "gnome-software-fedora-langpacks",
            "gnome-photos",
            "gnome-maps",
            "gnome-contacts",
            "gnome-weather",
            "gnome-clocks",
            "gnome-boxes",
            "rhythmbox",
            "totem",
            "cheese",
        ],
        notes=[
            "Safe — none of these are required by the XFCE shell.",
            "If you actively use any (e.g. LibreOffice), skip this level or "
            "reinstall the specific package afterwards.",
        ],
    ),
    DebloatLevel(
        n=2, name="Trim",
        blurb="Replace XFCE-adjacent helper apps with leaner alternatives.",
        description=(
            "Drops second-tier XFCE helpers that Mackes already replaces or "
            "doesn't need: xfce4-clipman (the mesh clipboard replaces it), "
            "xfce4-screenshooter (use flameshot via Apps panel), Mousepad "
            "(use any of the editors Mackes installs), Parole and Xfburn. "
            "The XFCE shell still works."
        ),
        packages=[
            "xfce4-clipman-plugin",
            "xfce4-screenshooter",
            "xfce4-screenshooter-plugin",
            "mousepad",
            "parole",
            "xfburn",
            "xfce4-dict-plugin",
            "xfce4-fsguard-plugin",
            "xfce4-genmon-plugin",
            "xfce4-mailwatch-plugin",
            "xfce4-mount-plugin",
            "xfce4-netload-plugin",
            "xfce4-places-plugin",
            "xfce4-smartbookmark-plugin",
            "xfce4-systemload-plugin",
            "xfce4-time-out-plugin",
            "xfce4-timer-plugin",
            "xfce4-wavelan-plugin",
            "xfce4-weather-plugin",
        ],
        notes=[
            "Mackes' default panel layout doesn't reference any of the "
            "removed xfce4-*-plugin entries.",
            "If you've customized the panel layout to use one of these, "
            "drop down a level.",
        ],
    ),
    DebloatLevel(
        n=3, name="Lean",
        blurb="Strip non-essential XFCE components — use Mackes-managed "
              "replacements.",
        description=(
            "Replaces XFCE bundled services with Mackes-managed equivalents: "
            "xfce4-notifyd (Mackes has its own mesh-aware notification surface), "
            "xfce4-power-manager (power-profiles-daemon handles it), the "
            "PulseAudio-specific plugin (most Fedora 44+ systems are PipeWire "
            "now). At this tier the XFCE shell is still functional but "
            "noticeably leaner."
        ),
        packages=[
            "xfce4-notifyd",
            "xfce4-power-manager",
            "xfce4-power-manager-plugins",
            "xfce4-pulseaudio-plugin",
            "xfce4-sensors-plugin",
            "xfce4-cpugraph-plugin",
            "xfce4-diskperf-plugin",
            "thunar-archive-plugin",
            "thunar-media-tags-plugin",
        ],
        xfconf_resets=[
            ("xfce4-panel", "/plugins/plugin-power-manager"),
            ("xfce4-panel", "/plugins/plugin-pulseaudio"),
            ("xfce4-panel", "/plugins/plugin-notification-plugin"),
        ],
        notes=[
            "Mackes replaces xfce4-notifyd with its mesh notification system.",
            "power-profiles-daemon is the modern Fedora default.",
            "If you're still on PulseAudio (rare in 2026), keep "
            "xfce4-pulseaudio-plugin.",
        ],
    ),
    DebloatLevel(
        n=4, name="Minimal",
        blurb="Drop nearly every non-XFCE Fedora desktop accumulator.",
        description=(
            "Aggressive: removes most of Fedora's desktop accumulators that "
            "aren't part of XFCE itself. Tracker (metadata indexer that eats "
            "I/O), gnome-keyring extras, evince (use Mackes' Apps to install "
            "a PDF viewer of choice), file-roller, simple-scan, baobab. After "
            "L4 the system is XFCE + Mackes + the few apps you specifically "
            "want."
        ),
        packages=[
            "tracker",
            "tracker-miners",
            "tracker3",
            "tracker3-miners",
            "evince",
            "evince-nautilus",
            "file-roller",
            "file-roller-nautilus",
            "simple-scan",
            "baobab",
            "gnome-disk-utility",
            "gnome-system-monitor",
            "gnome-calculator",
            "gnome-screenshot",
            "gnome-font-viewer",
            "gnome-characters",
            "gnome-logs",
            "gnome-tour",
            "yelp",
            "shotwell",
        ],
        notes=[
            "Tracker reindexes ~/. Removing it gives back I/O.",
            "Replace the GNOME tools you actually want via the Mackes Apps panel.",
            "If you do PDF work, install evince or okular afterwards.",
        ],
    ),
    DebloatLevel(
        n=5, name="Viable",
        blurb="Minimum viable XFCE — only the four core components survive.",
        description=(
            "Strips XFCE down to the four daemons that make it XFCE: "
            "xfce4-panel, xfwm4, xfdesktop, xfsettingsd. Plus xfconf (config "
            "store) and the Mackes-required panel plugins (whisker, docklike, "
            "clock). Everything else — including most other xfce4-* helpers — "
            "is removed. Use this for headless-leaning or aesthetic-purist "
            "installs; expect to reinstall things via the Mackes Apps panel."
        ),
        packages=[
            "xfce4-about",
            "xfce4-appfinder",
            "xfce4-taskmanager",
            "xfce4-dev-tools",
            "xfce4-volumed-pulse",
            "xfce4-volumed",
            "xfce4-battery-plugin",
            "xfce4-cpufreq-plugin",
            "xfce4-eyes-plugin",
            "xfce4-verve-plugin",
            "xfce4-xkb-plugin",
            "ristretto",
            "thunar-vfs",
            "exo-tools",
            "gigolo",
            "catfish",
            "menulibre",       # Mackes ships LibreMenu through other means
        ],
        notes=[
            "At L5 expect minor regressions: keyboard layout switching may "
            "require manual setxkbmap, screenshot tools need to be reinstalled.",
            "Reversible: dnf install <pkg> brings any component back.",
            "Recommended only after taking a snapshot via Maintain → Snapshots.",
        ],
    ),
]


# ---------------------------------------------------------------------------
# Introspection
# ---------------------------------------------------------------------------


def describe_level(n: int) -> DebloatLevel:
    """Return the DebloatLevel for tier n (1..5). Raises if out of range."""
    for lvl in LEVELS:
        if lvl.n == n:
            return lvl
    raise ValueError(f"unknown debloat level: {n}")


def packages_up_to(level_n: int) -> List[str]:
    """Cumulative package list — applying L3 implies L1 + L2 + L3."""
    out: List[str] = []
    for lvl in LEVELS:
        if lvl.n <= level_n:
            out.extend(lvl.packages)
    # Dedupe while preserving order
    seen: set[str] = set()
    deduped: List[str] = []
    for p in out:
        if p in seen:
            continue
        seen.add(p)
        deduped.append(p)
    return deduped


def xfconf_resets_up_to(level_n: int) -> List[tuple[str, str]]:
    out: List[tuple[str, str]] = []
    for lvl in LEVELS:
        if lvl.n <= level_n:
            out.extend(lvl.xfconf_resets)
    return out


# ---------------------------------------------------------------------------
# Preview — what would change if level N applied right now
# ---------------------------------------------------------------------------


def preview(level_n: int) -> dict:
    """Return what `apply_level(n)` would actually do on this peer.

    {
      "removable": [pkg, ...]      packages currently installed (would be removed),
      "absent":    [pkg, ...]      packages already absent (no-op),
      "xfconf_resets": [(ch,p), ...],
      "level":     DebloatLevel,
    }
    """
    pkgs = packages_up_to(level_n)
    removable: List[str] = []
    absent:    List[str] = []
    for raw in pkgs:
        # Glob patterns (e.g. "libreoffice-*") — let dnf decide; treat as
        # 'maybe installed' for preview purposes.
        if "*" in raw:
            if _glob_installed(raw):
                removable.append(raw)
            else:
                absent.append(raw)
            continue
        if _rpm_installed(raw):
            removable.append(raw)
        else:
            absent.append(raw)
    return {
        "level": describe_level(level_n),
        "removable": removable,
        "absent": absent,
        "xfconf_resets": xfconf_resets_up_to(level_n),
    }


def _rpm_installed(pkg: str) -> bool:
    if not shutil.which("rpm"):
        return False
    try:
        subprocess.check_call(["rpm", "-q", pkg],
                              stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL)
        return True
    except subprocess.CalledProcessError:
        return False


def _glob_installed(pattern: str) -> bool:
    """Cheap check: ask rpm if any package matches the glob pattern."""
    if not shutil.which("rpm"):
        return False
    try:
        out = subprocess.check_output(
            ["rpm", "-qa", pattern], text=True,
            stderr=subprocess.DEVNULL, timeout=10,
        )
        return bool(out.strip())
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
        return False


# ---------------------------------------------------------------------------
# Apply
# ---------------------------------------------------------------------------


def apply_level(level_n: int) -> List[str]:
    """Remove every package in the cumulative L1..Ln buckets, idempotently."""
    actions: List[str] = []
    lvl = describe_level(level_n)
    actions.append(f"debloat L{lvl.n} ({lvl.name}): starting")

    pkgs = packages_up_to(level_n)
    if not pkgs:
        actions.append("debloat: nothing to do")
        return actions

    # Skip already-absent packages to keep dnf output clean
    target = [p for p in pkgs if _rpm_installed(p) or "*" in p]
    if not target:
        actions.append("debloat: all packages already absent")
        return actions

    if shutil.which("dnf") is None:
        actions.append("debloat: dnf not available — skipping")
        return actions

    # Route through AdminSession (v1.4.0) — one auth for the whole session.
    from mackes.admin_session import AdminSession
    rc, out = AdminSession.instance().run(
        ["dnf", "remove", "-y", *target], timeout=900,
    )
    last_line = (out or "").strip().splitlines()
    last_line = last_line[-1] if last_line else f"rc={rc}"
    if rc == 0:
        actions.append(f"debloat L{lvl.n}: removed {len(target)} package(s) — {last_line}")
    else:
        actions.append(f"debloat L{lvl.n}: dnf rc={rc} — {last_line}")

    # xfconf resets
    resets = xfconf_resets_up_to(level_n)
    if resets and shutil.which("xfconf-query"):
        for channel, prop in resets:
            try:
                subprocess.run(
                    ["xfconf-query", "--channel", channel, "--property", prop,
                     "--reset", "--recursive"],
                    capture_output=True, text=True, timeout=10,
                )
                actions.append(f"debloat L{lvl.n}: xfconf reset {channel}{prop}")
            except (OSError, subprocess.TimeoutExpired):
                continue

    for line in actions:
        log_action(line)
    return actions
