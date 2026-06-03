# Mackes Desktop Environment (MDE) — User Guide

Welcome to **Mackes Desktop Environment (MDE)** — your single control
panel for the Mackes desktop on Fedora and the mesh fabric that connects
all your machines. (MDE is the successor to the 1.x line, which shipped
as "Mackes Shell" under XFCE; v2.0.0 renames the product to MDE and moves
to a Wayland-only stack on sway.)

## What MDE does

MDE replaces the legacy desktop's settings control panel as your daily
control surface and adds first-class mesh networking on top: every
machine running MDE can share files, clipboards, notifications, and
media services with every other machine in your mesh, regardless of
physical location.

Eight task tabs:

- **[Dashboard](dashboard.md)** — live status, drift detection, quick actions
- **[Look & Feel](look-and-feel.md)** — themes, fonts, icons, wallpaper
- **[Devices](devices.md)** — display, keyboard, mouse, sound, power
- **[Network](network.md)** — Wi-Fi, VPN, mesh, firewall, SSH
- **[System](system.md)** — window manager, workspaces, session, notifications
- **[Apps](apps.md)** — install curated apps, remove bloat
- **[Maintain](maintain.md)** — snapshots, repair, logs, uninstall
- **[Help](help.md)** — this guide

## The mesh

MDE ships a complete peer-to-peer mesh built on five layers:

- **[Mesh VPN](mesh-vpn.md)** — Headscale + Tailscale-bootstrap, WireGuard data plane
- **[Mesh in Thunar](mesh-thunar.md)** — `mesh:///` shows peers, clipboard, notifications, shared files
- **[Mesh SSH](mesh-ssh.md)** — auto-distributed keys + identity-based access
- **[Mesh Media Services](mesh-services.md)** — discover Jellyfin, Airsonic, Plex, etc. across the mesh
- **[Headless Node Mode](headless.md)** — full mesh on fileservers without a display

## Phones & tablets

- **[KDE Connect](kde-connect.md)** — paired phones / tablets surface in
  Workbench Connect, and the mesh-mDNS bridge keeps them reachable when
  they leave your LAN.

## Quick links

- **[Getting Started](getting-started.md)** — first-run wizard walkthrough
- **[Presets](presets.md)** — the 4+1 shipped presets explained
- **[Music](music.md)** — the native Airsonic / Subsonic player (mde-music)
- **[CLI Reference](cli-reference.md)** — every `mde` subcommand
- **[Keyboard shortcuts](keybindings.md)**
- **[Wayland support](wayland.md)** — what works on Wayland today (and
  why GNOME-shell on Wayland is not supported)
- **[Troubleshooting](troubleshooting.md)**

## About

Mackes Desktop Environment (MDE) 2.0.0. (Successor to Mackes Shell
1.x — see CHANGELOG for the 1.x release history.) GPL-3.0.
Source: https://github.com/matthewmackes/MAP2-RELEASES
