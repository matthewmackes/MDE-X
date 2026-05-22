"""Tests for the app.py CSS resolution helpers (design system)."""
from __future__ import annotations

from mackes.app import _resolve_css, _CSS_ROOTS


def test_base_css_resolves():
    p = _resolve_css("mackes.css")
    assert p is not None
    assert p.name == "mackes.css"


def test_every_accent_resolves():
    for preset in ("ableton",):
        p = _resolve_css("accents", f"{preset}.css")
        assert p is not None, f"missing accent for {preset}"


def test_unknown_css_returns_none():
    assert _resolve_css("not-a-real-file.css") is None
    assert _resolve_css("accents", "not-a-preset.css") is None


def test_roots_contain_at_least_one_existing_dir():
    assert any(r.is_dir() for r in _CSS_ROOTS)
