"""mesh_sync — bucket put/get/list/versions round-trip + retention."""
from __future__ import annotations


def test_bucket_put_get_string(isolated_xdg, monkeypatch):
    import importlib
    import mackes.mesh_sync
    importlib.reload(mackes.mesh_sync)
    from mackes.mesh_sync import BUCKET_CLIPBOARD, put

    actions = put(BUCKET_CLIPBOARD, "hello", "world")
    assert any("v1" in a for a in actions)

    # NB: get() reads from a PEER's mount, not 'mine'. We test list_keys
    # of our own bucket below — for the "my own bucket" read path,
    # callers go through list_keys(peer=None).
    from mackes.mesh_sync import list_keys
    keys = list_keys(BUCKET_CLIPBOARD)
    assert any(k.key == "hello" for k in keys)


def test_bucket_versioning(isolated_xdg, monkeypatch):
    """Two puts to the same key produce v1 and v2."""
    import importlib
    import mackes.mesh_sync
    importlib.reload(mackes.mesh_sync)
    from mackes.mesh_sync import BUCKET_DROP, list_keys, put, versions

    put(BUCKET_DROP, "thing", "first")
    put(BUCKET_DROP, "thing", "second")

    # The latest BucketEntry should report revision 2
    entries = [e for e in list_keys(BUCKET_DROP) if e.key == "thing"]
    assert len(entries) == 1
    assert entries[0].revision == 2

    # Our own bucket lives under SYNC_ROOT_MINE; versions() targets a
    # peer mount, so this is a smoke check that the function exists +
    # doesn't crash for a non-existent peer (the QNM-Mesh layout).
    assert versions(BUCKET_DROP, "(none)", "thing") == []


def test_bucket_dict_value_is_json_encoded(isolated_xdg, monkeypatch):
    import importlib
    import json
    import mackes.mesh_sync
    importlib.reload(mackes.mesh_sync)
    from mackes.mesh_sync import BUCKET_PRESETS, SYNC_ROOT_MINE, put

    put(BUCKET_PRESETS, "active", {"name": "hashbang", "version": 1})
    latest = (SYNC_ROOT_MINE / BUCKET_PRESETS / "active" / "latest")
    assert latest.exists() or latest.is_symlink()
    # Resolve the symlink and parse — should be valid JSON
    data = json.loads(latest.read_text(encoding="utf-8"))
    assert data == {"name": "hashbang", "version": 1}


def test_bucket_retention_drops_older_revisions(isolated_xdg, monkeypatch):
    """max_versions cap prevents unbounded growth."""
    import importlib
    import mackes.mesh_sync
    importlib.reload(mackes.mesh_sync)
    from mackes.mesh_sync import BUCKET_DROP, SYNC_ROOT_MINE, put

    for _ in range(5):
        put(BUCKET_DROP, "k", "x", max_versions=2)
    key_dir = SYNC_ROOT_MINE / BUCKET_DROP / "k"
    versions_on_disk = sorted(key_dir.glob("v*.dat"))
    assert len(versions_on_disk) == 2
