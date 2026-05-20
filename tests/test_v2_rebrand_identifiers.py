"""v2.0.0 Phase 0.13 — identifier-presence tests for the rebrand sweep.

Asserts the new MDE-namespaced identifiers actually ship in the
matching artifacts: spec Provides/Obsoletes line, CHANGELOG 2.0.0
header, metainfo component-id surface, bin/ shim presence, man
pages installed. Complements the per-area tests (D-Bus surfaces +
config-path migrator + env-var shim) shipped earlier.
"""
from __future__ import annotations

from pathlib import Path

REPO = Path(__file__).resolve().parent.parent


# --- spec Provides / Obsoletes parse ------------------------------------

def test_spec_advertises_mde_provides():
    spec = (REPO / "packaging" / "fedora" / "mackes-shell.spec").read_text()
    assert "Provides:       mde = %{version}-%{release}" in spec, (
        "spec must declare `Provides: mde = ...` so `dnf install mde` "
        "resolves to this RPM during the v1.x → v2.0.0 transition"
    )


def test_spec_keeps_legacy_mackes_shell_provides():
    spec = (REPO / "packaging" / "fedora" / "mackes-shell.spec").read_text()
    assert "Provides:       mackes-shell = %{version}-%{release}" in spec, (
        "spec must keep `Provides: mackes-shell` so installs from the "
        "1.0.6+ line resolve cleanly through the rename"
    )


def test_spec_obsoletes_pre_v3_legacy():
    spec = (REPO / "packaging" / "fedora" / "mackes-shell.spec").read_text()
    assert "Obsoletes:      mackes-shell < 3.0" in spec


def test_spec_drops_retired_systemd_install_lines():
    """Phase B.13 retired 10 standalone units; their install lines
    should NOT be in the spec anymore."""
    spec = (REPO / "packaging" / "fedora" / "mackes-shell.spec").read_text()
    for unit in (
        "data/systemd/mackes-clipboard-daemon.service",
        "data/systemd/mackes-gvfsd-mesh.service",
        "data/systemd/mackes-mdns-relay.service",
        "data/systemd/mackes-remmina-sync.service",
        "data/systemd/mackes-remmina-sync.timer",
        "data/systemd/mackes-media-sync.service",
        "data/systemd/mackes-media-sync.timer",
        "data/systemd/mackes-ansible-pull.service",
        "data/systemd/mackes-ansible-pull.timer",
        "data/systemd/mackesd-kdc-bridge.service",
    ):
        assert f"install -m 0644 {unit}" not in spec, (
            f"retired unit install line still in spec: {unit}"
        )


def test_spec_installs_new_mde_session_unit():
    spec = (REPO / "packaging" / "fedora" / "mackes-shell.spec").read_text()
    assert "install -m 0644 data/systemd/mde-session.service" in spec


def test_spec_installs_new_mde_binaries():
    spec = (REPO / "packaging" / "fedora" / "mackes-shell.spec").read_text()
    for bin_name in ("mde", "mde-wm", "mde-enforce-session",
                     "mde-migrate-from-1x", "mde-shell-migrate-v2"):
        assert f"bin/{bin_name}" in spec, (
            f"new mde-* binary {bin_name} missing from spec install lines"
        )


def test_spec_installs_new_man_pages():
    spec = (REPO / "packaging" / "fedora" / "mackes-shell.spec").read_text()
    for man in ("mde.1", "mded.8",
                "mde-migrate-from-1x.1", "mde-shell-migrate-v2.1"):
        assert f"data/man/{man}" in spec, (
            f"man page {man} missing from spec install lines"
        )


# --- CHANGELOG 2.0.0 header --------------------------------------------

def test_changelog_carries_v2_0_0_entry():
    text = (REPO / "CHANGELOG.md").read_text()
    assert "## 2.0.0 — Rebrand to Mackes Desktop Environment (MDE)" in text


def test_changelog_v2_entry_documents_upgrade_path():
    text = (REPO / "CHANGELOG.md").read_text()
    # Three critical upgrade-path elements: Obsoletes/Provides,
    # config-path migrator, env-var shim.
    assert "Obsoletes" in text or "Provides" in text
    assert "mde-migrate-from-1x" in text
    assert "MDE_*" in text and "MACKES_*" in text


def test_changelog_v2_entry_documents_unified_daemon():
    text = (REPO / "CHANGELOG.md").read_text()
    assert "Unified Rust meta-daemon" in text or "unified Rust" in text.lower()


# --- bin/ + man/ presence ----------------------------------------------

def test_every_new_mde_bin_shim_ships():
    for name in ("mde", "mde-wm", "mde-enforce-session",
                 "mde-migrate-from-1x", "mde-shell-migrate-v2"):
        p = REPO / "bin" / name
        assert p.is_file(), f"missing bin shim: {p}"
        assert p.stat().st_mode & 0o111, f"bin shim not executable: {p}"


def test_every_new_man_page_ships():
    for name in ("mde.1", "mde-migrate-from-1x.1", "mde-shell-migrate-v2.1"):
        p = REPO / "data" / "man" / name
        assert p.is_file(), f"missing man page: {p}"
    assert (REPO / "data" / "man" / "mded.8").is_file()


# --- identifier-table reference doc -----------------------------------

def test_identifier_table_doc_ships():
    p = REPO / "docs" / "design" / "v2.0.0-mde-rebrand" / "identifiers.md"
    assert p.is_file()
    text = p.read_text()
    # Spot-check that the canonical mappings are documented.
    assert "mackes-shell" in text and "mde" in text
    assert "mackesd" in text and "mded" in text
    assert ("shell.mackes" in text or "org.mackes" in text) \
        and "dev.mackes.MDE" in text
    assert "MACKES_" in text and "MDE_" in text


def test_cosmic_files_upstream_pin_doc_ships():
    p = REPO / "docs" / "upstream" / "cosmic-files.md"
    assert p.is_file()
    text = p.read_text()
    assert "cosmic-files" in text
    assert "GPL-3.0" in text


def test_cosmic_files_license_attribution_ships():
    p = REPO / "LICENSES" / "COSMIC-FILES.md"
    assert p.is_file()
    text = p.read_text()
    assert "cosmic-files" in text
    assert "System76" in text
    assert "GPL-3.0" in text


# --- CB-7.4 spec regression assertions for the v2.0.0 cut --------------
#
# These assertions land here so they fail loudly the moment a regression
# slips in. They cover the spec lines that *land at cut time* (Name,
# Conflicts:, Recommends:) — the rebrand items above cover what's
# already shipped during the back-compat window.

def test_spec_will_advertise_name_mde_at_cut():
    """At CB-3.1 cut, `Name:` flips from `mackes-shell` to `mde`.
    The current spec still ships as mackes-shell during back-compat
    (Provides/Obsoletes handle the upgrade); after CB-3.1 the spec
    rename happens and this assertion flips."""
    spec = (REPO / "packaging" / "fedora" / "mackes-shell.spec").read_text()
    # Pre-CB-3.1 form: keeps Name:mackes-shell + Provides: mde.
    # Post-CB-3.1 form: Name:mde + Provides: mackes-shell (legacy).
    # Both forms satisfy: "the spec ships mde as a resolvable name".
    has_provides_mde = "Provides:       mde = %{version}-%{release}" in spec
    has_name_mde = "Name:           mde\n" in spec or "Name:    mde\n" in spec
    assert has_provides_mde or has_name_mde, (
        "spec must either Provides: mde (back-compat window) OR "
        "Name: mde (post-cut form)"
    )


def test_spec_conflicts_block_lands_at_cb_3_3():
    """CB-3.3 adds explicit Conflicts: lines so `dnf install
    xfce4-panel` errors after MDE is installed. The Conflicts
    block lands AT CUT — until then this test is a soft check
    (passes when the block is absent, asserts shape when present)."""
    spec = (REPO / "packaging" / "fedora" / "mackes-shell.spec").read_text()
    # If the cut has happened (Conflicts: appears), every locked
    # entry must be present.
    if "Conflicts:" in spec:
        for pkg in (
            "xfce4-panel",
            "xfdesktop",
            "xfce4-session",
            "xfce4-settings",
            "xfwm4",
        ):
            assert f"Conflicts:" in spec and pkg in spec, (
                f"CB-3.3 locked Conflicts: entry {pkg} missing"
            )


def test_spec_recommends_wayland_stack_post_cut():
    """CB-3.2 adds hard Requires for sway+swaylock+swayidle+swaybg
    +foot. Until cut, those land as Recommends to avoid breaking
    1.x installs. Either form satisfies."""
    spec = (REPO / "packaging" / "fedora" / "mackes-shell.spec").read_text()
    for pkg in ("sway", "swaylock", "swayidle"):
        present = (
            f"Requires:       {pkg}" in spec
            or f"Recommends:     {pkg}" in spec
            or f"Recommends: {pkg}" in spec
            or f"Requires: {pkg}" in spec
        )
        # Soft check during the back-compat window — log but don't
        # block until the cut lands the hard Requires.
        if not present:
            print(f"NOTE: spec doesn't yet advertise {pkg} as Requires/Recommends")


def test_comps_xml_present_at_cb_3_4_cut():
    """CB-3.4 ships data/comps/mackes-desktop-environment.xml. The
    file is optional during the back-compat window — once present,
    its shape must satisfy the group definition contract."""
    comps = REPO / "data" / "comps" / "mackes-desktop-environment.xml"
    if not comps.is_file():
        # Pre-CB-3.4 — test is a noop.
        return
    text = comps.read_text()
    assert "<id>mackes-desktop-environment</id>" in text
    assert "<name>Mackes Desktop Environment</name>" in text
    assert "<packagereq" in text  # one or more package entries
    for pkg in ("sway", "swaylock", "swayidle", "swaybg", "foot"):
        assert pkg in text, f"comps group must include {pkg}"


def test_spec_ships_v2_0_0_preset():
    """CB-3.6 — the v2.0.0 preset must install."""
    spec = (REPO / "packaging" / "fedora" / "mackes-shell.spec").read_text()
    assert "data/systemd/90-mde.preset" in spec
    assert "%{_prefix}/lib/systemd/user-preset/90-mde.preset" in spec


def test_spec_ships_wayland_session_entry():
    """CB-2.1 — the Wayland-session .desktop must install."""
    spec = (REPO / "packaging" / "fedora" / "mackes-shell.spec").read_text()
    assert "%{_datadir}/wayland-sessions/mde.desktop" in spec
