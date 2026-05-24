"""Tests for GF-3.1 — `apply_uid_normalize` in `mackes/birthright.py`.

The v5.0.0 GlusterFS mesh-home rollout requires every peer's
primary login account on uid:gid 1000:1000 so cross-peer file
ownership stays consistent under FUSE.

These tests cover:

  1. Already-normalized: returns the "already 1000:1000" log line
     and runs zero subprocess calls.
  2. UID-1000 collision with a different user: refuses to migrate,
     surfaces a clear log line, runs zero subprocess calls.
  3. GID-1000 collision with a different group: refuses to migrate,
     surfaces a clear log line, runs zero subprocess calls.
  4. Happy-path migration: runs `usermod -u 1000`, `groupmod -g 1000`,
     `chown -R 1000:1000 $HOME` (when $HOME exists), `chown -R
     1000:1000 /var/lib/<user>` (when it exists).
  5. Missing $USER environment: returns the "no primary user"
     log line, runs zero subprocess calls.
  6. User-not-in-passwd: returns the "not in /etc/passwd" log
     line, runs zero subprocess calls.
  7. usermod failure: returns the failure log line + halts before
     groupmod/chown.

Every test monkeypatches `pwd.getpwnam` / `pwd.getpwuid` /
`grp.getgrgid` and `birthright._run_root` so the suite runs as a
non-root user without touching the host's account database.
"""
from __future__ import annotations

from typing import Any, Dict, List, Tuple

from mackes import birthright


class _FakePw:
    """Stand-in for a `pwd.struct_passwd`."""

    def __init__(self, name: str, uid: int, gid: int, home: str) -> None:
        self.pw_name = name
        self.pw_uid = uid
        self.pw_gid = gid
        self.pw_dir = home


class _FakeGr:
    """Stand-in for a `grp.struct_group`."""

    def __init__(self, name: str, gid: int) -> None:
        self.gr_name = name
        self.gr_gid = gid


def _patch_env(monkeypatch, user: str | None) -> None:
    for var in ("SUDO_USER", "USER", "LOGNAME"):
        monkeypatch.delenv(var, raising=False)
    if user is not None:
        monkeypatch.setenv("USER", user)


def _patch_pwd_grp(
    monkeypatch,
    *,
    by_name: Dict[str, _FakePw],
    by_uid: Dict[int, _FakePw],
    by_gid: Dict[int, _FakeGr],
) -> None:
    import grp
    import pwd

    def getpwnam(name: str) -> _FakePw:
        if name in by_name:
            return by_name[name]
        raise KeyError(name)

    def getpwuid(uid: int) -> _FakePw:
        if uid in by_uid:
            return by_uid[uid]
        raise KeyError(uid)

    def getgrgid(gid: int) -> _FakeGr:
        if gid in by_gid:
            return by_gid[gid]
        raise KeyError(gid)

    monkeypatch.setattr(pwd, "getpwnam", getpwnam)
    monkeypatch.setattr(pwd, "getpwuid", getpwuid)
    monkeypatch.setattr(grp, "getgrgid", getgrgid)


def _patch_run_root(monkeypatch, *, force_failures: Dict[str, Tuple[int, str]] | None = None
                    ) -> List[List[str]]:
    """Replace birthright._run_root with a recorder. Returns the
    cmd-list every call captured."""
    calls: List[List[str]] = []
    fails = force_failures or {}

    def fake(cmd: List[str], *, timeout: int = 300) -> Tuple[int, str]:
        calls.append(cmd[:])
        verb = cmd[0]
        if verb in fails:
            return fails[verb]
        return 0, "ok"

    monkeypatch.setattr(birthright, "_run_root", fake)
    return calls


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_already_normalized_returns_clean_line_no_subprocess_calls(monkeypatch):
    _patch_env(monkeypatch, "alice")
    pw = _FakePw("alice", 1000, 1000, "/home/alice")
    _patch_pwd_grp(
        monkeypatch,
        by_name={"alice": pw},
        by_uid={1000: pw},
        by_gid={1000: _FakeGr("alice", 1000)},
    )
    calls = _patch_run_root(monkeypatch)

    out = birthright.apply_uid_normalize(_dummy_preset())

    assert calls == []
    assert any("already uid:gid 1000:1000" in line for line in out)


def test_uid_1000_collision_with_other_user_refuses(monkeypatch):
    _patch_env(monkeypatch, "alice")
    alice = _FakePw("alice", 1001, 1001, "/home/alice")
    bob = _FakePw("bob", 1000, 1000, "/home/bob")
    _patch_pwd_grp(
        monkeypatch,
        by_name={"alice": alice, "bob": bob},
        by_uid={1000: bob, 1001: alice},
        by_gid={1000: _FakeGr("bob", 1000), 1001: _FakeGr("alice", 1001)},
    )
    calls = _patch_run_root(monkeypatch)

    out = birthright.apply_uid_normalize(_dummy_preset())

    assert calls == []
    msg = " | ".join(out)
    assert "uid 1000 is held by 'bob'" in msg
    assert "Refusing to migrate" in msg


def test_gid_1000_collision_with_other_group_refuses(monkeypatch):
    _patch_env(monkeypatch, "alice")
    alice = _FakePw("alice", 1001, 1001, "/home/alice")
    _patch_pwd_grp(
        monkeypatch,
        by_name={"alice": alice},
        by_uid={1001: alice},   # uid 1000 free
        by_gid={1000: _FakeGr("staff", 1000), 1001: _FakeGr("alice", 1001)},
    )
    calls = _patch_run_root(monkeypatch)

    out = birthright.apply_uid_normalize(_dummy_preset())

    assert calls == []
    msg = " | ".join(out)
    assert "gid 1000 is held by group 'staff'" in msg
    assert "Refusing to migrate" in msg


def test_happy_path_runs_usermod_groupmod_chowns_home(monkeypatch, tmp_path):
    _patch_env(monkeypatch, "alice")
    home = tmp_path / "alice-home"
    home.mkdir()
    alice = _FakePw("alice", 1001, 1001, str(home))
    _patch_pwd_grp(
        monkeypatch,
        by_name={"alice": alice},
        by_uid={1001: alice},
        by_gid={1001: _FakeGr("alice", 1001)},
    )
    calls = _patch_run_root(monkeypatch)

    out = birthright.apply_uid_normalize(_dummy_preset())

    verbs = [c[0] for c in calls]
    assert verbs == ["usermod", "groupmod", "chown"]
    chown_targets = [c[-1] for c in calls if c[0] == "chown"]
    assert str(home) in chown_targets
    assert any("usermod -u 1000 alice ok" in line for line in out)
    assert any(f"chown -R 1000:1000 {home} ok" in line for line in out)


def test_happy_path_also_chowns_var_lib_user_when_present(monkeypatch, tmp_path):
    _patch_env(monkeypatch, "alice")
    home = tmp_path / "alice-home"
    home.mkdir()
    state = tmp_path / "var-lib-alice"
    state.mkdir()
    alice = _FakePw("alice", 1001, 1001, str(home))
    _patch_pwd_grp(
        monkeypatch,
        by_name={"alice": alice},
        by_uid={1001: alice},
        by_gid={1001: _FakeGr("alice", 1001)},
    )

    # Override the state-dir construction so we test against tmp_path
    # rather than /var/lib/alice.
    monkeypatch.setattr(
        birthright, "Path", _PathPatched(birthright.Path, {"/var/lib/alice": state})
    )

    calls = _patch_run_root(monkeypatch)
    out = birthright.apply_uid_normalize(_dummy_preset())

    chown_targets = [c[-1] for c in calls if c[0] == "chown"]
    assert str(state) in chown_targets
    assert any(f"chown -R 1000:1000 {state} ok" in line for line in out)


def test_no_user_in_environment_skips(monkeypatch):
    _patch_env(monkeypatch, None)
    calls = _patch_run_root(monkeypatch)

    out = birthright.apply_uid_normalize(_dummy_preset())

    assert calls == []
    assert any("no primary user in environment" in line for line in out)


def test_root_user_skips(monkeypatch):
    _patch_env(monkeypatch, "root")
    calls = _patch_run_root(monkeypatch)

    out = birthright.apply_uid_normalize(_dummy_preset())

    assert calls == []
    assert any("no primary user in environment" in line for line in out)


def test_user_not_in_passwd_skips(monkeypatch):
    _patch_env(monkeypatch, "ghost")
    _patch_pwd_grp(
        monkeypatch,
        by_name={},
        by_uid={},
        by_gid={},
    )
    calls = _patch_run_root(monkeypatch)

    out = birthright.apply_uid_normalize(_dummy_preset())

    assert calls == []
    assert any("not in /etc/passwd" in line for line in out)


def test_usermod_failure_halts_before_groupmod(monkeypatch, tmp_path):
    _patch_env(monkeypatch, "alice")
    alice = _FakePw("alice", 1001, 1001, str(tmp_path))
    _patch_pwd_grp(
        monkeypatch,
        by_name={"alice": alice},
        by_uid={1001: alice},
        by_gid={1001: _FakeGr("alice", 1001)},
    )
    calls = _patch_run_root(
        monkeypatch,
        force_failures={"usermod": (1, "usermod: user is in use")},
    )

    out = birthright.apply_uid_normalize(_dummy_preset())

    verbs = [c[0] for c in calls]
    assert verbs == ["usermod"]   # groupmod + chown NOT called
    assert any("usermod -u 1000 alice failed" in line for line in out)


# ---------------------------------------------------------------------------
# Plumbing helpers
# ---------------------------------------------------------------------------


def _dummy_preset() -> Any:
    """The function ignores its preset argument; pass a sentinel."""
    return object()


class _PathPatched:
    """Pass-through wrapper around `pathlib.Path` that redirects
    a hand-picked set of paths to test fixtures.

    Only used by the chown-var-lib test; production code constructs
    `Path(f"/var/lib/{user}")` literally — we redirect that one
    path to a tempdir without affecting any other Path() calls.
    """

    def __init__(self, real: Any, redirects: Dict[str, Any]) -> None:
        self._real = real
        self._redirects = redirects

    def __call__(self, *args, **kwargs):
        if len(args) == 1 and args[0] in self._redirects:
            return self._redirects[args[0]]
        return self._real(*args, **kwargs)

    def __getattr__(self, name: str) -> Any:
        return getattr(self._real, name)
