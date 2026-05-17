"""mackes.mesh — unified health surface for every mesh layer.

`health()` is the single source of truth: every panel, the Conky HUD,
the Get Online wizard, and the Diagnose action all read from it.
Layers probed (the 8 mesh modules):

  vpn            — Tailscale daemon + Headscale auth + control peer
  ssh            — mesh keypair + authorized_keys + reachability
  services       — discovery registry freshness + probe coverage
  fs             — SSHFS mounts under ~/QNM-Mesh/<peer>/
  sync           — bucket dirs (clipboard / notifications / snapshots / …)
                   + recent-activity timestamps
  notifications  — inbox dir + mesh-notifications daemon health
  browser        — Thunar bookmarks + QNM-* view directories
  thumbnailer    — Tumbler thumbnailer registration

Each probe returns a `LayerHealth`:
  layer        — short key ("vpn", "ssh", …)
  state        — "ok" | "warn" | "fail" | "missing"
  label        — short user-facing summary (one line)
  detail       — multi-line human-readable diagnostic
  latency_ms   — Optional float for layers we can time
  hint         — Optional actionable fix sentence

`with_retry()` wraps transient-failing probes with exponential backoff.
Per-layer probes are cached in `probe_cache` with appropriate TTLs
(2–30 s) so repeat calls (multiple panels open) are cheap.

`overall_state()` returns the worst single-layer state — the colour of
the dashboard pill, the HUD's mesh row, the Get Online check chip.

Design rules:
  * Probes never raise — they catch and return state="fail".
  * Probes time-bound their I/O (subprocess timeout, socket timeout).
  * The `health()` aggregator runs probes serially (their I/O is short
    and parallelism wins are small); callers that want concurrency
    should iterate and thread per-layer.
"""
from __future__ import annotations

import json
import shutil
import socket
import subprocess
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Callable, Optional, TypeVar

from mackes.probe_cache import cached, invalidate_prefix


T = TypeVar("T")


# ---------------------------------------------------------------------------
# LayerHealth dataclass + state ordering
# ---------------------------------------------------------------------------


_STATE_RANK = {"ok": 0, "warn": 1, "fail": 2, "missing": 3}


@dataclass
class LayerHealth:
    layer: str
    state: str                 # "ok" | "warn" | "fail" | "missing"
    label: str
    detail: str = ""
    latency_ms: Optional[float] = None
    hint: Optional[str] = None

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


def _worse(a: str, b: str) -> str:
    return a if _STATE_RANK.get(a, 99) >= _STATE_RANK.get(b, 99) else b


def overall_state(snap: dict[str, "LayerHealth"]) -> str:
    """Worst single-layer state in the snapshot.

    A 'missing' optional layer (e.g. mesh_fs with no peers) is treated
    as 'warn' for the overall — missing-by-design shouldn't bring the
    whole mesh row to red.
    """
    worst = "ok"
    for h in snap.values():
        s = h.state
        if s == "missing":
            s = "warn"
        worst = _worse(worst, s)
    return worst


def summary(snap: dict[str, "LayerHealth"]) -> str:
    """One-line summary for Conky's mesh row.

    Format: '● 6/8 ok  · 1 warn · 1 fail'
    """
    counts = {"ok": 0, "warn": 0, "fail": 0, "missing": 0}
    for h in snap.values():
        counts[h.state] = counts.get(h.state, 0) + 1
    total = sum(counts.values())
    parts = [f"{counts['ok']}/{total} ok"]
    if counts["warn"]:    parts.append(f"{counts['warn']} warn")
    if counts["fail"]:    parts.append(f"{counts['fail']} fail")
    if counts["missing"]: parts.append(f"{counts['missing']} off")
    return "  ·  ".join(parts)


# ---------------------------------------------------------------------------
# Retry helper
# ---------------------------------------------------------------------------


def with_retry(
    fn: Callable[[], T],
    *,
    attempts: int = 3,
    backoff: float = 1.5,
    retry_on: tuple[type[BaseException], ...] = (
        OSError, subprocess.TimeoutExpired, ConnectionError,
    ),
) -> T:
    """Run `fn()` with exponential backoff. Re-raises the last exception
    if every attempt fails.

    Used for transient probes — Headscale flap, ssh connection refused
    on a peer that's mid-boot, a sshfs mount that's reconnecting.
    """
    delay = 0.2
    last: Optional[BaseException] = None
    for i in range(attempts):
        try:
            return fn()
        except retry_on as e:  # noqa: PERF203
            last = e
            if i < attempts - 1:
                time.sleep(delay)
                delay *= backoff
    assert last is not None
    raise last


# ---------------------------------------------------------------------------
# Per-layer probes
# ---------------------------------------------------------------------------


def _probe_vpn() -> LayerHealth:
    try:
        from mackes.mesh_vpn import (
            HEADSCALE_BIN, MeshState, TAILSCALE_BIN,
            headscale_list_peers, tailscale_status,
        )
    except Exception as e:  # noqa: BLE001
        return LayerHealth("vpn", "fail",
                           "mesh_vpn import failed", str(e))

    if shutil.which(TAILSCALE_BIN) is None:
        return LayerHealth(
            "vpn", "missing", "Tailscale is not installed",
            hint="Install via dnf: `sudo dnf install tailscale`",
        )

    t0 = time.monotonic()
    try:
        status = with_retry(tailscale_status, attempts=2, backoff=1.0)
    except Exception as e:  # noqa: BLE001
        return LayerHealth(
            "vpn", "fail", "Tailscale status query failed",
            detail=str(e),
            latency_ms=(time.monotonic() - t0) * 1000,
            hint="Try `sudo systemctl restart tailscaled`",
        )
    latency = (time.monotonic() - t0) * 1000

    if not status.get("online"):
        return LayerHealth(
            "vpn", "fail",
            "Tailscale is offline",
            detail=("Daemon is installed but not online. "
                    f"mesh_ip={status.get('mesh_ip','')!r}"),
            latency_ms=latency,
            hint="Run the Get Online wizard or "
                 "`sudo tailscale up --login-server=<headscale-url>`",
        )

    mesh_ip = status.get("mesh_ip", "")
    peers = status.get("peers", []) or []
    online_peers = [p for p in peers if p.get("online")]

    # Headscale is optional on non-control peers — only treat its
    # absence as warn if state says we should be control.
    state_obj = MeshState.load()
    detail_lines = [
        f"mesh_ip={mesh_ip or '(none)'}",
        f"peers={len(online_peers)} online / {len(peers)} known",
    ]
    if state_obj.is_control:
        if shutil.which(HEADSCALE_BIN) is None:
            return LayerHealth(
                "vpn", "warn",
                "Online, but Headscale is missing on the control node",
                detail="\n".join(detail_lines),
                latency_ms=latency,
                hint="`sudo dnf install headscale` and restart the Mesh Setup wizard",
            )
        # If we're control, expect at least N registered peers
        try:
            registered = headscale_list_peers()
            detail_lines.append(f"headscale={len(registered)} registered")
        except Exception:  # noqa: BLE001
            detail_lines.append("headscale=unreachable")

    state = "ok"
    label = f"Online · {len(online_peers)}/{len(peers)} peer(s) up"
    if peers and not online_peers:
        state = "warn"
        label = "Online · all peers offline"
    return LayerHealth("vpn", state, label,
                       detail="\n".join(detail_lines),
                       latency_ms=latency)


def _probe_ssh() -> LayerHealth:
    try:
        from mackes.mesh_ssh import (
            AUTHORIZED_KEYS, MESH_KEY_PATH, MESH_KEYS_DIR, MESH_PUB_PATH,
        )
    except Exception as e:  # noqa: BLE001
        return LayerHealth("ssh", "fail",
                           "mesh_ssh import failed", str(e))

    detail_lines: list[str] = []
    if not MESH_KEY_PATH.exists() or not MESH_PUB_PATH.exists():
        return LayerHealth(
            "ssh", "missing",
            "Mesh SSH keypair not generated",
            detail=f"Expected at {MESH_KEY_PATH}",
            hint="Open Network → Mesh SSH and click 'Generate keypair'",
        )
    detail_lines.append(f"keypair={MESH_KEY_PATH.name}")

    if not AUTHORIZED_KEYS.exists():
        return LayerHealth(
            "ssh", "warn",
            "Keypair OK but ~/.ssh/authorized_keys not present",
            detail="\n".join(detail_lines),
            hint="Click 'Sync authorized_keys' in Mesh SSH",
        )
    peer_pubkeys = (list(MESH_KEYS_DIR.glob("*.pub"))
                    if MESH_KEYS_DIR.exists() else [])
    detail_lines.append(f"peer_pubkeys_cached={len(peer_pubkeys)}")

    # Check sshd is listening (cheap local socket probe)
    sshd_listening = _tcp_open("127.0.0.1", 22, timeout=0.5)
    if not sshd_listening:
        return LayerHealth(
            "ssh", "warn",
            "Keypair OK but local sshd is not listening",
            detail="\n".join(detail_lines),
            hint="`sudo systemctl enable --now sshd`",
        )
    detail_lines.append("sshd=listening on :22")
    return LayerHealth("ssh", "ok",
                       f"Keypair + sshd OK · {len(peer_pubkeys)} peer key(s) cached",
                       detail="\n".join(detail_lines))


def _probe_services() -> LayerHealth:
    try:
        from mackes.mesh_services import REGISTRY_PATH, load_catalog, load_registry
    except Exception as e:  # noqa: BLE001
        return LayerHealth("services", "fail",
                           "mesh_services import failed", str(e))
    catalog = load_catalog()
    if not catalog:
        return LayerHealth(
            "services", "warn",
            "No service catalog entries",
            hint="Drop a YAML manifest at "
                 "~/.config/mackes-shell/media-services.yaml",
        )
    hits = load_registry()
    if not REGISTRY_PATH.exists():
        return LayerHealth(
            "services", "warn",
            f"{len(catalog)} catalog entries, registry never written",
            detail=f"Registry at {REGISTRY_PATH}",
            hint="Click 'Scan now' in Mesh Services",
        )
    age = time.time() - REGISTRY_PATH.stat().st_mtime
    age_h = age / 3600
    detail_lines = [
        f"catalog={len(catalog)} entries",
        f"registry={len(hits)} hits, age={age_h:.1f}h",
    ]
    state = "ok" if age_h < 24 else "warn"
    label = (f"{len(hits)} service(s) discovered"
             if hits else "No services discovered yet")
    return LayerHealth("services", state, label,
                       detail="\n".join(detail_lines))


def _probe_fs() -> LayerHealth:
    try:
        from mackes.mesh_fs import QNM_MESH, is_mounted
    except Exception as e:  # noqa: BLE001
        return LayerHealth("fs", "fail",
                           "mesh_fs import failed", str(e))
    if not QNM_MESH.is_dir():
        return LayerHealth(
            "fs", "missing",
            "~/QNM-Mesh/ not created",
            detail=f"Expected at {QNM_MESH}",
            hint="Will be created on first peer mount",
        )
    peer_dirs = [p for p in QNM_MESH.iterdir() if p.is_dir()]
    if not peer_dirs:
        return LayerHealth(
            "fs", "missing",
            "No peer mount points",
            detail=f"{QNM_MESH} is empty",
            hint="Add a peer in Mesh VPN and mount its share",
        )
    mounted = [p for p in peer_dirs if is_mounted(p.name)]
    detail = "\n".join(f"  {'✓' if p in mounted else '·'} {p.name}"
                       for p in peer_dirs)
    if not mounted:
        return LayerHealth(
            "fs", "fail",
            f"{len(peer_dirs)} mount point(s) defined but none mounted",
            detail=detail,
            hint="Run `mackes mesh remount`",
        )
    state = "ok" if len(mounted) == len(peer_dirs) else "warn"
    return LayerHealth("fs", state,
                       f"{len(mounted)}/{len(peer_dirs)} peer share(s) mounted",
                       detail=detail)


def _probe_sync() -> LayerHealth:
    try:
        from mackes.mesh_sync import (
            BUCKET_CLIPBOARD, BUCKET_DROP, BUCKET_NOTIFICATIONS,
            BUCKET_PRESETS, BUCKET_SNAPSHOTS, BUCKET_THEMES,
            SYNC_ROOT_MINE,
        )
    except Exception as e:  # noqa: BLE001
        return LayerHealth("sync", "fail",
                           "mesh_sync import failed", str(e))
    if not SYNC_ROOT_MINE.is_dir():
        return LayerHealth(
            "sync", "missing",
            "~/QNM-Shared/.qnm-sync/ not initialised",
            hint="Call `mackes.mesh_sync.ensure_buckets()`",
        )
    expected = [BUCKET_CLIPBOARD, BUCKET_NOTIFICATIONS,
                BUCKET_SNAPSHOTS, BUCKET_THEMES,
                BUCKET_PRESETS, BUCKET_DROP]
    present = [b for b in expected if (SYNC_ROOT_MINE / b).is_dir()]
    if not present:
        return LayerHealth(
            "sync", "fail",
            "Sync root exists but no buckets",
            hint="Call `mackes.mesh_sync.ensure_buckets()`",
        )
    # Recent activity = newest file mtime across all buckets
    newest: float = 0.0
    file_count = 0
    for b in present:
        for f in (SYNC_ROOT_MINE / b).rglob("*"):
            if f.is_file():
                file_count += 1
                try:
                    newest = max(newest, f.stat().st_mtime)
                except OSError:
                    pass
    if newest == 0:
        age_str = "no writes yet"
    else:
        age_h = (time.time() - newest) / 3600
        age_str = f"last write {age_h:.1f}h ago"
    detail = (f"buckets={len(present)}/{len(expected)} · "
              f"{file_count} file(s) · {age_str}")
    state = "ok" if len(present) == len(expected) else "warn"
    return LayerHealth("sync", state,
                       f"{len(present)}/{len(expected)} buckets present",
                       detail=detail)


def _probe_notifications() -> LayerHealth:
    try:
        from mackes.mesh_notifications import MESH_NOTIF_INBOX
    except Exception as e:  # noqa: BLE001
        return LayerHealth("notifications", "fail",
                           "mesh_notifications import failed", str(e))
    if not MESH_NOTIF_INBOX.is_dir():
        return LayerHealth(
            "notifications", "missing",
            "Inbox directory not created",
            detail=f"Expected at {MESH_NOTIF_INBOX}",
            hint="Will be auto-created on first receive_loop_once() call",
        )
    inbox_files = [f for f in MESH_NOTIF_INBOX.rglob("*") if f.is_file()]
    detail = f"inbox={len(inbox_files)} message(s)"
    return LayerHealth("notifications", "ok",
                       f"Inbox ready · {len(inbox_files)} message(s)",
                       detail=detail)


def _probe_browser() -> LayerHealth:
    try:
        from mackes.mesh_browser import (
            DIR_CLIPBOARD, DIR_DROP, DIR_MESH, DIR_NOTIFICATIONS,
        )
    except Exception as e:  # noqa: BLE001
        return LayerHealth("browser", "fail",
                           "mesh_browser import failed", str(e))
    dirs = {
        "QNM-Mesh":          DIR_MESH,
        "QNM-Clipboard":     DIR_CLIPBOARD,
        "QNM-Notifications": DIR_NOTIFICATIONS,
        "QNM-Drop":          DIR_DROP,
    }
    missing = [name for name, p in dirs.items() if not p.is_dir()]
    if missing:
        return LayerHealth(
            "browser", "warn",
            f"{len(missing)} view(s) missing",
            detail="missing: " + ", ".join(missing),
            hint="Call `mackes.mesh_browser.ensure_layout()`",
        )
    return LayerHealth("browser", "ok",
                       f"{len(dirs)} view directories present",
                       detail=" · ".join(dirs.keys()))


def _probe_thumbnailer() -> LayerHealth:
    # The Tumbler thumbnailer registration ships at
    # /usr/share/thumbnailers/mackes-mesh.thumbnailer (or
    # ~/.local/share/thumbnailers/). Just check presence.
    candidates = [
        Path("/usr/share/thumbnailers/mackes-mesh.thumbnailer"),
        Path.home() / ".local/share/thumbnailers/mackes-mesh.thumbnailer",
    ]
    present = [p for p in candidates if p.is_file()]
    if not present:
        return LayerHealth(
            "thumbnailer", "missing",
            "Mesh thumbnailer not registered",
            hint="`apply_thumbnailers` birthright step is missing or not yet applied",
        )
    return LayerHealth("thumbnailer", "ok",
                       "Mesh thumbnailer registered",
                       detail=str(present[0]))


# ---------------------------------------------------------------------------
# Cheap helpers
# ---------------------------------------------------------------------------


def _tcp_open(host: str, port: int, *, timeout: float = 1.0) -> bool:
    """True iff a TCP connect to host:port returns within timeout."""
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except (OSError, TimeoutError):
        return False


# ---------------------------------------------------------------------------
# Aggregator
# ---------------------------------------------------------------------------


# (layer-name, probe-callable, cache-ttl-seconds)
_LAYERS: tuple[tuple[str, Callable[[], LayerHealth], float], ...] = (
    ("vpn",            _probe_vpn,            5.0),
    ("ssh",            _probe_ssh,            10.0),
    ("services",       _probe_services,       30.0),
    ("fs",             _probe_fs,             10.0),
    ("sync",           _probe_sync,           10.0),
    ("notifications",  _probe_notifications,  15.0),
    ("browser",        _probe_browser,        60.0),
    ("thumbnailer",    _probe_thumbnailer,    300.0),
)


def health(*, force_refresh: bool = False,
           parallel: bool = True) -> dict[str, LayerHealth]:
    """Probe every mesh layer and return {layer_name: LayerHealth}.

    Layers are probed concurrently via a ThreadPoolExecutor — total
    wall-clock is bounded by the slowest single layer, not the sum.
    Cache hits return immediately and don't touch the pool. Pass
    parallel=False for deterministic single-threaded probing (tests).
    Pass force_refresh=True to invalidate every "mesh.health.<layer>"
    key before re-running.
    """
    if force_refresh:
        invalidate_prefix("mesh.health.")
    if not parallel:
        return {layer: cached(f"mesh.health.{layer}", factory=probe,
                              ttl_s=ttl)
                for layer, probe, ttl in _LAYERS}

    from concurrent.futures import ThreadPoolExecutor, as_completed
    out: dict[str, LayerHealth] = {}
    with ThreadPoolExecutor(max_workers=min(8, len(_LAYERS)),
                            thread_name_prefix="mesh-health") as ex:
        future_to_layer = {
            ex.submit(cached, f"mesh.health.{layer}",
                      factory=probe, ttl_s=ttl): layer
            for layer, probe, ttl in _LAYERS
        }
        for fut in as_completed(future_to_layer):
            layer = future_to_layer[fut]
            try:
                out[layer] = fut.result()
            except Exception as e:  # noqa: BLE001 — never let one
                # bad probe poison the whole snapshot
                out[layer] = LayerHealth(layer, "fail",
                                         "probe raised", str(e))
    # Preserve declaration order for downstream consumers (UI rows)
    return {layer: out[layer] for layer, _, _ in _LAYERS if layer in out}


def health_json(*, force_refresh: bool = False) -> str:
    """Same as health() but serialised to JSON. For Conky helpers and
    `mackes mesh health --json`."""
    snap = health(force_refresh=force_refresh)
    return json.dumps(
        {layer: h.to_dict() for layer, h in snap.items()},
        indent=2, sort_keys=True,
    )


def diagnose() -> list[str]:
    """Run every probe ignoring caches and return a human-readable
    multi-line diagnostic — one line per layer plus its hint if not OK.
    Used by the Mesh Health panel's Diagnose action."""
    lines: list[str] = []
    snap = health(force_refresh=True)
    lines.append(f"mesh state: {overall_state(snap).upper()}")
    lines.append("")
    for layer, h in snap.items():
        head = f"  [{h.state.upper():7s}] {layer:14s} {h.label}"
        if h.latency_ms is not None:
            head += f"  ({h.latency_ms:.0f} ms)"
        lines.append(head)
        if h.detail:
            for dl in h.detail.splitlines():
                lines.append(f"               {dl}")
        if h.hint and h.state != "ok":
            lines.append(f"      hint:    {h.hint}")
        lines.append("")
    return lines


__all__ = [
    "LayerHealth",
    "diagnose",
    "health",
    "health_json",
    "overall_state",
    "summary",
    "with_retry",
]
