---
name: Bug report
about: Something in Mackes Shell isn't working
title: '[bug] '
labels: bug
assignees: ''
---

## What happened

A clear, short description.

## What I expected

What you thought would happen.

## Steps to reproduce

1.
2.
3.

## Environment

- Mackes Shell version: `mackes --version`
- OS: `cat /etc/os-release` (Fedora 44 / 45 / etc.)
- Desktop environment: XFCE / GNOME / KDE / headless
- Display server: X11 / Wayland / none
- Installed via: install.sh / dnf / git checkout

## Logs

Run with `MACKES_LOG=DEBUG mackes` and paste the relevant tail. Useful logs:

- `~/.local/share/mackes-shell/logs/mackes.log`
- `journalctl --user -u mackes-* --since '5 minutes ago'`
- `journalctl -u mackes-* --since '5 minutes ago'` (system services)

## Screenshots

If the bug is visual, attach a screenshot. (For Carbon-styling issues,
include the active preset name from the header chip.)
