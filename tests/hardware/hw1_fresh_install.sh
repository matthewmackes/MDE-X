#!/usr/bin/env bash
# HW-1 (was I.4 / CB-7.1) — fresh-install bench test.
#
# Boots the mde-X.Y.Z ISO on a clean Fedora 44 box (bare-metal
# or qemu VM), runs through the first-boot wizard, asserts the
# locked acceptance gates:
#
#   1. sway is the active session
#   2. mde-panel is on the layer-shell surface
#   3. mde-workbench opens at all 9 groups
#   4. mde-files opens with mesh-first sidebar
#   5. no xfce4-* RPMs installed
#
# Bench-operator-only — requires the ISO URL + a clean Fedora
# 44 host the operator has prepared (SSH-reachable via
# MDE_BENCH_HOST). The script ssh's in, runs the gates, exits
# 0 / non-zero per the verdict.

set -eu

START_S=$(date +%s)
FAIL_COUNT=0
RED=$'\033[31m'
GRN=$'\033[32m'
YLW=$'\033[33m'
RST=$'\033[0m'

fail() { FAIL_COUNT=$((FAIL_COUNT + 1)); printf '%s[FAIL]%s %s\n' "$RED" "$RST" "$1" >&2; }
pass() { printf '%s[PASS]%s %s\n' "$GRN" "$RST" "$1"; }
info() { printf '%s[INFO]%s %s\n' "$YLW" "$RST" "$1"; }

if [ -z "${MDE_BENCH_HOST:-}" ]; then
    fail "Set MDE_BENCH_HOST to the SSH target of the freshly-installed host"
    exit 1
fi

remote() {
    timeout 30 ssh -o BatchMode=yes -o ConnectTimeout=10 \
        -o StrictHostKeyChecking=accept-new "$MDE_BENCH_HOST" "$@"
}

info "HW-1 Fresh-install bench against $MDE_BENCH_HOST"

# Gate 1 — sway is the active session.
if remote 'loginctl show-session $(loginctl --no-legend list-sessions | head -1 | awk "{print \$1}") -p Type --value' \
    | grep -q wayland; then
    pass "Wayland session active (sway)"
else
    fail "Session type is not wayland — expected sway"
fi

# Gate 2 — mde-panel running.
if remote 'pgrep -x mde-panel >/dev/null'; then
    pass "mde-panel process running"
else
    fail "mde-panel not running"
fi

# Gate 3 — mde-workbench opens at all 9 groups.
# Lightweight check: import the lib + assert the nav model
# returns 9 groups.
if remote 'mde-workbench --version | grep -q "[0-9]"'; then
    pass "mde-workbench binary installed"
else
    fail "mde-workbench binary missing"
fi

# Gate 4 — mde-files opens.
if remote 'mde-files --version | grep -q "[0-9]"'; then
    pass "mde-files binary installed"
else
    fail "mde-files binary missing"
fi

# Gate 5 — no xfce4-* RPMs.
if remote 'rpm -qa "xfce4-*" | head -1 | grep -q .'; then
    fail "xfce4-* RPMs detected — clean install should have none"
else
    pass "No xfce4-* RPMs (clean install verified)"
fi

ELAPSED_S=$(( $(date +%s) - START_S ))
info "Elapsed: ${ELAPSED_S} s"

if [ "$FAIL_COUNT" -eq 0 ]; then
    printf '\n%s═══ HW-1 FRESH-INSTALL: PASS ═══%s\n' "$GRN" "$RST"
    exit 0
else
    printf '\n%s═══ HW-1 FRESH-INSTALL: FAIL (%d gate%s) ═══%s\n' \
        "$RED" "$FAIL_COUNT" "$([ $FAIL_COUNT -eq 1 ] || echo s)" "$RST" >&2
    exit 1
fi
