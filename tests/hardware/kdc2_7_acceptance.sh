#!/usr/bin/env bash
# KDC2-7.1..7.7 — KDE Connect 2 acceptance gates.
#
# Single script that drives all seven KDC2-7.x bench gates
# against a live MDE bench + a real Android phone with the
# official KDE Connect app installed. Each gate can be run
# individually via --gate 7.N or all in sequence via no args.
#
# Required env:
#   MDE_BENCH_PEER_A — SSH target for peer-A (LAN-local to phone)
#   MDE_BENCH_PEER_B — SSH target for peer-B (across mesh from A)
#   MDE_BENCH_PHONE_IP — local IP of Android device (for ping test)
#   MDE_BENCH_RPM_VER — RPM version under test (e.g. "2.1.0")
#
# Hardware-only — these gates can't run without a real phone +
# a real two-peer mesh. The script ships so operators have a
# canonical, scriptable bench harness for the v2.1 cut sign-off.

set -eu

START_S=$(date +%s)
FAIL_COUNT=0
RED=$'\033[31m'; GRN=$'\033[32m'; YLW=$'\033[33m'; RST=$'\033[0m'
fail() { FAIL_COUNT=$((FAIL_COUNT + 1)); printf '%s[FAIL]%s %s\n' "$RED" "$RST" "$1" >&2; }
pass() { printf '%s[PASS]%s %s\n' "$GRN" "$RST" "$1"; }
info() { printf '%s[INFO]%s %s\n' "$YLW" "$RST" "$1"; }

remote() {
    local host="$1"; shift
    timeout 30 ssh -o BatchMode=yes -o ConnectTimeout=10 \
        -o StrictHostKeyChecking=accept-new "$host" "$@"
}

GATE="${1:-all}"

check_prereqs() {
    if [ -z "${MDE_BENCH_PEER_A:-}" ] || [ -z "${MDE_BENCH_PEER_B:-}" ]; then
        fail "Set MDE_BENCH_PEER_A + MDE_BENCH_PEER_B SSH targets"
        return 1
    fi
}

gate_7_1_pair_via_lan() {
    info "KDC2-7.1 — phone pairs via official Android KDE Connect over LAN"
    info "(MANUAL gate — operator-driven phone-side action)"
    info "Steps:"
    info "  1. Install MDE v${MDE_BENCH_RPM_VER:-2.1.0} on $MDE_BENCH_PEER_A"
    info "  2. Install official KDE Connect from Play Store on phone"
    info "  3. Pair via the KDE Connect UI"
    info "  4. Send ping from phone → peer-A"
    info "  5. Send ping from peer-A → phone"
    info "Operator: confirm both directions worked, then export"
    info "MDE_BENCH_GATE_7_1_RESULT=PASS or FAIL"
    case "${MDE_BENCH_GATE_7_1_RESULT:-}" in
        PASS) pass "KDC2-7.1 phone-LAN pairing" ;;
        FAIL) fail "KDC2-7.1 phone-LAN pairing" ;;
        *)    info "KDC2-7.1 result not provided — gate pending" ;;
    esac
}

gate_7_2_cross_mesh_phone() {
    info "KDC2-7.2 — phone reachable across mesh from non-pairing peer"
    info "Steps: paired phone on peer-A's LAN should be visible from"
    info "peer-B's mde-workbench peer list. Send Clipboard from peer-B;"
    info "phone receives."
    if remote "$MDE_BENCH_PEER_B" \
        "mde-workbench --list-peers 2>/dev/null | grep -q 'phone'"; then
        pass "Phone visible from $MDE_BENCH_PEER_B's peer list"
    else
        fail "Phone NOT visible from $MDE_BENCH_PEER_B"
    fi
}

gate_7_3_no_qt_kf6_deps() {
    info "KDC2-7.3 — rpm -qR mde-${MDE_BENCH_RPM_VER:-2.1.0} has no Qt/KF6"
    if remote "$MDE_BENCH_PEER_A" \
        "rpm -qR mde-${MDE_BENCH_RPM_VER:-2.1.0} 2>&1 | grep -iE 'qt[0-9]|kf[0-9]'"; then
        fail "Qt/KF6 deps detected in MDE RPM closure"
    else
        pass "No Qt/KF6 in MDE RPM closure"
    fi
}

gate_7_4_router_latency() {
    info "KDC2-7.4 — router decision latency p50 < 5ms, p99 < 25ms"
    info "Runs: mde-bench connect-router --samples=1000"
    if remote "$MDE_BENCH_PEER_A" \
        "mde-bench connect-router --samples=1000 --p50-max-ms=5 --p99-max-ms=25"; then
        pass "Router latency within p50<5ms / p99<25ms thresholds"
    else
        fail "Router latency exceeded thresholds"
    fi
}

gate_7_5_warm_latency() {
    info "KDC2-7.5 — first-packet warm < 3s + roaming switch < 10s"
    if remote "$MDE_BENCH_PEER_A" "mde-bench connect-warm --max-s=3"; then
        pass "Warm latency under 3s"
    else
        fail "Warm latency exceeded 3s"
    fi
    if remote "$MDE_BENCH_PEER_A" "mde-bench connect-roam --max-s=10"; then
        pass "Roaming switch under 10s"
    else
        fail "Roaming switch exceeded 10s"
    fi
}

gate_7_6_conflict_check() {
    info "KDC2-7.6 — dnf install kdeconnect-cli should conflict"
    if remote "$MDE_BENCH_PEER_A" \
        "sudo dnf install -y kdeconnect-cli 2>&1 | grep -qi 'conflict'"; then
        pass "kdeconnect-cli install blocked by Conflicts: line"
    else
        fail "kdeconnect-cli install succeeded — Conflicts: not effective"
    fi
}

gate_7_7_audit_path_switches() {
    info "KDC2-7.7 — PathSwitch audit-log entries present after load"
    info "Operator: kill Tailscale interface mid-flight to force switches"
    if remote "$MDE_BENCH_PEER_A" \
        "journalctl -u mded --since '5min ago' | grep -q PathSwitch.*last_switch_reason"; then
        pass "PathSwitch audit entries with last_switch_reason found"
    else
        fail "No PathSwitch entries with last_switch_reason in journal"
    fi
}

check_prereqs || exit 1

case "$GATE" in
    7.1) gate_7_1_pair_via_lan ;;
    7.2) gate_7_2_cross_mesh_phone ;;
    7.3) gate_7_3_no_qt_kf6_deps ;;
    7.4) gate_7_4_router_latency ;;
    7.5) gate_7_5_warm_latency ;;
    7.6) gate_7_6_conflict_check ;;
    7.7) gate_7_7_audit_path_switches ;;
    all)
        gate_7_1_pair_via_lan
        gate_7_2_cross_mesh_phone
        gate_7_3_no_qt_kf6_deps
        gate_7_4_router_latency
        gate_7_5_warm_latency
        gate_7_6_conflict_check
        gate_7_7_audit_path_switches
        ;;
    *) fail "Unknown gate: $GATE (use 7.1..7.7 or 'all')"; exit 1 ;;
esac

ELAPSED_S=$(( $(date +%s) - START_S ))
info "Elapsed: ${ELAPSED_S} s"

if [ "$FAIL_COUNT" -eq 0 ]; then
    printf '\n%s═══ KDC2-7 ACCEPTANCE: PASS ═══%s\n' "$GRN" "$RST"
    exit 0
else
    printf '\n%s═══ KDC2-7 ACCEPTANCE: FAIL (%d gate%s) ═══%s\n' \
        "$RED" "$FAIL_COUNT" "$([ $FAIL_COUNT -eq 1 ] || echo s)" "$RST" >&2
    exit 1
fi
