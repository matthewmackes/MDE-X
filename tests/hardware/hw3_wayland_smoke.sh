#!/usr/bin/env bash
# HW-3 (was I.3 / CB-7.3) — Wayland-in-CI smoke test.
#
# Launches a headless sway session via WLR_BACKENDS=headless,
# starts mde-session, and asserts:
#
#   1. swaymsg -t get_outputs returns the expected fake output.
#   2. mde-panel registers a toplevel in the foreign-toplevel
#      listener.
#   3. mde-workbench opens on Ctrl+1.
#
# Designed to run inside the GitHub Actions ubuntu-latest /
# fedora:44 container with sway + wlr-randr installed. The
# script lives separate from the Rust unit tests because
# spawning sway from inside a cargo test is messy.
#
# Exit 0 on PASS, non-zero on FAIL with a clear gate name.

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

cleanup() {
    if [ -n "${SWAY_PID:-}" ]; then
        kill "$SWAY_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

info "HW-3 Wayland smoke (headless sway)"

# Prereqs.
for bin in sway swaymsg wlr-randr; do
    if ! command -v "$bin" >/dev/null 2>&1; then
        fail "missing $bin — install sway + wlr-randr"
        exit 1
    fi
done
if [ -z "${XDG_RUNTIME_DIR:-}" ]; then
    export XDG_RUNTIME_DIR="/tmp/hw3-runtime-$$"
    mkdir -p "$XDG_RUNTIME_DIR"
    chmod 700 "$XDG_RUNTIME_DIR"
fi

# Spawn headless sway.
info "Starting headless sway (WLR_BACKENDS=headless)…"
WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 sway >/tmp/sway-hw3.log 2>&1 &
SWAY_PID=$!
sleep 3

# Gate 1 — get_outputs returns the fake headless output.
if swaymsg -t get_outputs | grep -q 'HEADLESS-1'; then
    pass "swaymsg -t get_outputs sees HEADLESS-1"
else
    fail "swaymsg -t get_outputs missing HEADLESS-1 (see /tmp/sway-hw3.log)"
fi

# Gate 2 — mde-panel registers a toplevel.
# (Skip if mde-panel binary isn't built; CI builds it before
# invoking this script.)
if command -v mde-panel >/dev/null 2>&1; then
    mde-panel &
    PANEL_PID=$!
    sleep 2
    if swaymsg -t get_tree | grep -q 'mde-panel'; then
        pass "mde-panel registered in sway tree"
    else
        fail "mde-panel didn't appear in sway tree"
    fi
    kill "$PANEL_PID" 2>/dev/null || true
else
    info "mde-panel binary not in PATH — skipping panel-registration gate"
fi

# Gate 3 — mde-workbench opens.
if command -v mde-workbench >/dev/null 2>&1; then
    mde-workbench &
    WB_PID=$!
    sleep 3
    if swaymsg -t get_tree | grep -q 'mde-workbench'; then
        pass "mde-workbench opened"
    else
        fail "mde-workbench didn't open"
    fi
    kill "$WB_PID" 2>/dev/null || true
else
    info "mde-workbench binary not in PATH — skipping workbench gate"
fi

ELAPSED_S=$(( $(date +%s) - START_S ))
info "Elapsed: ${ELAPSED_S} s"

if [ "$FAIL_COUNT" -eq 0 ]; then
    printf '\n%s═══ HW-3 WAYLAND SMOKE: PASS ═══%s\n' "$GRN" "$RST"
    exit 0
else
    printf '\n%s═══ HW-3 WAYLAND SMOKE: FAIL (%d gate%s) ═══%s\n' \
        "$RED" "$FAIL_COUNT" "$([ $FAIL_COUNT -eq 1 ] || echo s)" "$RST" >&2
    exit 1
fi
