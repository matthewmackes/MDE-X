"""Phase 13.4 — drawer KDE Connect notification merge tests.

v4.0.1 retirement (TEST-2): three tests that exercised the
`kdeconnect-notifications.json` merge (`merges_phone_origin`,
`concatenates_when_both_exist`, `skips_non_dict_phone_entries`) were
deleted on 2026-05-23. They covered behavior the drawer deliberately
retired in KDC2-5.10 — phone notifications now flow directly into
mako/the Iced notifications applet via the
`dev.mackes.MDE.Connect` DBus signal surface, not by file merging.
The remaining three tests still guard live drawer behavior:
missing-cache no-op, legacy-file ingest, corrupt-JSON tolerance.
"""
from __future__ import annotations

import json
import os
import tempfile
from pathlib import Path


def _with_isolated_cache(fn):
    """Run `fn(cache_dir)` with XDG_CACHE_HOME pointed at a tempdir.

    Doubles as a fixture-substitute for the no-pytest shim.
    """
    tmp = tempfile.TemporaryDirectory()
    try:
        old = os.environ.get("XDG_CACHE_HOME")
        os.environ["XDG_CACHE_HOME"] = tmp.name
        try:
            return fn(Path(tmp.name))
        finally:
            if old is None:
                del os.environ["XDG_CACHE_HOME"]
            else:
                os.environ["XDG_CACHE_HOME"] = old
    finally:
        tmp.cleanup()


def test_drawer_load_notifications_handles_missing_caches():
    def body(cache):
        from mackes.drawer import _load_pending_notifications
        assert _load_pending_notifications() == []
    _with_isolated_cache(body)


def test_drawer_load_notifications_reads_legacy_file():
    def body(cache):
        notes_dir = cache / "mackes"
        notes_dir.mkdir()
        (notes_dir / "notifications.json").write_text(json.dumps([
            {"app": "X", "title": "T", "body": "B"},
        ]))
        from mackes.drawer import _load_pending_notifications
        out = _load_pending_notifications()
        assert len(out) == 1
        assert out[0]["app"] == "X"
    _with_isolated_cache(body)


# --- Retired 2026-05-23 (TEST-2) ---
# `test_drawer_load_notifications_merges_phone_origin`,
# `test_drawer_load_notifications_concatenates_when_both_exist`, and
# `test_drawer_load_notifications_skips_non_dict_phone_entries` were
# removed here. They asserted the legacy kdeconnect-notifications.json
# file-merge that the drawer deliberately retired in KDC2-5.10 — phone
# notifications now flow through mako + the Iced notifications applet
# via the dev.mackes.MDE.Connect DBus signal surface.


def test_drawer_load_notifications_handles_corrupt_phone_json():
    def body(cache):
        notes_dir = cache / "mackes"
        notes_dir.mkdir()
        (notes_dir / "kdeconnect-notifications.json").write_text("{not json")
        from mackes.drawer import _load_pending_notifications
        # Corrupt phone file -> empty merge, no exception.
        assert _load_pending_notifications() == []
    _with_isolated_cache(body)
