"""mesh-fs FUSE backend with read-cache (#6).

Replaces N independent `sshfs` processes (one per peer) with a single
FUSE filesystem that opens ONE persistent SSH channel per peer and
multiplexes all file operations. Read paths land in an LRU disk cache
(via `diskcache`) so repeat directory listings + small-file reads
become local.

Mount layout:
  ~/QNM-Mesh-fast/<peer>/...    ← new (this module)
  ~/QNM-Mesh/<peer>/...         ← legacy sshfs (kept for back-compat
                                   until every peer is migrated)

Dependencies (soft — code reports unavailable if missing):
  * python3-fusepy  (Fedora: `python3-fusepy`, MIT)
  * python3-paramiko  (Fedora: `python3-paramiko`, LGPL)
  * python3-diskcache  (Fedora: `python3-diskcache`, Apache 2.0)

Public API:

  is_available()              → bool
  is_mounted(peer)            → bool
  mount(peer, *, host, user)  → bool
  unmount(peer)               → bool
  mounts()                    → list[dict]  for the panel
  cache_stats()               → dict        for the panel

This module ships the mount glue. The actual FUSE operations class
(`MeshFSOps`) is intentionally small — readdir, getattr, open, read
only. Writes are deferred to the legacy sshfs path until we have
end-to-end coverage on reads.
"""
from __future__ import annotations

import errno
import os
import stat as stat_mod
import subprocess
import threading
import time
from pathlib import Path
from typing import Optional


MESH_FAST_ROOT = Path.home() / "QNM-Mesh-fast"
SSH_KEY = Path.home() / ".ssh/mackes_mesh_ed25519"
CACHE_ROOT = Path.home() / ".cache/mackes-mesh-fs"


# ---------------------------------------------------------------------------
# Capability probes
# ---------------------------------------------------------------------------


def has_fusepy() -> bool:
    try:
        import fuse  # noqa: F401
        return True
    except ImportError:
        return False


def has_paramiko() -> bool:
    try:
        import paramiko  # noqa: F401
        return True
    except ImportError:
        return False


def has_diskcache() -> bool:
    try:
        import diskcache  # noqa: F401
        return True
    except ImportError:
        return False


def is_available() -> bool:
    """All three deps present. If False, fall back to legacy sshfs."""
    return has_fusepy() and has_paramiko() and has_diskcache()


# ---------------------------------------------------------------------------
# FUSE operations class
# ---------------------------------------------------------------------------


def _build_ops_class():
    """Build the MeshFSOps class lazily, so importing this module
    doesn't require fusepy to be installed."""
    if not is_available():
        return None
    import fuse
    import paramiko
    import diskcache

    class MeshFSOps(fuse.Operations):
        """One persistent SSHClient + SFTPClient per mount. Reads
        cached for 30 s via diskcache; stat-only ops cached 5 s.

        This implementation is read-only (write fns return EROFS).
        Pairs with the legacy `mesh_fs` SSHFS mount for writes during
        the migration period.
        """

        def __init__(self, *, host: str, user: str,
                     remote_dir: str = ".", peer_label: str = "") -> None:
            self.host = host
            self.user = user
            self.remote_dir = remote_dir
            self.label = peer_label or host
            self._ssh = paramiko.SSHClient()
            self._ssh.set_missing_host_key_policy(
                paramiko.AutoAddPolicy())
            self._ssh.connect(
                host, username=user, key_filename=str(SSH_KEY),
                timeout=10, banner_timeout=10, look_for_keys=False,
                allow_agent=False, compress=True,
            )
            self._sftp = self._ssh.open_sftp()
            self._cache = diskcache.Cache(
                str(CACHE_ROOT / self.label),
                size_limit=512 * 1024 * 1024,   # 512 MB per peer
            )
            self._lock = threading.Lock()

        # ---- read paths (cached) -----------------------------------

        def _remote(self, path: str) -> str:
            return f"{self.remote_dir.rstrip('/')}/{path.lstrip('/')}"

        def getattr(self, path: str, fh: Optional[int] = None) -> dict:
            key = f"stat:{path}"
            cached = self._cache.get(key)
            if cached is not None:
                return cached
            with self._lock:
                try:
                    s = self._sftp.stat(self._remote(path))
                except IOError as e:
                    raise fuse.FuseOSError(
                        errno.ENOENT if "No such" in str(e) else errno.EIO)
            d = {
                "st_mode": s.st_mode or stat_mod.S_IFREG | 0o644,
                "st_nlink": 1,
                "st_size": s.st_size or 0,
                "st_atime": s.st_atime or time.time(),
                "st_mtime": s.st_mtime or time.time(),
                "st_ctime": s.st_mtime or time.time(),
                "st_uid": os.getuid(),
                "st_gid": os.getgid(),
            }
            self._cache.set(key, d, expire=5)
            return d

        def readdir(self, path: str, fh: int):
            key = f"dir:{path}"
            cached = self._cache.get(key)
            if cached is not None:
                names = cached
            else:
                with self._lock:
                    try:
                        names = self._sftp.listdir(self._remote(path))
                    except IOError:
                        raise fuse.FuseOSError(errno.ENOENT)
                self._cache.set(key, names, expire=10)
            yield "."
            yield ".."
            for n in names:
                yield n

        def open(self, path: str, flags: int) -> int:
            # Reject any write flag; we're read-only.
            if flags & (os.O_WRONLY | os.O_RDWR | os.O_APPEND):
                raise fuse.FuseOSError(errno.EROFS)
            # We don't track per-fh state; SFTP open is implicit in read().
            return 0

        def read(self, path: str, size: int, offset: int,
                 fh: int) -> bytes:
            # Small reads (<= 64 KB) hit the disk cache; bigger reads
            # go straight to SFTP.
            CACHEABLE = 65536
            key = f"chunk:{path}:{offset}:{size}"
            if size <= CACHEABLE:
                cached = self._cache.get(key)
                if cached is not None:
                    return cached
            with self._lock:
                try:
                    f = self._sftp.open(self._remote(path), "rb")
                    f.seek(offset)
                    data = f.read(size)
                    f.close()
                except IOError:
                    raise fuse.FuseOSError(errno.EIO)
            if size <= CACHEABLE:
                self._cache.set(key, data, expire=30)
            return data

        # ---- write paths — read-only, return EROFS ------------------

        def create(self, *args, **kwargs):
            raise fuse.FuseOSError(errno.EROFS)
        def write(self, *args, **kwargs):
            raise fuse.FuseOSError(errno.EROFS)
        def mkdir(self, *args, **kwargs):
            raise fuse.FuseOSError(errno.EROFS)
        def unlink(self, *args, **kwargs):
            raise fuse.FuseOSError(errno.EROFS)
        def truncate(self, *args, **kwargs):
            raise fuse.FuseOSError(errno.EROFS)

        def destroy(self, path=None):
            try:
                self._sftp.close()
                self._ssh.close()
                self._cache.close()
            except Exception:  # noqa: BLE001
                pass

    return MeshFSOps


# ---------------------------------------------------------------------------
# Mount control
# ---------------------------------------------------------------------------


def _mountpoint(peer: str) -> Path:
    return MESH_FAST_ROOT / peer


def is_mounted(peer: str) -> bool:
    mp = _mountpoint(peer)
    if not mp.exists():
        return False
    try:
        return mp.stat().st_dev != mp.parent.stat().st_dev
    except OSError:
        return False


def mount(*, peer: str, host: str, user: str = "",
          remote_dir: str = ".") -> bool:
    """Mount peer's home dir at ~/QNM-Mesh-fast/<peer>/.

    Returns True on successful mount, False if dependencies absent or
    the mount failed (caller can fall back to legacy sshfs)."""
    if not is_available():
        return False
    if is_mounted(peer):
        return True
    mp = _mountpoint(peer)
    mp.mkdir(parents=True, exist_ok=True)
    CACHE_ROOT.mkdir(parents=True, exist_ok=True)
    if not user:
        user = os.environ.get("USER", "mm")
    Ops = _build_ops_class()
    if Ops is None:
        return False
    # FUSE.mount() blocks; spawn it on a thread so the caller returns.
    def runner():
        import fuse
        try:
            fuse.FUSE(
                Ops(host=host, user=user, remote_dir=remote_dir,
                    peer_label=peer),
                str(mp),
                foreground=True, allow_other=False, ro=True,
                fsname=f"meshfs-{peer}",
            )
        except Exception:  # noqa: BLE001
            pass
    t = threading.Thread(target=runner, daemon=True,
                         name=f"meshfs-{peer}")
    t.start()
    # Wait briefly for mount to come up
    for _ in range(20):
        if is_mounted(peer):
            return True
        time.sleep(0.1)
    return False


def unmount(peer: str) -> bool:
    if not is_mounted(peer):
        return True
    mp = _mountpoint(peer)
    try:
        r = subprocess.run(["fusermount", "-u", str(mp)],
                           capture_output=True, timeout=10)
        return r.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


# ---------------------------------------------------------------------------
# Panel inputs
# ---------------------------------------------------------------------------


def mounts() -> list[dict]:
    """Return [{peer, mountpoint, mounted, cache_size_bytes}] for the
    Mesh Performance panel."""
    out: list[dict] = []
    if not MESH_FAST_ROOT.is_dir():
        return out
    for child in MESH_FAST_ROOT.iterdir():
        if not child.is_dir():
            continue
        cache_dir = CACHE_ROOT / child.name
        cache_bytes = 0
        if cache_dir.is_dir():
            for f in cache_dir.rglob("*"):
                if f.is_file():
                    try:
                        cache_bytes += f.stat().st_size
                    except OSError:
                        pass
        out.append({
            "peer":              child.name,
            "mountpoint":        str(child),
            "mounted":           is_mounted(child.name),
            "cache_size_bytes":  cache_bytes,
        })
    return out


def cache_stats() -> dict:
    """Aggregate cache size + entry count across every peer's
    diskcache."""
    if not CACHE_ROOT.is_dir():
        return {"total_bytes": 0, "peer_count": 0}
    total = 0
    peers = 0
    for child in CACHE_ROOT.iterdir():
        if not child.is_dir():
            continue
        peers += 1
        for f in child.rglob("*"):
            if f.is_file():
                try:
                    total += f.stat().st_size
                except OSError:
                    pass
    return {"total_bytes": total, "peer_count": peers}


def status() -> dict:
    return {
        "available":    is_available(),
        "has_fusepy":   has_fusepy(),
        "has_paramiko": has_paramiko(),
        "has_diskcache": has_diskcache(),
        "mount_root":   str(MESH_FAST_ROOT),
        "cache_root":   str(CACHE_ROOT),
        "mounts":       mounts(),
        "cache":        cache_stats(),
    }


__all__ = [
    "is_available", "is_mounted", "mount", "unmount",
    "mounts", "cache_stats", "status",
    "MESH_FAST_ROOT", "CACHE_ROOT",
]
