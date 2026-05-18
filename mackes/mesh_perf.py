"""mackes.mesh_perf — performance knobs for the mesh fabric.

Implements three open-source-tooling-driven perf wins:

  #2  Kernel-mode WireGuard datapath (vs userspace wireguard-go)
  #3  WireGuard MTU + UDP GSO tuning
  #9  Concurrent probes  → see mesh.py + mesh_services.py

Each toggle lives in ~/.config/mackes-shell/tweaks.json so the same
state is read by the Tweaks panel, the Mesh Performance panel, the
wizard's tailscale-up call, and the CLI. Effective values are applied
by feeding the right --tun / --mtu flags to `tailscale up` and by
writing a sysctl drop-in that bumps UDP socket BQL.

Public API (stable; consumed by mesh_vpn.tailscale_up_with_headscale,
the Mesh Performance panel, and the headless CLI):

  kernel_module_loaded()            -> bool
  kernel_mode_available()           -> bool
  use_kernel_mode_preference()      -> bool   (reads tweaks.json)
  set_use_kernel_mode(bool)         -> None

  current_mtu(iface="tailscale0")   -> int | None
  preferred_mtu()                   -> int    (reads tweaks.json or 0 default)
  set_preferred_mtu(int)            -> None

  gso_enabled(iface)                -> bool   (ethtool offload status)
  apply_sysctl_tuning()             -> list[str]  actions

  tailscale_up_flags()              -> list[str]  flags merged into
                                                  mesh_vpn.tailscale_up_with_headscale
"""
from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path
from typing import Optional


# ---------------------------------------------------------------------------
# Tweaks storage (shared with mackes.workbench.system.tweaks_full)
# ---------------------------------------------------------------------------


# Keys we own in tweaks.json
TWEAK_KERNEL_WG = "mesh_kernel_wg"      # bool — prefer kernel WireGuard
TWEAK_LAN_MTU   = "mesh_lan_mtu"        # bool — bump MTU for LAN-only
TWEAK_SYSCTL    = "mesh_sysctl_tuning"  # bool — apply UDP BQL drop-in

LAN_MTU = 1380   # +100 over default; safe on any 1500-MTU LAN link
SYSCTL_DROPIN = Path("/etc/sysctl.d/90-mackes-mesh.conf")
SYSCTL_PAYLOAD = """# Mackes mesh UDP tuning (mackes.mesh_perf.apply_sysctl_tuning).
# Larger socket buffers + bigger BQL so userspace WireGuard / Tailscale
# can sustain gigabit on hosts with bursty packet arrival.
net.core.rmem_max = 26214400
net.core.wmem_max = 26214400
net.core.rmem_default = 1048576
net.core.wmem_default = 1048576
net.core.netdev_max_backlog = 5000
"""


def _tweaks_path() -> Path:
    from mackes.state import CONFIG_DIR
    return CONFIG_DIR / "tweaks.json"


def _read_tweaks() -> dict:
    p = _tweaks_path()
    if not p.exists():
        return {}
    try:
        return json.loads(p.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def _write_tweak(key: str, value) -> None:
    p = _tweaks_path()
    p.parent.mkdir(parents=True, exist_ok=True)
    data = _read_tweaks()
    data[key] = value
    p.write_text(json.dumps(data, indent=2, sort_keys=True),
                 encoding="utf-8")


# ---------------------------------------------------------------------------
# #2 — Kernel-mode WireGuard
# ---------------------------------------------------------------------------


def kernel_module_loaded() -> bool:
    """True iff the kernel `wireguard` module is currently loaded."""
    try:
        with open("/proc/modules", encoding="utf-8") as f:
            for line in f:
                if line.split(" ", 1)[0] == "wireguard":
                    return True
    except OSError:
        pass
    return False


def kernel_mode_available() -> bool:
    """True iff WireGuard can be loaded (module installed even if not
    yet loaded). Without admin we can't `modprobe`; we just confirm
    `/lib/modules/$(uname -r)/kernel/drivers/net/wireguard.ko*` exists.
    """
    if kernel_module_loaded():
        return True
    try:
        r = subprocess.run(["modinfo", "-n", "wireguard"],
                           capture_output=True, text=True, timeout=4)
        return r.returncode == 0 and bool((r.stdout or "").strip())
    except (OSError, subprocess.TimeoutExpired):
        return False


def use_kernel_mode_preference() -> bool:
    """User toggle from Tweaks panel. Default True — kernel mode is
    strictly better when available."""
    return bool(_read_tweaks().get(TWEAK_KERNEL_WG, True))


def set_use_kernel_mode(enabled: bool) -> None:
    _write_tweak(TWEAK_KERNEL_WG, bool(enabled))


# ---------------------------------------------------------------------------
# #3 — MTU + GSO
# ---------------------------------------------------------------------------


def current_mtu(iface: str = "tailscale0") -> Optional[int]:
    """Read MTU from /sys/class/net/<iface>/mtu. None if iface absent."""
    p = Path(f"/sys/class/net/{iface}/mtu")
    if not p.exists():
        return None
    try:
        return int(p.read_text().strip())
    except (OSError, ValueError):
        return None


def preferred_mtu() -> int:
    """Returns the MTU we'd pass to `tailscale up --mtu=`. 0 means
    "don't pass --mtu" (let Tailscale auto-detect, the default 1280)."""
    if bool(_read_tweaks().get(TWEAK_LAN_MTU, False)):
        return LAN_MTU
    return 0


def set_preferred_mtu(mtu: int) -> None:
    """0 disables the toggle (auto); LAN_MTU (1380) enables it.
    Other values are stored as-is for advanced users editing
    tweaks.json by hand."""
    if mtu == 0:
        _write_tweak(TWEAK_LAN_MTU, False)
    elif mtu == LAN_MTU:
        _write_tweak(TWEAK_LAN_MTU, True)
    else:
        _write_tweak(TWEAK_LAN_MTU, int(mtu))


def gso_enabled(iface: str = "tailscale0") -> bool:
    """Probe ethtool for generic-segmentation-offload status."""
    if shutil.which("ethtool") is None:
        return False
    try:
        r = subprocess.run(
            ["ethtool", "-k", iface],
            capture_output=True, text=True, timeout=4,
        )
        for line in (r.stdout or "").splitlines():
            if "generic-segmentation-offload:" in line:
                return line.strip().endswith(": on")
    except (OSError, subprocess.TimeoutExpired):
        pass
    return False


def apply_sysctl_tuning() -> list[str]:
    """Drop the Mackes sysctl tuning file in /etc/sysctl.d and reload.
    Requires admin; routes through mackes.admin_session."""
    from mackes.admin_session import AdminSession
    actions: list[str] = []
    import tempfile
    with tempfile.NamedTemporaryFile(mode="w", delete=False,
                                      suffix=".conf",
                                      encoding="utf-8") as tmp:
        tmp.write(SYSCTL_PAYLOAD)
        tmp_path = tmp.name
    rc, out = AdminSession.instance().run(
        ["install", "-D", "-m", "0644", tmp_path, str(SYSCTL_DROPIN)],
        timeout=10,
    )
    try:
        Path(tmp_path).unlink()
    except OSError:
        pass
    if rc != 0:
        actions.append(f"sysctl drop-in install failed: {out.strip()}")
        return actions
    actions.append(f"sysctl: wrote {SYSCTL_DROPIN}")
    rc2, out2 = AdminSession.instance().run(
        ["sysctl", "--system"], timeout=10,
    )
    if rc2 == 0:
        actions.append("sysctl: reload OK")
        _write_tweak(TWEAK_SYSCTL, True)
    else:
        actions.append(f"sysctl: reload failed: {out2.strip()}")
    return actions


def remove_sysctl_tuning() -> list[str]:
    from mackes.admin_session import AdminSession
    rc, out = AdminSession.instance().run(
        ["rm", "-f", str(SYSCTL_DROPIN)], timeout=5,
    )
    AdminSession.instance().run(["sysctl", "--system"], timeout=10)
    _write_tweak(TWEAK_SYSCTL, False)
    return [f"sysctl: removed {SYSCTL_DROPIN} (rc={rc})"]


def sysctl_tuning_active() -> bool:
    """True iff our sysctl drop-in is currently on disk."""
    return SYSCTL_DROPIN.exists()


# ---------------------------------------------------------------------------
# Merge perf flags into tailscale-up
# ---------------------------------------------------------------------------


def tailscale_up_flags() -> list[str]:
    """Extra flags `mesh_vpn.tailscale_up_with_headscale` should append.

    --tun=<name> + --netstack=false forces kernel-mode datapath when
    the wireguard module is loaded; otherwise we leave defaults so
    Tailscale falls back to userspace wireguard-go.

    --mtu=<N> bumps the WireGuard MTU when the user has opted into the
    LAN-MTU toggle.
    """
    flags: list[str] = []
    if use_kernel_mode_preference() and kernel_mode_available():
        flags.extend(["--tun=mackes-wg0", "--netstack=false"])
    mtu = preferred_mtu()
    if mtu > 0:
        flags.append(f"--mtu={mtu}")
    return flags


# ---------------------------------------------------------------------------
# Summary for the Mesh Performance panel
# ---------------------------------------------------------------------------


def summary() -> dict[str, object]:
    """One-shot snapshot used by the Mesh Performance panel."""
    iface = "tailscale0"
    # Pick the actual mesh iface if --tun was used
    for cand in ("mackes-wg0", "tailscale0", "wg0"):
        if Path(f"/sys/class/net/{cand}").exists():
            iface = cand
            break
    return {
        "kernel_module_loaded":   kernel_module_loaded(),
        "kernel_mode_available":  kernel_mode_available(),
        "use_kernel_preference":  use_kernel_mode_preference(),
        "current_mtu":            current_mtu(iface),
        "preferred_mtu":          preferred_mtu(),
        "gso_enabled":            gso_enabled(iface),
        "sysctl_tuning_active":   sysctl_tuning_active(),
        "iface":                  iface,
    }


__all__ = [
    # Kernel WireGuard
    "kernel_module_loaded", "kernel_mode_available",
    "use_kernel_mode_preference", "set_use_kernel_mode",
    # MTU + GSO
    "current_mtu", "preferred_mtu", "set_preferred_mtu",
    "gso_enabled",
    # sysctl
    "apply_sysctl_tuning", "remove_sysctl_tuning",
    "sysctl_tuning_active",
    # tailscale wiring
    "tailscale_up_flags",
    # UI
    "summary",
    # constants
    "LAN_MTU", "SYSCTL_DROPIN",
]
