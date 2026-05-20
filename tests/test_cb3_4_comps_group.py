"""CB-3.4 — comps group definition smoke tests."""
from __future__ import annotations

import os
import sys
from pathlib import Path
from xml.etree import ElementTree as ET

import pytest


REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO))

COMPS = REPO / "data/comps/mackes-desktop-environment.xml"
SPEC = REPO / "packaging/fedora/mackes-shell.spec"


def test_comps_file_ships():
    assert COMPS.is_file()


def test_comps_xml_is_well_formed():
    """ElementTree must parse the file without raising — guards
    against typos in the XML."""
    tree = ET.parse(COMPS)
    root = tree.getroot()
    assert root.tag == "comps"


def test_comps_declares_the_group():
    tree = ET.parse(COMPS)
    group = tree.find("group")
    assert group is not None
    assert group.findtext("id") == "mackes-desktop-environment"
    assert group.findtext("name") == "Mackes Desktop Environment"
    assert group.findtext("default") == "false"
    assert group.findtext("uservisible") == "true"


def test_comps_packagelist_contains_locked_packages():
    """The CB-3.4 lock requires every member of the Wayland stack
    + the unified daemon to be in the package list."""
    tree = ET.parse(COMPS)
    pkgs = {p.text for p in tree.findall("group/packagelist/packagereq")}
    for required in (
        "mde",
        "sway",
        "swaylock",
        "swayidle",
        "swaybg",
        "foot",
        "bemenu",
        "brightnessctl",
        "pipewire",
        "wireplumber",
        "grim",
        "slurp",
        "kanshi",
    ):
        assert required in pkgs, f"comps must include {required}"


def test_comps_mandatory_vs_default_split_is_locked():
    """Wayland-stack packages are mandatory; alternate file managers
    + logout helpers are default."""
    tree = ET.parse(COMPS)
    mandatories = {
        p.text for p in tree.findall("group/packagelist/packagereq")
        if p.get("type") == "mandatory"
    }
    defaults = {
        p.text for p in tree.findall("group/packagelist/packagereq")
        if p.get("type") == "default"
    }
    for must in ("mde", "sway", "swaylock", "swayidle", "swaybg"):
        assert must in mandatories, f"{must} must be mandatory"
    for opt in ("cosmic-files", "yazi", "wlogout", "wofi"):
        assert opt in defaults, f"{opt} should be default (optional)"


def test_spec_installs_comps_group():
    src = SPEC.read_text()
    assert "data/comps/mackes-desktop-environment.xml" in src
    assert "%{_datadir}/mde/comps/mackes-desktop-environment.xml" in src


def test_spec_registers_comps_group_in_post():
    src = SPEC.read_text()
    assert "dnf groups mark install mackes-desktop-environment" in src


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
