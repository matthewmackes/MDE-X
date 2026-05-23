#!/bin/bash
# install-parity-infra.sh — one-shot installer for the v4.0.1 PARITY
# infrastructure. Run with sudo from the repo root:
#
#     sudo install-helpers/install-parity-infra.sh
#
# What it does:
#   1. Copies install-helpers/parity-overlay.sh  → /usr/local/bin/mde-parity-overlay
#   2. Copies install-helpers/sudoers-mde-parity → /etc/sudoers.d/mde-parity
#   3. Creates /var/log/mde-parity.log with the right perms
#   4. Copies data/systemd-user/mde-parity.{path,service} into the
#      developer's ~/.config/systemd/user/ and enables the path unit
#   5. Optionally runs the overlay once now to perform the initial
#      Python+Rust sync against the v4.0.0 RPM
#
# Idempotent: re-running just refreshes any out-of-date files.

set -euo pipefail

DEVUSER="${SUDO_USER:-mm}"
DEVHOME="$(getent passwd "$DEVUSER" | cut -d: -f6)"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
SYSTEMD_USER_DIR="$DEVHOME/.config/systemd/user"

[ "$EUID" -eq 0 ] || { echo "ERROR: must run as root"; exit 1; }
[ -d "$REPO" ]    || { echo "ERROR: repo not found: $REPO"; exit 1; }
[ -d "$DEVHOME" ] || { echo "ERROR: dev home not found: $DEVHOME"; exit 1; }

echo "==> installing /usr/local/bin/mde-parity-overlay"
install -D -m 0755 "$REPO/install-helpers/parity-overlay.sh" \
    /usr/local/bin/mde-parity-overlay

echo "==> installing /etc/sudoers.d/mde-parity"
install -D -m 0440 "$REPO/install-helpers/sudoers-mde-parity" \
    /etc/sudoers.d/mde-parity
visudo -c -f /etc/sudoers.d/mde-parity

echo "==> ensuring /var/log/mde-parity.log"
install -m 0664 /dev/null /var/log/mde-parity.log

echo "==> installing systemd-user units into $SYSTEMD_USER_DIR"
install -D -m 0644 "$REPO/data/systemd-user/mde-parity.path" \
    "$SYSTEMD_USER_DIR/mde-parity.path"
install -D -m 0644 "$REPO/data/systemd-user/mde-parity.service" \
    "$SYSTEMD_USER_DIR/mde-parity.service"
# v4.0.1 AF-6 (2026-05-23) — per-user mackesd unit. Owns the
# session-bus side of the AF-* mega (Fleet.Files DBus surface
# that mde-files's DBusBackend talks to). The system mackesd
# unit (data/systemd/mackesd.service, User=mackesd) can't claim
# session-bus names; this one runs as $DEVUSER + uses an
# XDG-scoped DB at ~/.local/share/mde/mded.db.
install -D -m 0644 "$REPO/data/systemd-user/mackesd.service" \
    "$SYSTEMD_USER_DIR/mackesd.service"
chown -R "$DEVUSER:$DEVUSER" "$SYSTEMD_USER_DIR"

echo "==> enabling + starting the path watch"
sudo -u "$DEVUSER" XDG_RUNTIME_DIR="/run/user/$(id -u "$DEVUSER")" \
    systemctl --user daemon-reload
sudo -u "$DEVUSER" XDG_RUNTIME_DIR="/run/user/$(id -u "$DEVUSER")" \
    systemctl --user enable --now mde-parity.path

echo "==> enabling + starting mackesd (per-user)"
# Best-effort — if the mackesd binary isn't installed yet
# (parity overlay hasn't run), enable but don't start. The
# next overlay tick will install the binary; operator can
# `systemctl --user start mackesd` then.
if [ -x /usr/bin/mackesd ]; then
    sudo -u "$DEVUSER" XDG_RUNTIME_DIR="/run/user/$(id -u "$DEVUSER")" \
        systemctl --user enable --now mackesd
else
    sudo -u "$DEVUSER" XDG_RUNTIME_DIR="/run/user/$(id -u "$DEVUSER")" \
        systemctl --user enable mackesd
    echo "    NOTE: /usr/bin/mackesd not installed yet; enabled but not started."
    echo "    Run 'systemctl --user start mackesd' after the next parity tick."
fi

echo
echo "==> parity infra installed. running initial overlay now..."
echo "    (cargo build first run takes ~2-3 minutes; subsequent runs"
echo "    are seconds because cargo's incremental cache warms up.)"
echo
# Run the overlay as the developer user. The no-args path runs the
# build phase (as $DEVUSER, with their cargo cache) then re-execs
# itself via sudo -n for the install phase — the sudoers drop-in we
# just installed makes that passwordless. Skip with NO_INITIAL_OVERLAY=1
# if the operator only wants the watch set up.
if [ "${NO_INITIAL_OVERLAY:-0}" != "1" ]; then
    sudo -u "$DEVUSER" -H XDG_RUNTIME_DIR="/run/user/$(id -u "$DEVUSER")" \
        /usr/local/bin/mde-parity-overlay
fi

echo
echo "==> done. status:"
sudo -u "$DEVUSER" XDG_RUNTIME_DIR="/run/user/$(id -u "$DEVUSER")" \
    systemctl --user status mde-parity.path --no-pager || true

cat <<'POSTINSTALL'

Auto-deploy active. Every `git commit` on main from now on
triggers the overlay within ~2s. To watch deploys land:
    tail -f /var/log/mde-parity.log

To disable the auto-deploy:
    systemctl --user disable --now mde-parity.path

To re-run the overlay manually:
    /usr/local/bin/mde-parity-overlay

POSTINSTALL
