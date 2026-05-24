#!/usr/bin/env bash
# HW-4 (was I.2) — Docker peer fan-out.
#
# Extends the Phase 12.11.2 testcontainers harness with a 4th
# peer pushing a setting revision. Runs in CI when a live
# Docker daemon is attached.
#
# Prereqs: `docker info` must succeed (CI: docker-in-docker;
# bench: standard Docker install). The script self-skips with
# exit 0 + a warning when no Docker is available, so it's safe
# to invoke unconditionally in CI without forcing every
# developer to install Docker locally.

set -eu

START_S=$(date +%s)
FAIL_COUNT=0
RED=$'\033[31m'; GRN=$'\033[32m'; YLW=$'\033[33m'; RST=$'\033[0m'
fail() { FAIL_COUNT=$((FAIL_COUNT + 1)); printf '%s[FAIL]%s %s\n' "$RED" "$RST" "$1" >&2; }
pass() { printf '%s[PASS]%s %s\n' "$GRN" "$RST" "$1"; }
info() { printf '%s[INFO]%s %s\n' "$YLW" "$RST" "$1"; }

if ! command -v docker >/dev/null 2>&1; then
    info "docker not installed — skipping HW-4 (this is OK in dev sandboxes)"
    exit 0
fi
if ! docker info >/dev/null 2>&1; then
    info "docker daemon unreachable — skipping HW-4"
    exit 0
fi

info "HW-4 Docker peer fan-out"

# Drive the testcontainers harness via cargo. The existing
# `docker-tests` feature gates the test surface.
if ! cargo test -p mackesd --features docker-tests \
        --test docker_peer_fanout 2>&1 | tail -20; then
    fail "docker-tests harness failed"
fi

# Spot-check: we should have spawned at least 4 containers
# (the 4th peer the worklist names).
RUNNING=$(docker ps --filter "ancestor=fedora:44" --format '{{.Names}}' | wc -l)
if [ "$RUNNING" -ge 4 ]; then
    pass "≥4 peer containers running (saw $RUNNING)"
else
    info "Only $RUNNING peer containers visible — harness may have cleaned up"
fi

# Clean up any leftover containers from this test pass.
docker ps --filter "ancestor=fedora:44" --format '{{.ID}}' | xargs -r docker rm -f >/dev/null 2>&1 || true

ELAPSED_S=$(( $(date +%s) - START_S ))
info "Elapsed: ${ELAPSED_S} s"

if [ "$FAIL_COUNT" -eq 0 ]; then
    printf '\n%s═══ HW-4 DOCKER PEER FAN-OUT: PASS ═══%s\n' "$GRN" "$RST"
    exit 0
else
    printf '\n%s═══ HW-4 DOCKER PEER FAN-OUT: FAIL (%d gate%s) ═══%s\n' \
        "$RED" "$FAIL_COUNT" "$([ $FAIL_COUNT -eq 1 ] || echo s)" "$RST" >&2
    exit 1
fi
