#!/bin/bash
# mde-parity-overlay — sync mackes-shell repo updates onto the running
# installed MDE without cutting a fresh RPM. v4.0.1 (PARITY-1).
#
# Two-phase design:
#   --build  (runs as the developer user mm) — `cargo build --release
#            --workspace` so any Rust crate changes produce fresh
#            binaries in target/release/.
#   --install (runs as root via sudo) — copy newer Python modules into
#            site-packages, newer .desktop files into /usr/share/
#            applications/, newer cargo binaries into /usr/bin/.
#            Refresh desktop database + icon cache. Lock-protected.
#
# Invoked with no arguments (the systemd-user .service path) it runs
# --build first, then re-execs itself via `sudo -n` to enter --install.
# The sudoers drop-in (install-helpers/sudoers-mde-parity) grants the
# operator passwordless exec of EXACTLY this script.
#
# Idempotent: a no-change run prints the timestamp + "summary: py=0
# desktop=0 bin=0" and exits 0. Safe to invoke from a systemd .path
# unit that fires on every .git/refs/heads/main update.

set -euo pipefail

# ---- Configuration ---------------------------------------------------------

REPO=${MDE_PARITY_REPO:-/home/mm/Desktop/files/mackes-shell}
DEVUSER=${MDE_PARITY_USER:-mm}
SITE=${MDE_PARITY_SITE_PACKAGES:-/usr/lib/python3.14/site-packages}
APPS=${MDE_PARITY_APPS:-/usr/share/applications}
BIN=${MDE_PARITY_BIN:-/usr/bin}
LOCK=${MDE_PARITY_LOCK:-/run/mde-parity.lock}
LOG=${MDE_PARITY_LOG:-/var/log/mde-parity.log}

# Whitelist of binary-name prefixes we will install. Anything outside
# this list in target/release/ (e.g. example binaries, build deps) is
# skipped. Keeps the privileged copy path tightly scoped.
BIN_WHITELIST_PREFIXES=("mde-" "mde_" "mded" "mackes-" "mackesd")

# ---- Helpers ---------------------------------------------------------------

log() { printf '%s %s\n' "$(date -Iseconds)" "$*" >&2; }

ensure_log() {
    if [ ! -f "$LOG" ]; then
        install -m 0664 -o root -g root /dev/null "$LOG"
    fi
}

is_whitelisted_bin() {
    local base="$1"
    for p in "${BIN_WHITELIST_PREFIXES[@]}"; do
        case "$base" in "$p"*) return 0 ;; esac
    done
    return 1
}

# ---- Phase 1: build (runs as $DEVUSER) -------------------------------------

phase_build() {
    [ "$(id -un)" = "$DEVUSER" ] || {
        log "ERROR: --build must run as $DEVUSER (currently $(id -un))"
        exit 1
    }
    cd "$REPO"
    log "phase=build cargo build --release --workspace"
    # cargo output goes to the operator's stdout/stderr only. The log
    # file is root-owned and writable only by the install phase; if we
    # tried to tee here from the unprivileged build phase, the failed
    # tee would mask cargo's actual exit status and the install phase
    # would never fire. So: rely on cargo's own progress output, and
    # gate FAILED on cargo's real exit status via PIPESTATUS.
    cargo build --release --workspace
    rc=$?
    if [ "$rc" -ne 0 ]; then
        log "phase=build FAILED (cargo exit=$rc)"
        return 1
    fi
    log "phase=build done"
}

# ---- Phase 2: install (runs as root) ---------------------------------------

phase_install() {
    [ "$EUID" -eq 0 ] || {
        log "ERROR: --install must run as root (currently uid $EUID)"
        exit 1
    }
    ensure_log
    exec 9>"$LOCK"
    if ! flock -n 9; then
        log "another overlay holds $LOCK; exiting"
        exit 0
    fi

    {
        log "==== phase=install starting ===="

        # 1) Python overlay
        local py_changed=0
        while IFS= read -r f; do
            local rel="${f#mackes/}"
            local src="$REPO/$f"
            local dst="$SITE/mackes/$rel"
            if [ ! -f "$dst" ] || [ "$src" -nt "$dst" ]; then
                install -D -m 0644 "$src" "$dst"
                log "py: $rel"
                py_changed=$((py_changed + 1))
            fi
        done < <(cd "$REPO" && find mackes -type f -name '*.py')
        if [ "$py_changed" -gt 0 ]; then
            find "$SITE/mackes" -name '__pycache__' -prune -exec rm -rf {} + 2>/dev/null || true
        fi

        # 2) .desktop files
        local desktop_changed=0
        if [ -d "$REPO/data/applications" ]; then
            for f in "$REPO"/data/applications/*.desktop; do
                [ -f "$f" ] || continue
                local dst="$APPS/$(basename "$f")"
                if [ ! -f "$dst" ] || [ "$f" -nt "$dst" ]; then
                    install -m 0644 "$f" "$dst"
                    log "desktop: $(basename "$f")"
                    desktop_changed=$((desktop_changed + 1))
                fi
            done
        fi
        if [ "$desktop_changed" -gt 0 ] && command -v update-desktop-database >/dev/null; then
            update-desktop-database "$APPS" 2>/dev/null || true
        fi

        # 3) Rust binaries from target/release/
        local bin_changed=0
        if [ -d "$REPO/target/release" ]; then
            for src in "$REPO"/target/release/*; do
                [ -f "$src" ] || continue
                [ -x "$src" ] || continue
                local base
                base="$(basename "$src")"
                is_whitelisted_bin "$base" || continue
                local dst="$BIN/$base"
                if [ ! -f "$dst" ] || [ "$src" -nt "$dst" ]; then
                    install -m 0755 "$src" "$dst"
                    log "bin: $base"
                    bin_changed=$((bin_changed + 1))
                fi
            done
        fi

        log "summary: py=$py_changed desktop=$desktop_changed bin=$bin_changed"
        log "==== phase=install done ===="
    } 2>&1 | tee -a "$LOG"
}

# ---- Default entry: build then install -------------------------------------

main() {
    case "${1:-}" in
        --build)   phase_build ;;
        --install) phase_install ;;
        "")
            # Build as the current user, then re-exec via sudo for install.
            "$0" --build
            log "re-exec via sudo -n for install phase"
            exec sudo -n "$0" --install
            ;;
        *)
            cat >&2 <<USAGE
usage: $0 [--build|--install]
  --build    (run as $DEVUSER) cargo build --release --workspace
  --install  (run as root)     overlay-install changed files
  (no args)  build then sudo-exec install
USAGE
            exit 2
            ;;
    esac
}

main "$@"
