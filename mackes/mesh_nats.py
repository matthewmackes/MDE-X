"""NATS JetStream backend for mesh_sync (#5).

Upgrades the filesystem-polling bucket sync to a streaming publish/
subscribe model:

  * Control peer runs an embedded `nats-server` with JetStream on
    :4222 (open-source, Apache 2.0, github.com/nats-io/nats-server).
  * Every Mackes peer is a client. mesh_sync.put() ALSO publishes
    to the JetStream `mackes-buckets` stream; subscribers see writes
    in sub-100 ms instead of waiting for the next 30 s SSHFS scan.

Backwards-compatibility strategy: we DON'T remove the filesystem
path. Both write paths run; the filesystem stays as the durable
canonical store, NATS is the fast notification channel. This means
peers running older Mackes (no NATS) still see writes via the
sshfs scan; peers with NATS see them instantly.

Dependencies (soft — code degrades gracefully when absent):
  * nats-server (binary, can be downloaded to /usr/local/bin/)
  * nats-py (`pip install nats-py` or `dnf install python3-nats`)

Public API:

  is_server_installed()    → bool
  is_server_running()      → bool
  install_server()         → list[str]
  is_client_available()    → bool   (nats-py importable)
  publish_event(bucket, key, value_summary)  → bool
  start_subscriber(on_event)  → callable to stop
  status()                 → dict
"""
from __future__ import annotations

import json
import shutil
import subprocess
import threading
import time
import urllib.request
from pathlib import Path
from typing import Callable, Optional


NATS_BIN = Path("/usr/local/bin/nats-server")
NATS_VERSION = "2.10.22"
NATS_URL = (
    f"https://github.com/nats-io/nats-server/releases/download/"
    f"v{NATS_VERSION}/nats-server-v{NATS_VERSION}-linux-amd64.tar.gz"
)
NATS_DATA = Path("/var/lib/mackes-nats")
NATS_PORT = 4222
NATS_MONITOR_PORT = 8222
NATS_UNIT = Path("/etc/systemd/system/mackes-nats.service")
NATS_STREAM = "mackes-buckets"
NATS_SUBJECT_PREFIX = "mackes.buckets"


# ---------------------------------------------------------------------------
# Server-side: install + run nats-server with JetStream
# ---------------------------------------------------------------------------


def is_server_installed() -> bool:
    return NATS_BIN.is_file() and NATS_BIN.stat().st_mode & 0o100


def is_server_running() -> bool:
    if shutil.which("systemctl") is None:
        return False
    try:
        r = subprocess.run(
            ["systemctl", "is-active", "mackes-nats"],
            capture_output=True, text=True, timeout=4,
        )
        return r.returncode == 0 and r.stdout.strip() == "active"
    except (OSError, subprocess.TimeoutExpired):
        return False


def install_server(*, control_ip: str = "127.0.0.1") -> list[str]:
    """Download nats-server, write a JetStream config, install systemd
    unit, start. Idempotent on re-run."""
    from mackes.admin_session import AdminSession
    actions: list[str] = []

    if not is_server_installed():
        if shutil.which("curl") is None or shutil.which("tar") is None:
            return ["nats: curl + tar required"]
        import tempfile
        with tempfile.TemporaryDirectory(prefix="mackes-nats-") as td:
            tgz = Path(td) / "nats.tgz"
            r = subprocess.run(
                ["curl", "-fsSL", "-o", str(tgz), NATS_URL],
                capture_output=True, timeout=300,
            )
            if r.returncode != 0:
                return [f"nats: download failed (rc={r.returncode})"]
            r = subprocess.run(
                ["tar", "-xzf", str(tgz), "-C", td],
                capture_output=True, timeout=30,
            )
            if r.returncode != 0:
                return [f"nats: extract failed (rc={r.returncode})"]
            # Find the binary in the extracted dir
            built: Optional[Path] = None
            for p in Path(td).rglob("nats-server"):
                if p.is_file() and p.stat().st_mode & 0o100:
                    built = p; break
            if built is None:
                return ["nats: binary not found in tarball"]
            rc, out = AdminSession.instance().run(
                ["install", "-D", "-m", "0755", str(built), str(NATS_BIN)],
                timeout=10,
            )
            if rc != 0:
                return [f"nats: install failed: {out}"]
            actions.append(f"nats-server installed → {NATS_BIN}")
    else:
        actions.append(f"nats: {NATS_BIN} already present")

    # Config file with JetStream enabled
    cfg = _server_config(control_ip=control_ip)
    import tempfile
    with tempfile.NamedTemporaryFile(mode="w", delete=False,
                                      suffix=".conf",
                                      encoding="utf-8") as tmp:
        tmp.write(cfg)
        tmp_cfg = tmp.name
    rc, out = AdminSession.instance().run(
        ["install", "-D", "-m", "0644", tmp_cfg,
         "/etc/mackes-nats/nats.conf"], timeout=5,
    )
    Path(tmp_cfg).unlink(missing_ok=True)
    actions.append("nats: wrote /etc/mackes-nats/nats.conf")

    # systemd unit
    unit = _unit_payload()
    with tempfile.NamedTemporaryFile(mode="w", delete=False,
                                      suffix=".service",
                                      encoding="utf-8") as tmp:
        tmp.write(unit)
        tmp_unit = tmp.name
    AdminSession.instance().run(
        ["install", "-D", "-m", "0644", tmp_unit, str(NATS_UNIT)],
        timeout=5,
    )
    Path(tmp_unit).unlink(missing_ok=True)
    AdminSession.instance().run(["systemctl", "daemon-reload"], timeout=5)
    rc, out = AdminSession.instance().run(
        ["systemctl", "enable", "--now", "mackes-nats.service"],
        timeout=15,
    )
    if rc == 0:
        actions.append("nats: server enabled + started")
    else:
        actions.append(f"nats: start failed: {out.strip()[:200]}")
    return actions


def _server_config(*, control_ip: str) -> str:
    return (
        f"# Mackes NATS — autogenerated\n"
        f"port: {NATS_PORT}\n"
        f"http_port: {NATS_MONITOR_PORT}\n"
        f"server_name: mackes-control\n"
        f"max_payload: 8MB\n"
        f"\n"
        f"jetstream {{\n"
        f"  store_dir: {NATS_DATA}\n"
        f"  max_memory_store: 256MB\n"
        f"  max_file_store: 4GB\n"
        f"}}\n"
        f"\n"
        f"# No auth for v1 — assumes mesh-only access. v2 will add\n"
        f"# tls + per-peer NKey credentials.\n"
    )


def _unit_payload() -> str:
    return (
        "[Unit]\n"
        "Description=Mackes NATS JetStream server\n"
        "After=network-online.target\n"
        "Wants=network-online.target\n\n"
        "[Service]\n"
        "Type=simple\n"
        f"ExecStart={NATS_BIN} -c /etc/mackes-nats/nats.conf\n"
        f"StateDirectory=mackes-nats\n"
        f"WorkingDirectory={NATS_DATA}\n"
        "Restart=on-failure\n"
        "RestartSec=5\n"
        "NoNewPrivileges=true\n\n"
        "[Install]\n"
        "WantedBy=multi-user.target\n"
    )


def uninstall_server() -> list[str]:
    from mackes.admin_session import AdminSession
    AdminSession.instance().run(
        ["systemctl", "disable", "--now", "mackes-nats.service"],
        timeout=10,
    )
    return ["nats: stopped + disabled"]


# ---------------------------------------------------------------------------
# Client-side: publish + subscribe
# ---------------------------------------------------------------------------


def is_client_available() -> bool:
    try:
        import nats  # noqa: F401
        return True
    except ImportError:
        return False


def _control_url() -> str:
    """NATS connect URL — points at the mesh's control peer."""
    try:
        from mackes.mesh_vpn import MeshState, headscale_list_peers
        state = MeshState.load()
        cid = state.control_peer_id
        if cid:
            for p in headscale_list_peers():
                if getattr(p, "peer_id", "") == cid:
                    ip = getattr(p, "mesh_ip", "") or getattr(p, "ip", "")
                    if ip:
                        return f"nats://{ip}:{NATS_PORT}"
    except Exception:  # noqa: BLE001
        pass
    # Fallback: connect to localhost (we ARE the control peer)
    return f"nats://127.0.0.1:{NATS_PORT}"


def publish_event(bucket: str, key: str, *,
                  value_summary: str = "", peer: str = "") -> bool:
    """Publish a bucket-change event over NATS. Returns True on send.

    Subject: mackes.buckets.<bucket>.<peer>.<key>
    Payload: JSON {bucket, key, peer, summary, ts}

    Cheap fire-and-forget: if nats-py isn't installed or the server
    isn't reachable, returns False and callers continue with the
    filesystem-only path."""
    if not is_client_available():
        return False
    if not peer:
        import socket as _s
        peer = _s.gethostname().split(".", 1)[0]
    try:
        import asyncio
        import nats as _nats

        async def _send():
            nc = await _nats.connect(_control_url(),
                                     connect_timeout=2.0,
                                     max_reconnect_attempts=1)
            subject = f"{NATS_SUBJECT_PREFIX}.{bucket}.{peer}.{key}"
            payload = json.dumps({
                "bucket": bucket, "key": key, "peer": peer,
                "summary": value_summary, "ts": time.time(),
            }).encode("utf-8")
            await nc.publish(subject, payload)
            await nc.flush(timeout=2.0)
            await nc.close()

        # Run on this thread synchronously
        try:
            asyncio.run(_send())
            return True
        except RuntimeError:
            # Already in an event loop (rare in mesh_sync context but
            # protect anyway) — fall through to False rather than crash.
            return False
    except Exception:  # noqa: BLE001
        return False


# ---------------------------------------------------------------------------
# Subscriber (runs on a background thread)
# ---------------------------------------------------------------------------


def start_subscriber(on_event: Callable[[dict], None]) -> Callable[[], None]:
    """Start a background subscriber thread; returns a callable that
    stops it. Each delivered event is parsed JSON {bucket, key, peer,
    summary, ts} and passed to on_event."""
    if not is_client_available():
        return lambda: None

    stop_flag = threading.Event()
    thread = threading.Thread(
        target=_subscriber_loop,
        args=(on_event, stop_flag),
        daemon=True,
        name="mesh-nats-sub",
    )
    thread.start()
    return stop_flag.set


def _subscriber_loop(on_event, stop_flag) -> None:
    import asyncio
    import nats as _nats

    async def _loop():
        backoff = 1.0
        while not stop_flag.is_set():
            try:
                nc = await _nats.connect(_control_url(),
                                         connect_timeout=2.0,
                                         max_reconnect_attempts=3)
                async def cb(msg):
                    try:
                        on_event(json.loads(msg.data.decode("utf-8")))
                    except (ValueError, UnicodeDecodeError):
                        pass
                await nc.subscribe(f"{NATS_SUBJECT_PREFIX}.>", cb=cb)
                backoff = 1.0
                while not stop_flag.is_set():
                    await asyncio.sleep(0.5)
                await nc.close()
                return
            except Exception:  # noqa: BLE001
                # Reconnect with exponential backoff
                await asyncio.sleep(min(backoff, 30))
                backoff *= 2.0
    try:
        asyncio.run(_loop())
    except Exception:  # noqa: BLE001
        pass


# ---------------------------------------------------------------------------
# Status
# ---------------------------------------------------------------------------


def status() -> dict:
    out = {
        "server_installed": is_server_installed(),
        "server_running":   is_server_running(),
        "client_available": is_client_available(),
        "port":             NATS_PORT,
        "monitor_port":     NATS_MONITOR_PORT,
        "url":              _control_url(),
    }
    if out["server_running"]:
        try:
            with urllib.request.urlopen(
                f"http://127.0.0.1:{NATS_MONITOR_PORT}/jsz",
                timeout=2,
            ) as resp:
                jsz = json.loads(resp.read().decode("utf-8"))
                out["jetstream_streams"] = jsz.get("streams", 0)
                out["jetstream_messages"] = jsz.get("messages", 0)
        except Exception:  # noqa: BLE001
            pass
    return out


__all__ = [
    "is_server_installed", "is_server_running", "install_server",
    "uninstall_server", "is_client_available",
    "publish_event", "start_subscriber", "status",
    "NATS_PORT", "NATS_VERSION", "NATS_SUBJECT_PREFIX",
]
