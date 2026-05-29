#!/usr/bin/env bash
# Add the Mackes Shell dnf repo so users can `dnf upgrade mackes-shell` once
# the upstream repo URL is live. Idempotent: re-running is a no-op.
#
# The baseurl/gpgkey in the shipped .repo are PLACEHOLDERS — replace them
# with the real URLs before publishing to users. Until then, this helper is
# safe to run (skip_if_unavailable=True), but no upgrades will land.
set -euo pipefail

REPO_FILE="/etc/yum.repos.d/mackes-shell.repo"
SHIP_DIR="${MACKES_SHELL_SHARE:-/usr/share/mde}"
SOURCE="${SHIP_DIR}/data/dnf/mackes-shell.repo"

if [[ ! -f "$SOURCE" ]]; then
    echo "add-mackes-repo: source .repo not found at $SOURCE" >&2
    exit 1
fi

if [[ -f "$REPO_FILE" ]] && cmp -s "$SOURCE" "$REPO_FILE"; then
    echo "add-mackes-repo: repo already installed and up to date"
    exit 0
fi

if [[ $EUID -ne 0 ]]; then
    echo "add-mackes-repo: must run as root (try: sudo $0)" >&2
    exit 2
fi

install -m 0644 "$SOURCE" "$REPO_FILE"
dnf clean metadata >/dev/null 2>&1 || true
echo "add-mackes-repo: installed $REPO_FILE"
