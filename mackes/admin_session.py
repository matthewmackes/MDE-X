"""Session-wide admin elevation (v1.4.0).

Mackes wraps many privileged operations behind `pkexec` — clean security
posture, but ugly UX when the user is doing multiple config changes
back-to-back (5+ password prompts per wizard apply).

This module adds a session-wide unlock. The user clicks the lock icon in
the shell header, types their password ONCE, and every subsequent admin
operation runs without re-prompting until they explicitly lock again or
the Mackes window closes.

Mechanism: standard `sudo` timestamp file + background keepalive.

  unlock()  → `sudo -v` (validates credentials, refreshes timestamp).
              Starts a background thread that runs `sudo -n -v` every
              4 minutes to keep the timestamp alive.
  lock()    → `sudo -k` (invalidates timestamp). Stops keepalive thread.
  run(cmd)  → If unlocked: `sudo -n <cmd>` (no prompt, uses cached auth).
              If locked: legacy fallback to `pkexec <cmd>` (per-call prompt).

The keepalive thread is daemon=True so it dies with the process — there's
no risk of leaking a privileged auth past Mackes' lifetime.

Public API:

  AdminSession.instance()       → singleton
  .unlock(on_change=cb)         → returns True on success
  .lock()                       → revoke immediately
  .is_unlocked()                → bool
  .run(cmd, *, timeout=60)      → (rc, out_plus_err)
  .add_listener(fn)             → fn(unlocked: bool) on every state change
"""
from __future__ import annotations

import shutil
import subprocess
import threading
import time
from typing import Callable, List, Optional


_KEEPALIVE_INTERVAL = 240   # 4 minutes (sudo default timestamp_timeout is 5)


class AdminSession:
    _instance: Optional["AdminSession"] = None

    def __init__(self) -> None:
        self._unlocked = False
        self._unlocked_at: Optional[float] = None
        self._stop_event = threading.Event()
        self._keepalive_thread: Optional[threading.Thread] = None
        self._listeners: List[Callable[[bool], None]] = []
        self._lock = threading.Lock()

    @classmethod
    def instance(cls) -> "AdminSession":
        if cls._instance is None:
            cls._instance = cls()
        return cls._instance

    # ---- state queries ---------------------------------------------------

    def is_unlocked(self) -> bool:
        return self._unlocked

    def unlocked_at(self) -> Optional[float]:
        return self._unlocked_at

    def age_seconds(self) -> Optional[float]:
        if self._unlocked_at is None:
            return None
        return time.time() - self._unlocked_at

    # ---- listeners (UI binds here) --------------------------------------

    def add_listener(self, fn: Callable[[bool], None]) -> None:
        with self._lock:
            self._listeners.append(fn)

    def _notify(self) -> None:
        # Always dispatch via GLib if available so the listener fires on
        # the GTK main loop. Falls back to a direct call for non-GUI use.
        try:
            from gi.repository import GLib
            for fn in list(self._listeners):
                GLib.idle_add(fn, self._unlocked)
        except Exception:  # noqa: BLE001
            for fn in list(self._listeners):
                try:
                    fn(self._unlocked)
                except Exception:  # noqa: BLE001
                    pass

    # ---- unlock / lock ---------------------------------------------------

    def unlock(self) -> bool:
        """Authenticate once, then hold credentials for the rest of the session.

        Returns True on success. False on auth failure or sudo unavailable.
        """
        if self._unlocked:
            return True
        if not shutil.which("sudo"):
            # No sudo → fall back to per-call pkexec; we can't keep a session.
            return False

        # Use the GUI askpass helper if one is configured, otherwise rely on
        # the terminal/polkit agent — most XFCE desktops have polkit-gnome
        # or polkit-xfce running, which renders the prompt.
        env = self._sudo_env()
        try:
            r = subprocess.run(
                ["sudo", "-v"],
                env=env,
                capture_output=True, text=True, timeout=180,
            )
        except (OSError, subprocess.TimeoutExpired):
            return False
        if r.returncode != 0:
            return False

        # Success — flip state and start the keepalive.
        with self._lock:
            self._unlocked = True
            self._unlocked_at = time.time()
            self._stop_event.clear()
            self._keepalive_thread = threading.Thread(
                target=self._keepalive_loop, daemon=True,
                name="mackes-admin-keepalive",
            )
            self._keepalive_thread.start()
        self._notify()
        try:
            from mackes.logging import log_action
            log_action("admin session: unlocked")
        except Exception:  # noqa: BLE001
            pass
        return True

    def lock(self) -> None:
        """Revoke cached credentials + stop keepalive."""
        with self._lock:
            self._unlocked = False
            self._unlocked_at = None
            self._stop_event.set()
            self._keepalive_thread = None
        if shutil.which("sudo"):
            try:
                subprocess.run(["sudo", "-k"], capture_output=True, timeout=10)
            except (OSError, subprocess.TimeoutExpired):
                pass
        self._notify()
        try:
            from mackes.logging import log_action
            log_action("admin session: locked")
        except Exception:  # noqa: BLE001
            pass

    # ---- run a privileged command ----------------------------------------

    def run(self, cmd: List[str], *, timeout: int = 60,
            capture: bool = True) -> tuple[int, str]:
        """Execute `cmd` with admin privileges.

        Uses the cached sudo credentials when the session is unlocked.
        Falls back to a per-call pkexec prompt when locked. Always
        returns (returncode, combined stdout+stderr).
        """
        if self._unlocked and shutil.which("sudo"):
            full = ["sudo", "-n", *cmd]
        elif shutil.which("pkexec"):
            full = ["pkexec", *cmd]
        elif shutil.which("sudo"):
            full = ["sudo", *cmd]
        else:
            full = cmd
        try:
            if capture:
                r = subprocess.run(full, capture_output=True, text=True,
                                   timeout=timeout)
                return r.returncode, (r.stdout + r.stderr)
            else:
                r = subprocess.run(full, timeout=timeout)
                return r.returncode, ""
        except (OSError, subprocess.TimeoutExpired) as e:
            return 1, str(e)

    # ---- internals -------------------------------------------------------

    def _keepalive_loop(self) -> None:
        env = self._sudo_env()
        while not self._stop_event.is_set():
            # Wait first so we don't burn an extra refresh right after the
            # initial unlock (sudo already refreshed it).
            if self._stop_event.wait(_KEEPALIVE_INTERVAL):
                break
            try:
                r = subprocess.run(
                    ["sudo", "-n", "-v"],
                    env=env,
                    capture_output=True, text=True, timeout=10,
                )
            except (OSError, subprocess.TimeoutExpired):
                # Network hiccup, suspend/resume, whatever — try again next cycle.
                continue
            if r.returncode != 0:
                # The cached timestamp expired or got invalidated externally.
                # Lock ourselves so the UI reflects reality.
                with self._lock:
                    if not self._unlocked:
                        return
                    self._unlocked = False
                    self._unlocked_at = None
                self._notify()
                try:
                    from mackes.logging import log_action
                    log_action("admin session: lost cached credentials (timed out externally)")
                except Exception:  # noqa: BLE001
                    pass
                return

    @staticmethod
    def _sudo_env() -> dict:
        """Return an env dict that lets sudo prompt via the polkit agent."""
        import os
        env = dict(os.environ)
        # If SUDO_ASKPASS isn't set and we have a polkit agent in the
        # session, sudo will still prompt on the controlling tty — most
        # XFCE setups have polkit-gnome-authentication-agent-1 running
        # which intercepts. For the GUI flow, this is enough.
        return env


# Convenience module-level shortcuts ---------------------------------------


def session() -> AdminSession:
    """Get the singleton."""
    return AdminSession.instance()


def run_root(cmd: List[str], *, timeout: int = 60) -> tuple[int, str]:
    """Module-level shortcut. Equivalent to session().run(cmd, ...)."""
    return AdminSession.instance().run(cmd, timeout=timeout)
