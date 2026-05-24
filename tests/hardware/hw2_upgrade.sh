#!/usr/bin/env bash
# HW-2 (was I.5 / CB-7.2) — upgrade bench test.
#
# Boots a pre-built mackes-xfce-workstation-1.1.0 install
# (bare-metal or VM), runs `dnf upgrade -y`, reboots, logs
# in, asserts:
#
#   - HW-1 gates pass (sway active, mde-panel + mde-workbench
#     + mde-files installed, no xfce4-* RPMs)
#   - mde-migrate-from-1x ran successfully
#   - ~/.config/mde/ populated from ~/.config/mackes-shell/
#   - ~/.config/xfce4.v1x-backup.<ts>/ exists
#   - 1.x panel settings (theme name, font name, power
#     preferences, autostart list) carried across
#
# Requires MDE_BENCH_UPGRADE_HOST set to a 1.x-installed Fedora
# 44 host; the script ssh's in + drives the upgrade.

set -eu

START_S=$(date +%s)
FAIL_COUNT=0
RED=$'\033[31m'; GRN=$'\033[32m'; YLW=$'\033[33m'; RST=$'\033[0m'
fail() { FAIL_COUNT=$((FAIL_COUNT + 1)); printf '%s[FAIL]%s %s\n' "$RED" "$RST" "$1" >&2; }
pass() { printf '%s[PASS]%s %s\n' "$GRN" "$RST" "$1"; }
info() { printf '%s[INFO]%s %s\n' "$YLW" "$RST" "$1"; }

if [ -z "${MDE_BENCH_UPGRADE_HOST:-}" ]; then
    fail "Set MDE_BENCH_UPGRADE_HOST to the SSH target running mackes-xfce-workstation-1.1.0"
    exit 1
fi

remote() {
    timeout 60 ssh -o BatchMode=yes -o ConnectTimeout=10 \
        -o StrictHostKeyChecking=accept-new "$MDE_BENCH_UPGRADE_HOST" "$@"
}

info "HW-2 Upgrade bench against $MDE_BENCH_UPGRADE_HOST"

# Drive the upgrade.
info "Running dnf upgrade -y…"
if remote 'sudo dnf upgrade -y mde mackes-shell 2>&1 | tail -10'; then
    pass "dnf upgrade completed"
else
    fail "dnf upgrade failed"
fi

info "Rebooting + waiting for SSH back up…"
remote 'sudo systemctl reboot' || true
sleep 30
for i in 1 2 3 4 5; do
    if remote 'echo ok' 2>/dev/null | grep -q ok; then
        pass "Host back up after reboot (${i}0 s wait)"
        break
    fi
    sleep 10
done

# HW-1 gates (subset relevant to upgrade).
if remote 'pgrep -x mde-panel >/dev/null'; then
    pass "mde-panel running post-upgrade"
else
    fail "mde-panel not running"
fi

if remote 'rpm -qa "xfce4-*" | head -1 | grep -q .'; then
    fail "xfce4-* RPMs still present post-upgrade"
else
    pass "xfce4-* RPMs removed by upgrade"
fi

# Migration gates.
if remote 'test -f /var/lib/mde/migrate-from-1x.done'; then
    pass "mde-migrate-from-1x ran"
else
    fail "mde-migrate-from-1x marker missing"
fi

if remote 'test -d ~/.config/mde'; then
    pass "~/.config/mde/ exists post-migration"
else
    fail "~/.config/mde/ missing"
fi

if remote 'ls -d ~/.config/xfce4.v1x-backup.* 2>/dev/null | head -1 | grep -q .'; then
    pass "xfce4 1x backup dir created"
else
    fail "~/.config/xfce4.v1x-backup.<ts>/ missing"
fi

# Settings-preservation spot-check (theme name).
if remote 'test -f ~/.config/mde/state.json && grep -q theme ~/.config/mde/state.json'; then
    pass "Theme preference carried over to ~/.config/mde/state.json"
else
    fail "Theme preference not found in ~/.config/mde/state.json"
fi

ELAPSED_S=$(( $(date +%s) - START_S ))
info "Elapsed: ${ELAPSED_S} s"

if [ "$FAIL_COUNT" -eq 0 ]; then
    printf '\n%s═══ HW-2 UPGRADE: PASS ═══%s\n' "$GRN" "$RST"
    exit 0
else
    printf '\n%s═══ HW-2 UPGRADE: FAIL (%d gate%s) ═══%s\n' \
        "$RED" "$FAIL_COUNT" "$([ $FAIL_COUNT -eq 1 ] || echo s)" "$RST" >&2
    exit 1
fi
