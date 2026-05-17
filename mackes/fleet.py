"""Mesh Fleet — Ansible-pull driver + inventory + run-history.

v1.3.0 design locks (10-question survey, 2026-05-17):
  1. Transport: ansible-pull on every peer (no central controller for steady-state).
  2. Playbook store: QNM-Shared/.qnm-sync/playbooks/ (file-substrate replication).
  3. Schedule: systemd timer, OnBootSec=10m / OnUnitActiveSec=30m / RandomizedDelaySec=5m.
  4. Editor: read-only browser + 'Open in $EDITOR'.
  5. Secrets: none (plaintext playbooks).
  6. Run history: last 30 days, one JSON per run at
     QNM-Shared/.qnm-sync/ansible-runs/<peer>/<ts>.json.
  7. Ad-hoc runs: yes — SSH-push from the Inventory panel.

Public API:

  build_inventory()          → list[FleetPeer] from Headscale + QNM-Mesh
  list_playbooks()           → list[Playbook]
  run_local_pull(tags)       → triggers mackes-ansible-pull.service
  run_push(peers, tags)      → SSH-push ansible-playbook to selected peers
  list_runs(peer=None,
            playbook=None,
            since=None)      → list[RunRecord] across the mesh
  prune_runs(days=30)        → drop run JSONs older than `days`
  open_playbook_in_editor(p) → xdg-open the playbook's tasks/main.yml
  current_peer_name()        → this peer's identity (mesh hostname)
"""
from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable, List, Optional


# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------


def _home() -> Path:
    return Path(os.path.expanduser("~"))


def _qnm_shared() -> Path:
    return _home() / "QNM-Shared"


def playbooks_dir() -> Path:
    """Active playbook tree (replicated across the mesh by QNM-Shared)."""
    return _qnm_shared() / ".qnm-sync" / "playbooks"


def runs_dir() -> Path:
    return _qnm_shared() / ".qnm-sync" / "ansible-runs"


def peer_runs_dir(peer: str) -> Path:
    return runs_dir() / peer


def current_peer_name() -> str:
    """This peer's identity (matches Headscale node name)."""
    # Prefer the Mackes-managed peer-id if available.
    state_file = Path("/etc/mackes-shell/state.json")
    try:
        if state_file.exists():
            data = json.loads(state_file.read_text(encoding="utf-8"))
            pid = data.get("peer_id")
            if pid:
                return str(pid)
    except (OSError, json.JSONDecodeError):
        pass
    return socket.gethostname()


# ---------------------------------------------------------------------------
# Data model
# ---------------------------------------------------------------------------


@dataclass
class FleetPeer:
    name: str
    mesh_ip: str = ""
    online: bool = False
    last_pull_at: Optional[float] = None     # unix ts (mtime of latest run JSON)
    last_pull_ok: Optional[bool] = None      # True/False, None=never run
    pulls_24h: int = 0


@dataclass
class Playbook:
    """A role under playbooks/roles/. Treated as a single runnable unit."""
    name: str
    description: str
    path: Path
    tags: List[str] = field(default_factory=list)

    @property
    def main_task_path(self) -> Path:
        return self.path / "tasks" / "main.yml"


@dataclass
class RunRecord:
    peer: str
    timestamp: float        # unix ts
    playbook: str           # role / tag name; "site" for full apply
    exit_code: int
    changed: int
    ok: int
    failed: int
    duration_s: float
    log_tail: str           # last ~16 lines of stdout
    triggered_by: str       # "pull" | "push" | "manual"


# ---------------------------------------------------------------------------
# Inventory — derives from Headscale, falls back to QNM-Mesh peer subdirs
# ---------------------------------------------------------------------------


def build_inventory() -> List[FleetPeer]:
    """Compose the live fleet inventory."""
    raw_peers: List[tuple[str, str, bool]] = []
    try:
        from mackes.mesh_vpn import headscale_list_peers
        for p in headscale_list_peers():
            raw_peers.append((p.name, p.mesh_ip or "", bool(p.online)))
    except Exception:  # noqa: BLE001
        pass
    if not raw_peers:
        mesh_root = _home() / "QNM-Mesh"
        if mesh_root.is_dir():
            for d in mesh_root.iterdir():
                if d.is_dir():
                    raw_peers.append((d.name, "", False))
    # If we're standalone (no mesh at all), at least show ourselves.
    if not raw_peers:
        raw_peers = [(current_peer_name(), "127.0.0.1", True)]

    out: List[FleetPeer] = []
    for name, ip, online in raw_peers:
        out.append(_fleet_peer_with_run_meta(name, ip, online))
    # Sort: this peer first, then online peers, then offline alphabetical.
    me = current_peer_name()
    out.sort(key=lambda p: (p.name != me, not p.online, p.name.lower()))
    return out


def _fleet_peer_with_run_meta(name: str, mesh_ip: str, online: bool) -> FleetPeer:
    peer_dir = peer_runs_dir(name)
    last_pull_at: Optional[float] = None
    last_pull_ok: Optional[bool] = None
    pulls_24h = 0
    cutoff_24h = time.time() - 86400
    if peer_dir.is_dir():
        try:
            runs = sorted(peer_dir.glob("*.json"))
            if runs:
                latest = runs[-1]
                last_pull_at = latest.stat().st_mtime
                try:
                    data = json.loads(latest.read_text(encoding="utf-8"))
                    last_pull_ok = (int(data.get("exit_code", 0)) == 0)
                except (OSError, json.JSONDecodeError):
                    last_pull_ok = None
            pulls_24h = sum(1 for r in runs if r.stat().st_mtime > cutoff_24h)
        except OSError:
            pass
    return FleetPeer(
        name=name, mesh_ip=mesh_ip, online=online,
        last_pull_at=last_pull_at, last_pull_ok=last_pull_ok,
        pulls_24h=pulls_24h,
    )


# ---------------------------------------------------------------------------
# Playbook discovery
# ---------------------------------------------------------------------------


_PLAYBOOK_DESCRIPTIONS = {
    "system-update":              "dnf upgrade -y --refresh — full system update",
    "bloat-removal":              "Apply preset.apps.remove_bloat (idempotent)",
    "apps-install":               "Apply preset.apps.install + ensure IBM Plex fonts",
    "xfconf-baseline":            "Re-apply the active preset's xfconf state — corrects drift",
    "mesh-state-snapshot":        "Capture a Mackes snapshot on every peer",
    "selinux-permissive-toggle":  "Toggle SELinux between enforcing and permissive",
    "container-runtime-setup":    "Install podman + buildah + skopeo + toolbox",
}


def list_playbooks() -> List[Playbook]:
    """List every role under the active playbook tree."""
    roles_dir = playbooks_dir() / "roles"
    if not roles_dir.is_dir():
        return []
    out: List[Playbook] = []
    for entry in sorted(roles_dir.iterdir()):
        if not entry.is_dir():
            continue
        desc = _PLAYBOOK_DESCRIPTIONS.get(entry.name,
                                          f"Custom role at {entry}")
        out.append(Playbook(
            name=entry.name,
            description=desc,
            path=entry,
            tags=_tags_for(entry.name),
        ))
    return out


def _tags_for(role_name: str) -> List[str]:
    """Mirror the tag wiring in site.yml."""
    default = {"system-update": ["update", "never"],
               "mesh-state-snapshot": ["snapshot", "never"],
               "selinux-permissive-toggle": ["selinux", "never"],
               "container-runtime-setup": ["containers", "never"],
               "xfconf-baseline": ["xfconf", "default"],
               "bloat-removal": ["bloat", "default"],
               "apps-install": ["apps", "default"]}
    return default.get(role_name, [role_name])


# ---------------------------------------------------------------------------
# Run history
# ---------------------------------------------------------------------------


def list_runs(*, peer: Optional[str] = None,
              playbook: Optional[str] = None,
              since: Optional[float] = None,
              limit: int = 200) -> List[RunRecord]:
    """Walk QNM-Shared/.qnm-sync/ansible-runs/ and return decoded records.

    Newest first.
    """
    out: List[RunRecord] = []
    root = runs_dir()
    if not root.is_dir():
        return out
    peer_dirs = [root / peer] if peer else [d for d in root.iterdir() if d.is_dir()]
    for pdir in peer_dirs:
        if not pdir.is_dir():
            continue
        for f in pdir.glob("*.json"):
            try:
                if since is not None and f.stat().st_mtime < since:
                    continue
                data = json.loads(f.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                continue
            rec = RunRecord(
                peer=str(data.get("peer", pdir.name)),
                timestamp=float(data.get("timestamp", f.stat().st_mtime)),
                playbook=str(data.get("playbook", "site")),
                exit_code=int(data.get("exit_code", 0)),
                changed=int(data.get("changed", 0)),
                ok=int(data.get("ok", 0)),
                failed=int(data.get("failed", 0)),
                duration_s=float(data.get("duration_s", 0.0)),
                log_tail=str(data.get("log_tail", "")),
                triggered_by=str(data.get("triggered_by", "pull")),
            )
            if playbook and rec.playbook != playbook:
                continue
            out.append(rec)
    out.sort(key=lambda r: r.timestamp, reverse=True)
    return out[:limit]


def write_run_record(rec: RunRecord) -> Path:
    """Write a run JSON. Caller usually does this from the pull/push runners."""
    pdir = peer_runs_dir(rec.peer)
    pdir.mkdir(parents=True, exist_ok=True)
    ts_str = time.strftime("%Y%m%dT%H%M%S", time.localtime(rec.timestamp))
    path = pdir / f"{ts_str}_{rec.playbook}.json"
    path.write_text(json.dumps({
        "peer": rec.peer, "timestamp": rec.timestamp,
        "playbook": rec.playbook, "exit_code": rec.exit_code,
        "changed": rec.changed, "ok": rec.ok, "failed": rec.failed,
        "duration_s": rec.duration_s, "log_tail": rec.log_tail,
        "triggered_by": rec.triggered_by,
    }, indent=2), encoding="utf-8")
    return path


def prune_runs(days: int = 30) -> int:
    """Drop run JSONs older than `days`. Returns count of removed files."""
    cutoff = time.time() - (days * 86400)
    n = 0
    root = runs_dir()
    if not root.is_dir():
        return 0
    for pdir in root.iterdir():
        if not pdir.is_dir():
            continue
        for f in pdir.glob("*.json"):
            try:
                if f.stat().st_mtime < cutoff:
                    f.unlink()
                    n += 1
            except OSError:
                pass
    return n


# ---------------------------------------------------------------------------
# Runners
# ---------------------------------------------------------------------------


def run_local_pull(tags: Optional[List[str]] = None,
                   *, triggered_by: str = "manual") -> RunRecord:
    """Trigger a local ansible-pull cycle. Caller decides how to surface it."""
    start = time.time()
    cmd = [
        "ansible-pull",
        "-U", "file://" + str(playbooks_dir()),
        "-i", "localhost,",
        "--connection=local",
        "site.yml",
    ]
    if tags:
        cmd.extend(["--tags", ",".join(tags)])
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=3600)
        log_tail = "\n".join((proc.stdout or "").splitlines()[-16:])
        if proc.returncode != 0 and proc.stderr:
            log_tail += "\n--- stderr ---\n" + "\n".join(
                proc.stderr.splitlines()[-8:])
        ok, changed, failed = _parse_recap(proc.stdout or "")
        exit_code = proc.returncode
    except (OSError, subprocess.TimeoutExpired) as e:
        log_tail = f"runner error: {e}"
        ok = changed = failed = 0
        exit_code = 1
    rec = RunRecord(
        peer=current_peer_name(),
        timestamp=start,
        playbook=",".join(tags) if tags else "site",
        exit_code=exit_code,
        changed=changed, ok=ok, failed=failed,
        duration_s=time.time() - start,
        log_tail=log_tail,
        triggered_by=triggered_by,
    )
    write_run_record(rec)
    return rec


def run_push(peer_names: Iterable[str],
             tags: Optional[List[str]] = None) -> List[RunRecord]:
    """Ad-hoc SSH push: ansible-playbook --limit <peers> from this peer."""
    inv_path = _write_ephemeral_inventory()
    if inv_path is None:
        return []
    targets = list(peer_names)
    cmd = [
        "ansible-playbook",
        "-i", str(inv_path),
        "--limit", ",".join(targets),
        str(playbooks_dir() / "site.yml"),
    ]
    if tags:
        cmd.extend(["--tags", ",".join(tags)])
    start = time.time()
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=3600)
        log_tail = "\n".join((proc.stdout or "").splitlines()[-32:])
        if proc.returncode != 0 and proc.stderr:
            log_tail += "\n--- stderr ---\n" + "\n".join(
                proc.stderr.splitlines()[-8:])
        ok, changed, failed = _parse_recap(proc.stdout or "")
        exit_code = proc.returncode
    except (OSError, subprocess.TimeoutExpired) as e:
        log_tail = f"runner error: {e}"
        ok = changed = failed = 0
        exit_code = 1

    # Write one combined record per target peer; recap output is mixed so we
    # use the same totals for each. This is intentional — the per-peer JSON
    # gives the user *something* to click in the run-history table.
    records: List[RunRecord] = []
    for target in targets:
        rec = RunRecord(
            peer=target,
            timestamp=start,
            playbook=",".join(tags) if tags else "site",
            exit_code=exit_code,
            changed=changed, ok=ok, failed=failed,
            duration_s=time.time() - start,
            log_tail=log_tail,
            triggered_by="push",
        )
        write_run_record(rec)
        records.append(rec)
    return records


def _write_ephemeral_inventory() -> Optional[Path]:
    """Write a temporary inventory.ini for the SSH-push path."""
    try:
        peers = build_inventory()
    except Exception:  # noqa: BLE001
        return None
    if not peers:
        return None
    tmp = Path("/tmp") / f"mackes-inv-{os.getpid()}.ini"
    lines = ["[mesh]"]
    for p in peers:
        host = p.mesh_ip or f"{p.name}.mesh"
        lines.append(f"{p.name} ansible_host={host} ansible_user=mackes")
    lines.append("")
    lines.append("[mesh:vars]")
    lines.append("ansible_python_interpreter=/usr/bin/python3")
    lines.append("ansible_ssh_common_args='-o StrictHostKeyChecking=accept-new'")
    tmp.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return tmp


def _parse_recap(output: str) -> tuple[int, int, int]:
    """Parse Ansible's PLAY RECAP — return (ok, changed, failed) summed across hosts."""
    ok = changed = failed = 0
    in_recap = False
    for line in (output or "").splitlines():
        if "PLAY RECAP" in line:
            in_recap = True
            continue
        if not in_recap:
            continue
        if ":" not in line:
            continue
        # e.g. "anvil   : ok=4    changed=1    unreachable=0    failed=0"
        for token in line.split():
            if token.startswith("ok="):
                try: ok += int(token[3:])
                except ValueError: pass
            elif token.startswith("changed="):
                try: changed += int(token[8:])
                except ValueError: pass
            elif token.startswith("failed="):
                try: failed += int(token[7:])
                except ValueError: pass
    return ok, changed, failed


# ---------------------------------------------------------------------------
# Open in $EDITOR
# ---------------------------------------------------------------------------


def open_playbook_in_editor(pb: Playbook) -> bool:
    target = pb.main_task_path
    if not target.exists():
        return False
    # Prefer the user's GUI editor via xdg-open; fall back to $EDITOR.
    if shutil.which("xdg-open"):
        try:
            subprocess.Popen(["xdg-open", str(target)],
                             stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL,
                             start_new_session=True)
            return True
        except OSError:
            pass
    editor = os.environ.get("EDITOR", "vi")
    try:
        subprocess.Popen([editor, str(target)],
                         start_new_session=True)
        return True
    except OSError:
        return False


# ---------------------------------------------------------------------------
# CLI entry — invoked by mackes-ansible-pull.service
# ---------------------------------------------------------------------------


def _cli_main(argv: list[str]) -> int:
    if "--pull" in argv:
        tags = None
        if "--tags" in argv:
            i = argv.index("--tags")
            if i + 1 < len(argv):
                tags = argv[i + 1].split(",")
        rec = run_local_pull(tags=tags, triggered_by="pull")
        prune_runs(30)
        return rec.exit_code
    if "--push" in argv:
        # python -m mackes.fleet --push peer1,peer2 --tags update
        i = argv.index("--push")
        if i + 1 >= len(argv):
            print("usage: --push <peer1,peer2,...>")
            return 2
        peers = argv[i + 1].split(",")
        tags = None
        if "--tags" in argv:
            j = argv.index("--tags")
            if j + 1 < len(argv):
                tags = argv[j + 1].split(",")
        for r in run_push(peers, tags=tags):
            print(f"{r.peer}: rc={r.exit_code} changed={r.changed} ok={r.ok}")
        return 0
    if "--list" in argv:
        for pb in list_playbooks():
            print(f"  {pb.name:24} {pb.description}")
        return 0
    if "--history" in argv:
        for rec in list_runs(limit=50):
            ts = time.strftime("%Y-%m-%d %H:%M:%S",
                               time.localtime(rec.timestamp))
            mark = "ok" if rec.exit_code == 0 else "FAIL"
            print(f"  {ts}  {rec.peer:14}  {rec.playbook:24}  {mark}  "
                  f"changed={rec.changed}")
        return 0
    if "--prune" in argv:
        n = prune_runs(30)
        print(f"pruned {n} run record(s) older than 30 days")
        return 0
    print(__doc__)
    print("\nUsage:")
    print("  python -m mackes.fleet --pull [--tags update,bloat]")
    print("  python -m mackes.fleet --push <peers> [--tags ...]")
    print("  python -m mackes.fleet --list")
    print("  python -m mackes.fleet --history")
    print("  python -m mackes.fleet --prune")
    return 0


if __name__ == "__main__":
    import sys
    raise SystemExit(_cli_main(sys.argv[1:]))
