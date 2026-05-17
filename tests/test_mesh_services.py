"""mesh_services — TCP probe + registry round-trip."""
from __future__ import annotations

import socket
from dataclasses import asdict


def _free_port() -> int:
    """Bind a socket to an OS-assigned port, close it, and return the
    port. The OS may reuse it for the next test — that's fine for a
    'this port should refuse connections right now' check."""
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def test_probe_tcp_returns_false_for_closed_port():
    from mackes.mesh_services import _probe_tcp
    # _free_port() releases its socket before we probe — the port is
    # almost certainly refusing connections by the time we try.
    port = _free_port()
    assert _probe_tcp("127.0.0.1", port, timeout=0.5) is False


def test_probe_tcp_succeeds_against_listening_socket():
    from mackes.mesh_services import _probe_tcp
    with socket.socket() as srv:
        srv.bind(("127.0.0.1", 0))
        srv.listen(1)
        port = srv.getsockname()[1]
        assert _probe_tcp("127.0.0.1", port, timeout=1.0) is True


def test_service_hit_round_trip_through_registry(isolated_xdg, monkeypatch):
    """Serialise a ServiceHit via the same path probe_all uses, then
    load_registry() should reconstruct the same dataclass."""
    import importlib
    import json
    import time
    import mackes.mesh_services
    importlib.reload(mackes.mesh_services)
    from mackes.mesh_services import REGISTRY_PATH, ServiceHit, load_registry

    REGISTRY_PATH.parent.mkdir(parents=True, exist_ok=True)
    original = ServiceHit(
        peer="node-a", service="jellyfin",
        port=8096, scheme="http", path="/web/",
        online=True, last_probe=time.time(),
    )
    REGISTRY_PATH.write_text(json.dumps([asdict(original)]))
    loaded = load_registry()
    assert len(loaded) == 1
    assert loaded[0].peer == "node-a"
    assert loaded[0].service == "jellyfin"
    assert loaded[0].port == 8096
    assert loaded[0].scheme == "http"


def test_load_registry_handles_corrupt_json(isolated_xdg, monkeypatch):
    """A garbled registry.json must not crash callers — return []."""
    import importlib
    import mackes.mesh_services
    importlib.reload(mackes.mesh_services)
    from mackes.mesh_services import REGISTRY_PATH, load_registry

    REGISTRY_PATH.parent.mkdir(parents=True, exist_ok=True)
    REGISTRY_PATH.write_text("{not valid json")
    assert load_registry() == []
