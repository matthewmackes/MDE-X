"""NF-5.3 + NF-13 (v2.5) — thin read/configure wrapper around Nebula.

Python-side counterpart to the Rust `mackes-nebula-https-tunnel`
+ `mackesd::ca` modules. NO privileged operations live here —
enrollment, cert rotation, lighthouse promotion all route
through `mded`'s D-Bus surface (`dev.mackes.MDE.Nebula.Status`).
This module is the consumer side: read overlay state, write
sshd config snippets, emit WoL via the lighthouse relay.

Per the open-mesh / flat-trust directive (2026-05-23), every
service on a paired peer is reachable from every other peer.
The helpers here don't introduce ACLs.

DEAD-2.14 audit (2026-05-26 — coverage check vs Rust nebula
infrastructure):

  Status: PARTIAL COVERAGE — module kept; deletion deferred.

  Functions with Rust equivalent (can migrate consumers when
  the parallel Rust effort surfaces a stable D-Bus surface):
    - current_overlay_ip          → read /var/lib/mackesd/nebula/overlay-ip
                                    (GF-1.3.a) directly; trivial migration
    - lighthouse_addresses        → mackesd Nebula.Status::ListLighthouses
                                    (future, not yet on the D-Bus surface)
    - nebula_peer_ips             → dev.mackes.MDE.Nebula.Status::ListPeers
    - published_services_summary  → flat-trust + Nebula direct connect
                                    (services concept retired with mesh_services
                                    per DEAD-2.9; this helper is essentially dead)
    - reload_sshd                 → mackesd-side service-reload via
                                    Shell.Workers (already exists)

  Functions with NO Rust equivalent yet (= NF-21.x follow-on
  needed before mesh_nebula.py can fully retire):
    - write_sshd_overlay_bind     — RUST EQUIVALENT SHIPPED 2026-05-26
                                    as crates/mackesd/src/workers/sshd_overlay_bind.rs
                                    (NF-21.1 [✓] Done). Python helper kept
                                    only until workbench/network/mesh_ssh.py
                                    retires under EPIC-RETIRE-PY-WORKBENCH;
                                    no new callers should use it.
    - wol_via_lighthouse          — RUST EQUIVALENT SHIPPED 2026-05-26
                                    as workers::wol::wake_via_lighthouse
                                    + `mackesd wake-peer --via-lighthouse <ip>`
                                    CLI flag (NF-21.2 [✓] Done). Python
                                    helper kept only until consumers retire
                                    under EPIC-RETIRE-PY-WORKBENCH; no new
                                    callers should use it.
    - apply_nebula_firewall_preset — RUST EQUIVALENT SHIPPED 2026-05-26
                                     as crates/mackesd/src/workers/firewall_preset.rs
                                     (NF-21.3 [✓] Done). The Rust worker
                                     refines the preset to lighthouse-aware:
                                     UDP/4242 inbound on all peers + TCP/443
                                     inbound additionally on lighthouses
                                     (`role.host` marker exists). Python
                                     helper kept only until consumers retire
                                     under EPIC-RETIRE-PY-WORKBENCH.
    - emit_lighthouse_event / emit_ca_rotation / emit_https_fallback_state
      / emit_cert_expiry_warning  — NF-21.4 (2026-05-27) — all four
                                    helpers now publish to `nebula/<event>`
                                    Bus topics via the `mde-bus publish`
                                    CLI. Bus owns notification routing per
                                    BUS-4.4 + Q20 + Q96; toasts.jsonl path
                                    retired.

  Effective consumers post-DEAD-2 Wave 6+7:
    - mesh_media.py     — uses nebula_peer_ips (could migrate to D-Bus)
    - workbench/network/mesh_ssh.py — uses several functions; will
                          retire entirely under EPIC-RETIRE-PY-WORKBENCH
    - tests/test_mesh_nebula.py — keep alongside the module

  Retirement plan (becomes a separate DEAD-2.14.* sub-epic once
  NF-21.1..21.4 land):
    1. Migrate mesh_media to D-Bus (small follow-on)
    2. Retire python workbench (EPIC-RETIRE-PY-WORKBENCH / Q49 → 1.0)
    3. Migrate emit_* helpers to BUS publish (BUS-4.4)
    4. Migrate write_sshd_overlay_bind + apply_nebula_firewall_preset
       + wol_via_lighthouse to mackesd workers (NF-21.1..21.3)
    5. Delete mesh_nebula.py + tests

  Until then, the module stays — consumers above keep importing it.
"""
from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path
from typing import Optional


# ─────────────────────────────────────────────────────────────────
# Canonical paths
# ─────────────────────────────────────────────────────────────────

CONFIG_DIR = Path("/etc/nebula")
HOST_CERT_PATH = CONFIG_DIR / "host.crt"
LIGHTHOUSE_CONFIG_PATH = CONFIG_DIR / "lighthouse-config.yaml"
SSHD_DROPIN_DIR = Path("/etc/ssh/sshd_config.d")
SSHD_DROPIN_PATH = SSHD_DROPIN_DIR / "mackes-mesh.conf"


# ─────────────────────────────────────────────────────────────────
# Read helpers (no privilege required)
# ─────────────────────────────────────────────────────────────────

def current_overlay_ip(host_cert_path: Optional[Path] = None) -> Optional[str]:
    """NF-5.3 / NF-13.1 — return this peer's allocated overlay IP
    (e.g. "10.42.0.5") read from the nebula host cert.

    Implementation: shells out to `nebula-cert print -path <crt>`
    + greps the "Ips:" line. Returns None when nebula-cert isn't
    on PATH (dev boxes without the Fedora `nebula` package) or
    when the cert doesn't exist (pre-enrollment).
    """
    path = host_cert_path or HOST_CERT_PATH
    if not path.exists():
        return None
    if shutil.which("nebula-cert") is None:
        return None
    try:
        out = subprocess.run(
            ["nebula-cert", "print", "-path", str(path)],
            capture_output=True,
            text=True,
            timeout=2,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if out.returncode != 0:
        return None
    for line in out.stdout.splitlines():
        if "Ips:" in line:
            # Lines look like "Ips: [10.42.0.5/16]"
            body = line.split("Ips:", 1)[1].strip()
            body = body.strip("[]")
            ip_with_mask = body.split(",")[0].strip()
            ip = ip_with_mask.split("/")[0]
            if ip:
                return ip
    return None


def lighthouse_addresses(
    lighthouse_config_path: Optional[Path] = None,
) -> list[str]:
    """NF-13.6 — return the list of lighthouse overlay IPs from
    the local nebula config. Empty list when no config exists.
    Pure read.
    """
    path = lighthouse_config_path or LIGHTHOUSE_CONFIG_PATH
    if not path.exists():
        # Try the regular config (peer-role) instead.
        alt = CONFIG_DIR / "config.yaml"
        if not alt.exists():
            return []
        path = alt
    try:
        body = path.read_text()
    except OSError:
        return []
    return _extract_lighthouse_hosts(body)


def _extract_lighthouse_hosts(yaml_body: str) -> list[str]:
    """Pure helper — pulls IPv4 strings from the
    `lighthouse.hosts:` block of a nebula config YAML.

    Not a full YAML parser; tolerates the shape the
    nebula_supervisor's `render_config_yaml` emits:

        lighthouse:
          am_lighthouse: false
          hosts:
            - "10.42.0.1"
            - "10.42.0.2"
    """
    out: list[str] = []
    inside_hosts = False
    for raw in yaml_body.splitlines():
        line = raw.rstrip()
        if line.startswith("lighthouse:"):
            inside_hosts = False
            continue
        if "hosts:" in line and inside_hosts is False:
            inside_hosts = True
            continue
        if inside_hosts:
            stripped = line.strip()
            if stripped.startswith("- "):
                ip = stripped[2:].strip().strip('"').strip("'")
                if ip:
                    out.append(ip)
            elif stripped and not stripped.startswith("-"):
                # Left the hosts list — next key starts.
                inside_hosts = False
    return out


# ─────────────────────────────────────────────────────────────────
# Write helpers (privileged, expect pkexec / root)
# ─────────────────────────────────────────────────────────────────

def write_sshd_overlay_bind(
    overlay_ip: str,
    dropin_path: Optional[Path] = None,
) -> Path:
    """NF-13.1 — write the sshd_config.d drop-in that binds the
    SSH daemon to the nebula overlay IP. Replaces any existing
    file with identical contents (idempotent — the supervisor
    re-runs this on every overlay-IP change, which is rare;
    only on re-enrollment under a new CA epoch).

    Returns the path written.

    Raises OSError on permission failure (caller is expected to
    invoke under pkexec / inside a privileged systemd unit).
    """
    path = dropin_path or SSHD_DROPIN_PATH
    path.parent.mkdir(parents=True, exist_ok=True)
    body = (
        "# Generated by mackes/mesh_nebula.py (NF-13.1)\n"
        "# Do not edit by hand — the supervisor rewrites this\n"
        "# on every overlay-IP change.\n"
        f"ListenAddress {overlay_ip}\n"
        "# Per the open-mesh directive (2026-05-23): every\n"
        "# enrolled peer sees every other on the overlay; no\n"
        "# per-service ACL splits here.\n"
    )
    # Atomic write via temp + rename so a sshd reload mid-write
    # doesn't see a half-formed config.
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(body)
    tmp.replace(path)
    return path


def reload_sshd() -> int:
    """NF-13.1 — best-effort `systemctl reload sshd` after a
    drop-in change. Returns the exit code; 0 on success. Non-
    zero exits get swallowed by the caller (the supervisor) so
    a failed reload doesn't kill the worker.
    """
    if shutil.which("systemctl") is None:
        return 1
    return subprocess.call(["systemctl", "reload", "sshd"])


# ─────────────────────────────────────────────────────────────────
# WoL via lighthouse relay (NF-13.6 — new capability)
# ─────────────────────────────────────────────────────────────────

def wol_via_lighthouse(
    target_mac: str,
    lighthouse_ip: Optional[str] = None,
) -> int:
    """NF-13.6 — wake the peer with `target_mac` by sending the
    magic packet over the nebula overlay to a lighthouse, which
    de-encapsulates + re-broadcasts on the target's LAN segment.

    This is the new "WoL across LANs" capability the v2.5 cut
    enables — pre-Nebula, WoL only worked within a single
    broadcast domain.

    Implementation: shells out to `wakeonlan` (the canonical
    Fedora WoL utility) targeting the lighthouse's overlay IP.
    The lighthouse-side relay re-broadcasts on the target LAN
    via the static_host_map cached MAC address.

    Returns the wakeonlan exit code; 0 on success. Returns 2
    when no lighthouse can be reached (no IPs in
    `lighthouse_addresses()` and no override supplied).
    """
    if lighthouse_ip is None:
        candidates = lighthouse_addresses()
        if not candidates:
            return 2
        lighthouse_ip = candidates[0]
    if shutil.which("wakeonlan") is None:
        return 3
    return subprocess.call(
        ["wakeonlan", "-i", lighthouse_ip, target_mac]
    )


# ─────────────────────────────────────────────────────────────────
# Read-only status query for the panel / workbench
# ─────────────────────────────────────────────────────────────────

CANONICAL_SERVICES: tuple[tuple[str, str, int, str], ...] = (
    # (service-id, display-name, default-port, "tcp"|"udp")
    ("ssh", "SSH", 22, "tcp"),
    ("nats", "NATS broker", 4222, "tcp"),
    ("fs", "Mesh FS (SSHFS)", 22, "tcp"),
    ("media", "Media library", 8080, "tcp"),
    ("sync", "rsync", 873, "tcp"),
    ("wol", "Wake-on-LAN relay", 9, "udp"),
    ("av", "Audio/video transport", 5004, "udp"),
)


def published_services_summary() -> list[dict]:
    """NF-13.8 — return one row per canonical service for the
    new "Service Publishing" workbench panel. Each row carries
    the service id, display name, port + protocol, the overlay
    IP it would bind to (None when not yet enrolled), and a
    bench-observable "is_publishable" flag (true when both an
    overlay IP exists AND the service binary is on PATH).

    Pure read — no side effects.
    """
    overlay = current_overlay_ip()
    out: list[dict] = []
    for service_id, display, port, proto in CANONICAL_SERVICES:
        out.append({
            "id": service_id,
            "name": display,
            "port": port,
            "proto": proto,
            "overlay_ip": overlay,
            "is_publishable": overlay is not None,
        })
    return out


# ─────────────────────────────────────────────────────────────────
# NF-16 notification emitters (NF-21.4 migrated to Bus 2026-05-27)
# ─────────────────────────────────────────────────────────────────
#
# The 4 `emit_*` helpers now publish to `nebula/<event>` Bus
# topics via the `mde-bus publish` CLI per BUS-4.4 + Q20 + Q96.
# Bus owns notification routing; the legacy
# `~/.cache/mde/toasts.jsonl` path retired with this migration.
# Subscribers (mde-popover toasts applet, BUS-2.x surfaces) read
# from the per-topic file tree under `<XDG_DATA_HOME>/mde/bus/`.
# Failures are best-effort — missing `mde-bus` binary or shell-out
# error returns False without crashing the caller.


def _publish_to_bus(topic: str, priority: str, title: str, body: str = "") -> bool:
    """NF-21.4 (2026-05-27) — shell-out to `mde-bus publish` so
    Bus owns notification routing per Q20 + Q96 + BUS-4.4. Returns
    True on success, False on any subprocess error or missing
    binary (caller treats the publish as best-effort, just like
    the legacy `_emit_toast` path did for filesystem errors).

    Replaces the prior `~/.cache/mde/toasts.jsonl` append path —
    Bus is now the single authoritative routing layer; legacy
    consumers that polled the jsonl file should subscribe to
    `nebula/#` instead.
    """
    import subprocess  # noqa: PLC0415 — local import keeps the
                       # module-level surface small + avoids
                       # importing subprocess for callers that
                       # don't trigger an emit.
    try:
        result = subprocess.run(
            [
                "mde-bus", "publish", topic,
                "--priority", priority,
                "--title", title,
                "--body-flag", body,
                "--no-broker",
            ],
            check=False,
            capture_output=True,
            timeout=5,
        )
        return result.returncode == 0
    except (OSError, subprocess.SubprocessError):
        return False


def emit_lighthouse_event(promoted: bool) -> bool:
    """NF-16.1 — subtle informational publish on
    promotion/demotion to/from the lighthouse role. Publishes
    to `nebula/lighthouse` at default priority per NF-21.4.
    """
    if promoted:
        return _publish_to_bus(
            "nebula/lighthouse",
            "default",
            "Lighthouse active",
            "This peer is now serving as a lighthouse for the mesh.",
        )
    return _publish_to_bus(
        "nebula/lighthouse",
        "default",
        "Lighthouse stepped down",
        "This peer is no longer serving as a lighthouse.",
    )


def emit_ca_rotation(success: bool, error_detail: str = "") -> bool:
    """NF-16.2 — per-peer publish when the mesh CA rotates.
    Success path: default-priority publish confirming the new
    cert propagated. Failure path: high-priority publish pointing
    to the recovery doc. NF-21.4 routes to `nebula/ca-rotation`.
    """
    if success:
        return _publish_to_bus(
            "nebula/ca-rotation",
            "default",
            "Mesh CA rotated",
            "Your peer cert was re-issued under the new CA epoch.",
        )
    body = "See docs/help/mesh-recovery.md for recovery steps."
    if error_detail:
        body = f"{error_detail}\n\n{body}"
    return _publish_to_bus("nebula/ca-rotation", "high", "CA rotation failed", body)


def emit_https_fallback_state(active: bool) -> bool:
    """NF-16.3 — transition-only publish when the TCP/443
    fallback flips Active / Inactive. Honors the Q12 lock:
    transition event, not a persistent banner. NF-21.4 routes
    to `nebula/https-fallback`.
    """
    if active:
        return _publish_to_bus(
            "nebula/https-fallback",
            "high",
            "Mesh in firewall mode",
            "UDP path lost — falling over to TCP/443 (covert tunnel).",
        )
    return _publish_to_bus(
        "nebula/https-fallback",
        "default",
        "Direct UDP mesh restored",
        "Covert TCP/443 fallback stood down.",
    )


def emit_cert_expiry_warning(peer_name: str, days_remaining: int) -> bool:
    """NF-16.4 — early warning when a peer's cert is approaching
    expiry. < 24 h escalates to urgent priority; 1-7 days is
    high. NF-21.4 routes to `nebula/cert-expiry`.
    """
    if days_remaining < 1:
        return _publish_to_bus(
            "nebula/cert-expiry",
            "urgent",
            f"{peer_name} cert expired",
            "Re-enroll the peer or rotate the CA to restore reachability.",
        )
    if days_remaining <= 7:
        return _publish_to_bus(
            "nebula/cert-expiry",
            "high",
            f"{peer_name} cert expires in {days_remaining}d",
            "Plan a CA rotation or peer re-enrollment soon.",
        )
    # Already > 7 days — don't publish.
    return False


# ─────────────────────────────────────────────────────────────────
# NF-17 firewall + system surface
# ─────────────────────────────────────────────────────────────────

NEBULA_FIREWALL_PORTS: tuple[tuple[int, str], ...] = (
    (4242, "udp"),  # native Nebula
    (443, "tcp"),   # NF-1 covert TCP/443 fallback
)


def apply_nebula_firewall_preset() -> int:
    """NF-17.1 — one-click "Allow Nebula" firewalld preset.
    Opens UDP/4242 inbound + outbound and TCP/443 outbound
    on the default zone. Returns 0 on success; non-zero on
    firewall-cmd failure (caller surfaces a toast).

    Tailscale's UDP/41641 preset (the v1.x default) is NOT
    cleaned up here — leave existing rules alone so a peer
    migrating from Tailscale doesn't lose connectivity
    mid-flight. The mackesd cleanup pass retires the
    Tailscale preset in NF-6.x once the operator confirms
    the migration succeeded.
    """
    if shutil.which("firewall-cmd") is None:
        return 1
    rc = 0
    for port, proto in NEBULA_FIREWALL_PORTS:
        spec = f"{port}/{proto}"
        rc |= subprocess.call(
            ["firewall-cmd", "--permanent", "--add-port", spec],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    rc |= subprocess.call(
        ["firewall-cmd", "--reload"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return rc


# ─────────────────────────────────────────────────────────────────
# NF-13.2..13.7 peer-IP enumeration
# ─────────────────────────────────────────────────────────────────
#
# Canonical replacement for `tailscale status --json` parsing.
# Every NF-13 service publisher (mesh_nats, mesh_fs, mesh_media,
# mesh_sync, mesh_av) needs the (hostname, overlay-ip) tuple set
# to know which peers to broadcast / mount / serve to. This
# helper consults mded.Nebula.Status.ListPeers() via dbus-send
# subprocess + falls back to an empty list when the daemon isn't
# reachable.


def nebula_peer_ips() -> list[tuple[str, str]]:
    """NF-13.2..13.7 — return (hostname, overlay_ip) pairs for
    every reachable Nebula peer (excluding self).

    Implementation: shells out to `dbus-send` to call
    org.mackes.mackesd
    /dev/mackes/MDE/Nebula/Status
    dev.mackes.MDE.Nebula.Status.ListPeers() and parses the JSON
    reply. On any failure (dbus-send missing, daemon offline,
    JSON parse error) returns an empty list so callers fall
    back to their legacy enumeration path during the migration
    window.
    """
    if shutil.which("dbus-send") is None:
        return []
    try:
        out = subprocess.run(
            [
                "dbus-send", "--session", "--print-reply=literal",
                "--dest=org.mackes.mackesd",
                "/dev/mackes/MDE/Nebula/Status",
                "dev.mackes.MDE.Nebula.Status.ListPeers",
            ],
            capture_output=True, text=True, timeout=2, check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return []
    if out.returncode != 0:
        return []
    # dbus-send --print-reply=literal emits the JSON as a single
    # string with whitespace; locate the array.
    raw = out.stdout.strip()
    if not raw:
        return []
    # Strip any leading `string "` wrapper dbus-send may add.
    if raw.startswith('string "'):
        raw = raw[len('string "'):]
        if raw.endswith('"'):
            raw = raw[:-1]
    raw = raw.encode("latin-1").decode("unicode_escape", errors="ignore")
    try:
        peers = json.loads(raw)
    except (ValueError, json.JSONDecodeError):
        return []
    if not isinstance(peers, list):
        return []
    out_pairs: list[tuple[str, str]] = []
    for p in peers:
        if not isinstance(p, dict):
            continue
        name = p.get("name") or p.get("node_id")
        ip = p.get("overlay_ip")
        if isinstance(name, str) and isinstance(ip, str) and name and ip:
            out_pairs.append((name, ip))
    return out_pairs


def bind_target_for(service_id: str) -> str | None:
    """NF-13.2..13.7 — return the overlay IP this peer should bind
    `<service>` to, or None when no overlay IP exists yet (the
    service stays unbound until enrollment completes). Same
    behavior for every service today; future-proofed via the
    service_id parameter so per-service overrides can land
    without touching the consumer.
    """
    _ = service_id  # reserved for future per-service overrides
    return current_overlay_ip()


__all__ = [
    "CANONICAL_SERVICES",
    "CONFIG_DIR",
    "HOST_CERT_PATH",
    "LIGHTHOUSE_CONFIG_PATH",
    "SSHD_DROPIN_DIR",
    "SSHD_DROPIN_PATH",
    "current_overlay_ip",
    "emit_ca_rotation",
    "emit_cert_expiry_warning",
    "emit_https_fallback_state",
    "emit_lighthouse_event",
    "lighthouse_addresses",
    "published_services_summary",
    "reload_sshd",
    "wol_via_lighthouse",
    "write_sshd_overlay_bind",
    "_extract_lighthouse_hosts",
    "NEBULA_FIREWALL_PORTS",
    "apply_nebula_firewall_preset",
    "bind_target_for",
    "nebula_peer_ips",
]
