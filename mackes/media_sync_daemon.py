"""Media Sync daemon (v2.1.0) — keeps Sublime Music + Delfin configs and
the Thunar `Mackes Media/` view in sync with discovered mesh media
servers.

Runs as a per-user systemd service (`mackes-media-sync.service`)
triggered every 60s by `mackes-media-sync.timer`. Idempotent: a no-op
when nothing changed.

What it writes per cycle:

  1. ~/.config/sublime-music/config.json     — every discovered
     Airsonic / Subsonic server (with QNM-Shared creds if present)
  2. ~/.local/share/Delfin/servers.json      — every discovered
     Jellyfin server (with access tokens if present)
  3. ~/Mackes Media/<friendly>.desktop       — one launcher per server,
     opens the right client pre-pointed at that server
  4. ~/.config/gtk-3.0/bookmarks             — appends/refreshes the
     "Mackes Media" Thunar bookmark

Credential source: QNM-Shared bucket at
`qnm-shared://mackes/media-credentials.json`. Encrypted at rest by QNM;
written to client configs in plaintext (mode 0600).

Failure posture: any write failure logs through `mackes.logging` and
the daemon continues — never crashes the systemd unit. The next cycle
re-attempts.

Run loop entry point: `python3 -m mackes.media_sync_daemon`.
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Optional

from mackes.logging import log_action
from mackes.mesh_media import KIND_AIRSONIC, KIND_JELLYFIN, MediaServer, discover


# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------


HOME = Path.home()
MACKES_MEDIA_DIR = HOME / "Mackes Media"
SUBLIME_CONFIG = HOME / ".config" / "sublime-music" / "config.json"
DELFIN_CONFIG = HOME / ".local" / "share" / "Delfin" / "servers.json"
GTK_BOOKMARKS = HOME / ".config" / "gtk-3.0" / "bookmarks"
QNM_CREDS_PATH = HOME / ".local" / "share" / "mackes" / "qnm-shared" / "mackes" / "media-credentials.json"
BOOKMARK_LINE = f"file://{MACKES_MEDIA_DIR.as_posix()} Mackes Media"


# ---------------------------------------------------------------------------
# Credentials
# ---------------------------------------------------------------------------


def _load_credentials() -> dict:
    """Load QNM-Shared media credentials if present.

    Schema:
        {
          "airsonic": {"<host>": {"user": "...", "password": "..."}},
          "jellyfin": {"<host>": {"user": "...", "access_token": "..."}}
        }
    Missing file → empty dict (clients render their own login prompt).
    """
    if not QNM_CREDS_PATH.is_file():
        return {"airsonic": {}, "jellyfin": {}}
    try:
        data = json.loads(QNM_CREDS_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as e:
        log_action(f"media-sync: could not read credentials: {e}")
        return {"airsonic": {}, "jellyfin": {}}
    data.setdefault("airsonic", {})
    data.setdefault("jellyfin", {})
    return data


# ---------------------------------------------------------------------------
# Client config writers
# ---------------------------------------------------------------------------


def _write_atomic(path: Path, payload: str, *, mode: int = 0o600) -> bool:
    """Write `payload` to `path` atomically (rename-in-place) at `mode`."""
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(path.suffix + ".tmp")
        tmp.write_text(payload, encoding="utf-8")
        tmp.chmod(mode)
        tmp.replace(path)
        return True
    except OSError as e:
        log_action(f"media-sync: write {path} failed: {e}")
        return False


def _write_sublime_config(airsonic_servers: list[MediaServer],
                          creds: dict) -> bool:
    """Generate Sublime Music's config.json from the discovered list."""
    servers = []
    for s in airsonic_servers:
        c = creds.get(s.host, {})
        servers.append({
            "name":     f"{s.host} (mesh)",
            "server_address": s.url,
            "username": c.get("user", ""),
            "password": c.get("password", ""),
            "sync_enabled": True,
            "verify_cert": False,
        })
    payload = json.dumps({"providers": servers}, indent=2)
    return _write_atomic(SUBLIME_CONFIG, payload)


def _write_delfin_config(jellyfin_servers: list[MediaServer],
                         creds: dict) -> bool:
    """Generate Delfin's servers.json from the discovered list."""
    servers = []
    for s in jellyfin_servers:
        c = creds.get(s.host, {})
        servers.append({
            "name":         f"{s.host} (mesh)",
            "address":      s.url,
            "user":         c.get("user", ""),
            "access_token": c.get("access_token", ""),
        })
    payload = json.dumps({"servers": servers}, indent=2)
    return _write_atomic(DELFIN_CONFIG, payload)


# ---------------------------------------------------------------------------
# Thunar view
# ---------------------------------------------------------------------------


def _desktop_for(server: MediaServer) -> tuple[str, str]:
    """Return (filename, file body) for the .desktop launcher in
    `~/Mackes Media/`."""
    if server.kind == KIND_AIRSONIC:
        title = f"Airsonic — {server.host}"
        # Sublime Music doesn't accept --server on the CLI yet; opening
        # the app reads the synced config.json we just wrote. The
        # launcher is mostly a visual entry-point.
        exec_line = "flatpak run com.sublimemusic.SublimeMusic"
        icon = "audio-x-generic"
    else:
        title = f"Jellyfin — {server.host}"
        exec_line = "flatpak run app.drey.Delfin"
        icon = "video-x-generic"
    safe = (title.replace(" ", "_").replace("/", "_")
                  .replace("—", "-")) + ".desktop"
    body = (
        "[Desktop Entry]\n"
        "Type=Application\n"
        f"Name={title}\n"
        f"Comment=Mesh media server at {server.url}\n"
        f"Exec={exec_line}\n"
        f"Icon={icon}\n"
        "Terminal=false\n"
        "Categories=AudioVideo;\n"
    )
    return safe, body


def _rebuild_thunar_view(servers: list[MediaServer]) -> None:
    """Recreate `~/Mackes Media/` to contain exactly one .desktop per
    discovered server. Removes stale entries."""
    try:
        MACKES_MEDIA_DIR.mkdir(parents=True, exist_ok=True)
    except OSError as e:
        log_action(f"media-sync: could not create {MACKES_MEDIA_DIR}: {e}")
        return

    expected: dict[str, str] = {}
    for s in servers:
        name, body = _desktop_for(s)
        expected[name] = body

    # Write/refresh expected entries
    for name, body in expected.items():
        path = MACKES_MEDIA_DIR / name
        try:
            if path.read_text(encoding="utf-8") == body:
                continue
        except OSError:
            pass
        try:
            path.write_text(body, encoding="utf-8")
            path.chmod(0o644)
        except OSError as e:
            log_action(f"media-sync: write {path} failed: {e}")

    # Remove stale .desktop files (servers we no longer discover)
    for existing in MACKES_MEDIA_DIR.glob("*.desktop"):
        if existing.name not in expected:
            try:
                existing.unlink()
            except OSError:
                pass


def _ensure_bookmark() -> None:
    """Append the Mackes Media bookmark to ~/.config/gtk-3.0/bookmarks
    if it isn't already there."""
    try:
        GTK_BOOKMARKS.parent.mkdir(parents=True, exist_ok=True)
        existing = ""
        if GTK_BOOKMARKS.is_file():
            existing = GTK_BOOKMARKS.read_text(encoding="utf-8")
        if BOOKMARK_LINE in existing.splitlines():
            return
        with GTK_BOOKMARKS.open("a", encoding="utf-8") as f:
            if existing and not existing.endswith("\n"):
                f.write("\n")
            f.write(BOOKMARK_LINE + "\n")
    except OSError as e:
        log_action(f"media-sync: could not update bookmarks: {e}")


# ---------------------------------------------------------------------------
# Public surface — one cycle of the sync loop
# ---------------------------------------------------------------------------


def run_once() -> int:
    """One sync cycle. Returns the number of servers discovered."""
    creds = _load_credentials()
    servers = discover()
    airsonic = [s for s in servers if s.kind == KIND_AIRSONIC]
    jellyfin = [s for s in servers if s.kind == KIND_JELLYFIN]

    _write_sublime_config(airsonic, creds.get("airsonic", {}))
    _write_delfin_config(jellyfin, creds.get("jellyfin", {}))
    _rebuild_thunar_view(servers)
    _ensure_bookmark()

    if servers:
        log_action(f"media-sync: synced {len(airsonic)} airsonic + "
                   f"{len(jellyfin)} jellyfin")
    return len(servers)


def main() -> int:
    """systemd entry point — one cycle then exit; the timer re-fires us."""
    try:
        run_once()
        return 0
    except Exception as e:  # noqa: BLE001
        log_action(f"media-sync: unhandled exception: {e}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
