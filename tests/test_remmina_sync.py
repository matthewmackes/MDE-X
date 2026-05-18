"""remmina_sync — INI render + reconcile semantics.

The reconciler MUST: only touch files with group=Mesh Peers, add files
for detected (peer, protocol) pairs, delete managed files no longer
in the target set, and never alter files outside the group.
"""
from __future__ import annotations

import configparser


def test_render_ssh_uses_mesh_keypair():
    from mackes.remmina_sync import (
        MACKES_GROUP, MACKES_TAG, PeerProbe, _render_remmina,
    )
    text = _render_remmina(
        PeerProbe(name="alpha", host="100.64.0.5", ssh=True), "ssh")
    cp = configparser.ConfigParser(interpolation=None); cp.optionxform = str
    cp.read_string(text)
    sect = cp["remmina"]
    assert sect["group"] == MACKES_GROUP
    assert sect[MACKES_TAG] == "1"
    assert sect["protocol"] == "SSH"
    assert sect["server"] == "100.64.0.5:22"
    assert sect["ssh_auth"] == "3"           # public-key auth
    assert "mackes_mesh_ed25519" in sect["ssh_privatekey"]


def test_render_rdp_has_blank_password():
    """Q3 lock: RDP/VNC password fields blank — Remmina prompts and
    stores via its own keyring."""
    from mackes.remmina_sync import PeerProbe, _render_remmina
    text = _render_remmina(
        PeerProbe(name="beta", host="100.64.0.6", rdp=True), "rdp")
    cp = configparser.ConfigParser(interpolation=None); cp.optionxform = str
    cp.read_string(text)
    sect = cp["remmina"]
    assert sect["protocol"] == "RDP"
    assert sect["server"] == "100.64.0.6:3389"
    assert sect["password"] == ""
    assert sect["username"] == ""


def test_render_vnc_uses_5900():
    from mackes.remmina_sync import PeerProbe, _render_remmina
    text = _render_remmina(
        PeerProbe(name="gamma", host="100.64.0.7", vnc=True), "vnc")
    cp = configparser.ConfigParser(interpolation=None); cp.optionxform = str
    cp.read_string(text)
    assert cp["remmina"]["protocol"] == "VNC"
    assert cp["remmina"]["server"] == "100.64.0.7:5900"


def test_sync_adds_files_for_detected_services(tmp_path, monkeypatch):
    """When sync() is given a peer with ssh+rdp open, it writes one
    .remmina file per protocol."""
    import mackes.remmina_sync as rs
    monkeypatch.setattr(rs, "REMMINA_DIR", tmp_path)
    peer = rs.PeerProbe(name="alpha", host="100.64.0.5",
                        ssh=True, rdp=True, vnc=False)
    report = rs.sync(peers=[peer])
    assert report.peers_probed == 1
    assert len(report.added) == 2     # ssh + rdp
    files = sorted(p.name for p in tmp_path.glob("*.remmina"))
    assert any("ssh" in f for f in files)
    assert any("rdp" in f for f in files)
    assert not any("vnc" in f for f in files)


def test_sync_is_idempotent(tmp_path, monkeypatch):
    """Re-running with the same input produces zero changes."""
    import mackes.remmina_sync as rs
    monkeypatch.setattr(rs, "REMMINA_DIR", tmp_path)
    peer = rs.PeerProbe(name="alpha", host="100.64.0.5", ssh=True)
    rs.sync(peers=[peer])
    report2 = rs.sync(peers=[peer])
    assert report2.added == []
    assert report2.updated == []
    assert report2.deleted == []
    assert len(report2.skipped) == 1


def test_sync_deletes_stale_managed_entries(tmp_path, monkeypatch):
    """A managed entry whose peer goes away gets deleted."""
    import mackes.remmina_sync as rs
    monkeypatch.setattr(rs, "REMMINA_DIR", tmp_path)
    rs.sync(peers=[rs.PeerProbe(name="alpha", host="1.1.1.1", ssh=True)])
    assert len(list(tmp_path.glob("*.remmina"))) == 1
    # alpha goes away
    report = rs.sync(peers=[])
    assert len(report.deleted) == 1
    assert list(tmp_path.glob("*.remmina")) == []


def test_sync_never_touches_files_outside_group(tmp_path, monkeypatch):
    """User-owned Remmina files (group != 'Mesh Peers') must survive
    every sync() call unchanged. This is the safety guarantee from Q4."""
    import mackes.remmina_sync as rs
    monkeypatch.setattr(rs, "REMMINA_DIR", tmp_path)
    user_file = tmp_path / "my-personal.remmina"
    user_file.write_text(
        "[remmina]\n"
        "group=My Group\n"
        "name=My VPS\n"
        "protocol=SSH\n"
        "server=vps.example.com:22\n",
        encoding="utf-8",
    )
    snapshot_before = user_file.read_text(encoding="utf-8")
    # Run sync with no peers — should delete managed entries but not
    # touch user_file
    rs.sync(peers=[])
    assert user_file.exists()
    assert user_file.read_text(encoding="utf-8") == snapshot_before
    # Run sync with peers — same guarantee
    rs.sync(peers=[rs.PeerProbe(name="alpha", host="1.1.1.1", ssh=True)])
    assert user_file.exists()
    assert user_file.read_text(encoding="utf-8") == snapshot_before


def test_sync_updates_when_content_drifts(tmp_path, monkeypatch):
    """If a peer's mesh_ip changes (e.g. tailscale reassigned), the
    existing managed file is rewritten rather than added/deleted."""
    import mackes.remmina_sync as rs
    monkeypatch.setattr(rs, "REMMINA_DIR", tmp_path)
    rs.sync(peers=[rs.PeerProbe(name="alpha", host="1.1.1.1", ssh=True)])
    report = rs.sync(peers=[rs.PeerProbe(name="alpha", host="2.2.2.2",
                                          ssh=True)])
    assert len(report.updated) == 1
    # File still present, content updated
    file = next(tmp_path.glob("*ssh*"))
    assert "2.2.2.2" in file.read_text(encoding="utf-8")


def test_render_filename_slugifies_peer_name(tmp_path, monkeypatch):
    """Peer names with funky chars get slugified safely."""
    import mackes.remmina_sync as rs
    monkeypatch.setattr(rs, "REMMINA_DIR", tmp_path)
    rs.sync(peers=[rs.PeerProbe(name="Mom's Laptop!", host="1.1.1.1",
                                 ssh=True)])
    files = list(tmp_path.glob("*.remmina"))
    assert len(files) == 1
    # No spaces / apostrophes / exclamation marks in the filename
    assert "'" not in files[0].name
    assert " " not in files[0].name
    assert "!" not in files[0].name
