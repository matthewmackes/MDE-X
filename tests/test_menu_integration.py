"""xfce4-settings menu hiding round-trip."""
from __future__ import annotations



def test_hide_then_restore(isolated_xdg, monkeypatch):
    """Faking a system .desktop, ensure hide creates an override and restore removes it."""
    from mackes import menu_integration
    fake_system = isolated_xdg["home"] / "fake-system-applications"
    fake_system.mkdir(parents=True, exist_ok=True)
    fake_name = "xfce-display-settings.desktop"
    (fake_system / fake_name).write_text(
        "[Desktop Entry]\nType=Application\nName=Display\n", encoding="utf-8")

    monkeypatch.setattr(menu_integration, "_system_desktop",
                        lambda name: (fake_system / name) if (fake_system / name).exists() else None)

    actions = menu_integration.hide_xfce_settings_entries()
    override = menu_integration.USER_APPLICATIONS_DIR / fake_name
    assert override.exists()
    assert "X-Mackes-Hidden" in override.read_text(encoding="utf-8")
    assert any("hidden:" in a for a in actions)

    menu_integration.restore_xfce_settings_entries()
    assert not override.exists()
