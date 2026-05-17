"""mesh_ssh — audit-record JSONL round-trip + policy dataclass shape."""
from __future__ import annotations


def test_policy_rule_defaults():
    from mackes.mesh_ssh import PolicyRule
    r = PolicyRule()
    assert r.action == "accept"
    assert r.src == ["*"]
    assert r.dst == ["*"]
    assert r.users == ["root"]


def test_audit_record_jsonl_round_trip(isolated_xdg, monkeypatch):
    """record_audit appends a single JSON line per call; read_audit
    parses them back into AuditRecord dataclasses."""
    import importlib
    import mackes.mesh_ssh
    importlib.reload(mackes.mesh_ssh)
    from mackes.mesh_ssh import AuditRecord, read_audit, record_audit

    rec1 = AuditRecord(
        timestamp="2026-05-17T12:00:00", source_peer="alpha",
        source_user="me", target_peer="beta", target_user="me",
        session_id="abc", exit_status=0,
    )
    rec2 = AuditRecord(
        timestamp="2026-05-17T12:05:00", source_peer="alpha",
        source_user="me", target_peer="gamma", target_user="root",
        session_id="def", exit_status=1,
    )
    record_audit(rec1)
    record_audit(rec2)

    out = read_audit()
    assert len(out) == 2
    assert out[0].target_peer == "beta"
    assert out[1].exit_status == 1


def test_read_audit_skips_corrupt_lines(isolated_xdg, monkeypatch):
    """A truncated or non-JSON line in the audit log must not stop
    later valid lines from being returned."""
    import importlib
    import mackes.mesh_ssh
    importlib.reload(mackes.mesh_ssh)
    from mackes.mesh_ssh import AuditRecord, MESH_AUDIT_LOG, read_audit, record_audit

    rec = AuditRecord(
        timestamp="2026-05-17T13:00:00", source_peer="a", source_user="me",
        target_peer="b", target_user="me", session_id="ok", exit_status=0,
    )
    record_audit(rec)
    # Inject garbage between records
    with MESH_AUDIT_LOG.open("a", encoding="utf-8") as f:
        f.write("{not valid json\n")
        f.write("{\"missing\": \"required-fields\"}\n")
    record_audit(rec)

    out = read_audit()
    # Both valid records survive; the two bad lines are dropped.
    assert len(out) == 2
    assert all(r.session_id == "ok" for r in out)


def test_read_audit_empty_when_file_absent(isolated_xdg):
    import importlib
    import mackes.mesh_ssh
    importlib.reload(mackes.mesh_ssh)
    from mackes.mesh_ssh import read_audit
    assert read_audit() == []
