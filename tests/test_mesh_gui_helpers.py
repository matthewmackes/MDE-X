"""Pure-helper tests for the mesh wizard passcode validator.

The Phase 12.8 workbench mesh-control + mesh-history tests that used
to live here were retired with `EPIC-RETIRE-PY-WORKBENCH.delete-ported.batch-2`
on 2026-05-26 — the `mackes/workbench/network/mesh_control.py` +
`mesh_history.py` panels were deleted in favor of the Iced
`crates/mde-workbench/src/panels/mesh_control.rs` +
`mesh_history.rs` equivalents (Rust-side coverage lives there).

What survives here: the 3 `passcode_validator_*` tests that exercise
`mackes/wizard/pages/mesh_passcode.py` — wizard code is NOT being
retired, so these stay.
"""
from __future__ import annotations


def test_passcode_validator_accepts_16_char_url_safe():
    from mackes.wizard.pages.mesh_passcode import passcode_is_valid
    assert passcode_is_valid("aB3-_xyz12345678")
    assert passcode_is_valid("0123456789abcdef")
    assert passcode_is_valid("ABCDEFGHIJKLMNOP")
    assert passcode_is_valid("------__________")


def test_passcode_validator_rejects_wrong_length():
    from mackes.wizard.pages.mesh_passcode import passcode_is_valid
    assert not passcode_is_valid("")
    assert not passcode_is_valid("short")
    assert not passcode_is_valid("a" * 15)
    assert not passcode_is_valid("a" * 17)
    assert not passcode_is_valid("a" * 32)


def test_passcode_validator_rejects_non_url_safe():
    from mackes.wizard.pages.mesh_passcode import passcode_is_valid
    # Space, plus, slash, equals are NOT in the URL-safe alphabet.
    assert not passcode_is_valid("aaaaaaaaaaaaaa b")
    assert not passcode_is_valid("a/aaaaaaaaaaaa+a")
    assert not passcode_is_valid("aaaaaaaaaaaaaa=a")
    # Multi-byte chars are out.
    assert not passcode_is_valid("aaaaaaaaaaaaaaña")
