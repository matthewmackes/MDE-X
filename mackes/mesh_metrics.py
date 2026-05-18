"""Prometheus metrics for the Mackes mesh.

Open-source tooling:
  * prometheus-wireguard-exporter (Rust, MIT) —
    github.com/MindFlavor/prometheus_wireguard_exporter
    exposes per-peer transfer / handshake / endpoint metrics on a
    user-configurable HTTP port (default 9586).
  * prometheus (Apache 2.0) — the standard scraper.

Layout we ship:
  Every peer:    `prometheus-wireguard-exporter` listening on
                 127.0.0.1:9586 (or :9586 inside the mesh).
  Control peer:  full `prometheus` server scraping every peer.

Public API:

  install_exporter()    install the exporter binary + user systemd unit
  is_exporter_running()
  exporter_metrics()    one-shot fetch of the current metrics page
  install_prometheus_control()    only on the control peer
  prometheus_status()
"""
from __future__ import annotations

import shutil
import subprocess
import urllib.error
import urllib.request
from pathlib import Path
from typing import Optional


# Where we install the exporter — under /usr/local because Fedora
# doesn't package it; the binary comes from the upstream release.
EXPORTER_BIN = Path("/usr/local/bin/prometheus-wireguard-exporter")
EXPORTER_VERSION = "3.6.7"   # latest stable as of 2026-05
EXPORTER_URL = (
    "https://github.com/MindFlavor/prometheus_wireguard_exporter/"
    f"releases/download/{EXPORTER_VERSION}/"
    f"prometheus_wireguard_exporter-{EXPORTER_VERSION}-x86_64-linux"
)
EXPORTER_PORT = 9586

# User systemd unit (one per peer)
USER_UNITDIR = Path.home() / ".config/systemd/user"
EXPORTER_UNIT = USER_UNITDIR / "mackes-wg-exporter.service"

PROMETHEUS_CONFIG_DIR = Path("/etc/mackes-prometheus")
PROMETHEUS_CONFIG = PROMETHEUS_CONFIG_DIR / "prometheus.yml"


# ---------------------------------------------------------------------------
# Per-peer: WireGuard exporter
# ---------------------------------------------------------------------------


def is_exporter_installed() -> bool:
    return EXPORTER_BIN.is_file() and EXPORTER_BIN.stat().st_mode & 0o100


def is_exporter_running() -> bool:
    if shutil.which("systemctl") is None:
        return False
    try:
        r = subprocess.run(
            ["systemctl", "--user", "is-active", "mackes-wg-exporter"],
            capture_output=True, text=True, timeout=4,
        )
        return r.returncode == 0 and r.stdout.strip() == "active"
    except (OSError, subprocess.TimeoutExpired):
        return False


def install_exporter() -> list[str]:
    """Download + install the upstream prometheus-wireguard-exporter
    binary, then write + start the user systemd unit.

    Idempotent: skips the download when the binary is already present
    at the expected version.
    """
    from mackes.admin_session import AdminSession
    actions: list[str] = []
    if not is_exporter_installed():
        if shutil.which("curl") is None:
            return ["curl missing; cannot install exporter"]
        import tempfile
        with tempfile.NamedTemporaryFile(delete=False) as tmp:
            tmp_path = tmp.name
        rc, _ = _run(["curl", "-fsSL", "-o", tmp_path, EXPORTER_URL],
                     timeout=300)
        if rc != 0:
            Path(tmp_path).unlink(missing_ok=True)
            return [f"exporter download failed (rc={rc})"]
        rc, out = AdminSession.instance().run(
            ["install", "-D", "-m", "0755", tmp_path, str(EXPORTER_BIN)],
            timeout=10,
        )
        Path(tmp_path).unlink(missing_ok=True)
        if rc != 0:
            return [f"exporter install failed: {out}"]
        actions.append(f"exporter: installed {EXPORTER_BIN}")
    else:
        actions.append(f"exporter: {EXPORTER_BIN} already present")

    # Write user unit
    USER_UNITDIR.mkdir(parents=True, exist_ok=True)
    EXPORTER_UNIT.write_text(_unit_payload(), encoding="utf-8")
    actions.append(f"exporter: wrote {EXPORTER_UNIT}")
    subprocess.run(["systemctl", "--user", "daemon-reload"],
                   capture_output=True, timeout=10)
    r = subprocess.run(
        ["systemctl", "--user", "enable", "--now",
         "mackes-wg-exporter.service"],
        capture_output=True, text=True, timeout=10,
    )
    if r.returncode == 0:
        actions.append("exporter: enabled + started")
    else:
        actions.append(f"exporter: enable failed: {r.stderr.strip()}")
    return actions


def uninstall_exporter() -> list[str]:
    subprocess.run(
        ["systemctl", "--user", "disable", "--now",
         "mackes-wg-exporter.service"],
        capture_output=True, timeout=10,
    )
    if EXPORTER_UNIT.exists():
        try:
            EXPORTER_UNIT.unlink()
        except OSError:
            pass
    return ["exporter: stopped + unit removed (binary left in place)"]


def _unit_payload() -> str:
    # Exporter is a per-user service so a non-root install can run it.
    # It calls `wg show all dump` which requires CAP_NET_ADMIN — we
    # gate that via a sudoers rule in apply_prometheus_metrics.
    return f"""[Unit]
Description=Mackes prometheus-wireguard-exporter
After=network-online.target tailscaled.service
Wants=network-online.target

[Service]
Type=simple
ExecStart={EXPORTER_BIN} \\
    --port {EXPORTER_PORT} \\
    --extract_names_config_files /etc/wireguard/wg0.conf
Restart=on-failure
RestartSec=5
Nice=10

[Install]
WantedBy=default.target
"""


def exporter_metrics(timeout: float = 3.0) -> Optional[str]:
    """Fetch the current /metrics page from this peer's exporter.
    Returns None on any error."""
    url = f"http://127.0.0.1:{EXPORTER_PORT}/metrics"
    try:
        with urllib.request.urlopen(url, timeout=timeout) as resp:
            return resp.read().decode("utf-8", errors="replace")
    except (urllib.error.URLError, OSError):
        return None


def parsed_per_peer_metrics() -> dict[str, dict[str, float]]:
    """Parse /metrics text into {peer_label: {metric_name: value}}.

    Lines look like:
      wireguard_sent_bytes_total{interface="wg0",public_key="…",
                                  allowed_ips="…",friendly_name="alpha"} 12345
    """
    text = exporter_metrics()
    if text is None:
        return {}
    out: dict[str, dict[str, float]] = {}
    for line in text.splitlines():
        if line.startswith("#") or not line.strip():
            continue
        # name{labels} value
        if "{" not in line:
            continue
        name, rest = line.split("{", 1)
        labels_part, _, value_part = rest.rpartition("}")
        try:
            value = float(value_part.strip())
        except ValueError:
            continue
        labels: dict[str, str] = {}
        for chunk in labels_part.split(","):
            if "=" in chunk:
                k, _, v = chunk.partition("=")
                labels[k.strip()] = v.strip().strip('"')
        peer = labels.get("friendly_name") or labels.get("public_key", "")[:12]
        if not peer:
            continue
        out.setdefault(peer, {})[name.strip()] = value
    return out


# ---------------------------------------------------------------------------
# Control peer: Prometheus server
# ---------------------------------------------------------------------------


def install_prometheus_control() -> list[str]:
    """Install prometheus via dnf + write a scrape config that pulls
    from every mesh peer's exporter."""
    from mackes.admin_session import AdminSession
    if shutil.which("dnf") is None:
        return ["dnf missing; cannot install prometheus"]
    rc, out = AdminSession.instance().run(
        ["dnf", "install", "-y", "golang-github-prometheus"], timeout=600,
    )
    if rc != 0:
        return [f"prometheus install failed: {out.strip().splitlines()[-1] if out else rc}"]
    return ["prometheus: installed (configure scrape targets via "
            "mesh_metrics.write_scrape_config())"]


def write_scrape_config(peer_ips: list[str]) -> list[str]:
    """Render and write the prometheus scrape config that targets
    every peer's :9586. Idempotent."""
    from mackes.admin_session import AdminSession
    cfg = "\n".join([
        "# Mackes mesh — autogenerated by mackes.mesh_metrics",
        "global:",
        "  scrape_interval: 30s",
        "  evaluation_interval: 60s",
        "",
        "scrape_configs:",
        "  - job_name: mackes-mesh-wireguard",
        "    static_configs:",
        "      - targets:",
        *[f"        - '{ip}:{EXPORTER_PORT}'" for ip in peer_ips],
        "        labels:",
        "          mesh: 'mackes'",
        "",
    ])
    import tempfile
    with tempfile.NamedTemporaryFile(mode="w", delete=False,
                                      suffix=".yml",
                                      encoding="utf-8") as tmp:
        tmp.write(cfg)
        tmp_path = tmp.name
    rc, out = AdminSession.instance().run(
        ["install", "-D", "-m", "0644", tmp_path,
         str(PROMETHEUS_CONFIG)],
        timeout=10,
    )
    Path(tmp_path).unlink(missing_ok=True)
    if rc != 0:
        return [f"prometheus: scrape config write failed: {out}"]
    return [f"prometheus: scrape config written to {PROMETHEUS_CONFIG} "
            f"with {len(peer_ips)} target(s)"]


def prometheus_status() -> dict:
    """One-shot status for the Mesh Performance panel."""
    return {
        "exporter_installed": is_exporter_installed(),
        "exporter_running":   is_exporter_running(),
        "exporter_url":       f"http://127.0.0.1:{EXPORTER_PORT}/metrics",
        "control_installed":  bool(shutil.which("prometheus")),
        "control_config":     str(PROMETHEUS_CONFIG) if PROMETHEUS_CONFIG.exists() else "",
    }


# ---------------------------------------------------------------------------
# Local helpers
# ---------------------------------------------------------------------------


def _run(cmd, *, timeout=60):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True,
                           timeout=timeout)
        return r.returncode, (r.stdout or "") + (r.stderr or "")
    except (OSError, subprocess.TimeoutExpired) as e:
        return 1, str(e)


__all__ = [
    "install_exporter", "uninstall_exporter",
    "is_exporter_installed", "is_exporter_running",
    "exporter_metrics", "parsed_per_peer_metrics",
    "install_prometheus_control", "write_scrape_config",
    "prometheus_status",
    "EXPORTER_PORT", "EXPORTER_VERSION",
]
