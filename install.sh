#!/usr/bin/env bash
# Mackes Shell — Carbon-styled curl-pipe-bash installer (v1.4.1).
#
#   curl -fsSL https://raw.githubusercontent.com/matthewmackes/MAP2-RELEASES/main/install.sh | bash
#
# Phases (each shown as a Carbon-styled box in the terminal):
#   1. Detect Fedora release + architecture
#   2. Resolve the latest release tag from GitHub
#   3. Download the RPM (with spinner)
#   4. Install via dnf (live dimmed log lines, not a silent multi-minute wait)
#   5. Hand off to the first-run wizard

set -euo pipefail

# ============================================================================
# Carbon palette — ANSI 256-color escapes that map cleanly to gray-100.
# ============================================================================

if [ -t 1 ] && [ "${TERM:-dumb}" != "dumb" ]; then
    C_ACCENT='\033[38;5;208m'      # Carbon orange 60 ~ Mackes accent
    C_DIM='\033[38;5;245m'         # gray-50
    C_TEXT='\033[38;5;255m'        # near-white
    C_OK='\033[38;5;78m'           # support-success
    C_WARN='\033[38;5;220m'        # support-warning
    C_FAIL='\033[38;5;203m'        # support-error
    C_BOLD='\033[1m'
    C_RESET='\033[0m'
    DOT='●'
    ARROW='▸'
    CHECK='✓'
    CROSS='✗'
    SPIN_FRAMES=(⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏)
else
    C_ACCENT='' C_DIM='' C_TEXT='' C_OK='' C_WARN='' C_FAIL=''
    C_BOLD='' C_RESET=''
    DOT='*' ARROW='>' CHECK='ok' CROSS='x'
    SPIN_FRAMES=('-' '\' '|' '/')
fi

# ============================================================================
# UI primitives
# ============================================================================

banner() {
    printf '\n'
    printf '%b┌─────────────────────────────────────────────────────────────┐%b\n' "$C_ACCENT" "$C_RESET"
    printf '%b│%b  %bMackes Shell%b  %b· installer%b%*s%b│%b\n' \
        "$C_ACCENT" "$C_RESET" \
        "$C_BOLD" "$C_RESET" \
        "$C_DIM" "$C_RESET" \
        $((61 - 27)) " " \
        "$C_ACCENT" "$C_RESET"
    printf '%b│%b  %bCarbon Design System chrome · XFCE · Fedora%b%*s%b│%b\n' \
        "$C_ACCENT" "$C_RESET" \
        "$C_DIM" "$C_RESET" \
        $((61 - 47)) " " \
        "$C_ACCENT" "$C_RESET"
    printf '%b└─────────────────────────────────────────────────────────────┘%b\n\n' "$C_ACCENT" "$C_RESET"
}

phase_start() {
    local n="$1" total="$2" name="$3"
    printf '%b%s%b  %bPhase %d/%d%b  %b%s%b ... ' \
        "$C_ACCENT" "$ARROW" "$C_RESET" \
        "$C_DIM" "$n" "$total" "$C_RESET" \
        "$C_BOLD" "$name" "$C_RESET"
}

phase_ok() {
    local detail="${1:-}"
    printf '%b%s%b' "$C_OK" "$CHECK" "$C_RESET"
    [ -n "$detail" ] && printf ' %b%s%b' "$C_DIM" "$detail" "$C_RESET"
    printf '\n'
}

phase_fail() {
    local detail="${1:-}"
    printf '%b%s%b' "$C_FAIL" "$CROSS" "$C_RESET"
    [ -n "$detail" ] && printf ' %b%s%b' "$C_FAIL" "$detail" "$C_RESET"
    printf '\n'
}

# Spinner that runs a command in the background.
# Usage: run_with_spinner <log-file> <description> -- <command...>
run_with_spinner() {
    local log="$1"
    local desc="$2"
    shift 2
    [ "$1" = "--" ] && shift

    ( "$@" >"$log" 2>&1 ) &
    local pid=$!

    local i=0
    while kill -0 "$pid" 2>/dev/null; do
        local frame="${SPIN_FRAMES[$i]}"
        printf '\r%b%s%b  %s' \
            "$C_ACCENT" "$frame" "$C_RESET" "$desc"
        i=$(( (i + 1) % ${#SPIN_FRAMES[@]} ))
        sleep 0.1
    done
    wait "$pid"
    local rc=$?
    printf '\r%80s\r' " "
    return $rc
}

# ============================================================================
# Sanity checks
# ============================================================================

err() {
    printf '\n%b%s install failed: %s%b\n' "$C_FAIL" "$CROSS" "$*" "$C_RESET" >&2
    exit 1
}

[ "$(id -u)" -ne 0 ] || err "Do not pipe this to sudo. The script asks for sudo only when it needs it."

# ============================================================================
# Run
# ============================================================================

banner

REPO="${MACKES_REPO:-matthewmackes/MAP2-RELEASES}"
GH_API="https://api.github.com/repos/$REPO/releases/latest"
LOG="$(mktemp -t mackes-install.XXXXXX.log)"
TMP="$(mktemp -d -t mackes-install.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

TOTAL=5

# ---- Phase 1: detect ------------------------------------------------------
phase_start 1 $TOTAL "Detect Fedora release"
command -v dnf  >/dev/null 2>&1 || { phase_fail "dnf not found"; err "Targets Fedora."; }
command -v rpm  >/dev/null 2>&1 || { phase_fail "rpm not found"; err "Targets Fedora."; }
command -v curl >/dev/null 2>&1 || { phase_fail "curl not found"; exit 1; }
fedora_ver="$(rpm -E %fedora)"
arch="$(uname -m)"
phase_ok "Fedora $fedora_ver · $arch"

# ---- Phase 2: resolve latest ----------------------------------------------
phase_start 2 $TOTAL "Resolve latest release"
tag="$(curl -fsSL "$GH_API" 2>"$LOG" | grep -oP '"tag_name":\s*"\K[^"]+' | head -1 || true)"
if [ -z "$tag" ]; then
    phase_fail "no release tag for $REPO"
    err "Could not resolve latest release tag — see $LOG"
fi
version="${tag#v}"
rpm_url="https://github.com/$REPO/releases/download/$tag/mackes-shell-${version}-1.fc${fedora_ver}.${arch}.rpm"
phase_ok "$tag"

# ---- Phase 3: download RPM ------------------------------------------------
phase_start 3 $TOTAL "Download RPM"
run_with_spinner "$LOG" "downloading $tag…" -- \
    curl -fL --silent --show-error -o "$TMP/mackes-shell.rpm" "$rpm_url"
if [ ! -f "$TMP/mackes-shell.rpm" ] || [ ! -s "$TMP/mackes-shell.rpm" ]; then
    phase_fail "download failed"
    err "$(tail -n 3 "$LOG" 2>/dev/null)"
fi
size="$(du -h "$TMP/mackes-shell.rpm" | cut -f1)"
phase_ok "$size"

# ---- Phase 4: dnf install -------------------------------------------------
phase_start 4 $TOTAL "Install RPM (dnf — can take a few minutes)"
printf '\n'                # newline before live tail

(
    sudo dnf install -y "$TMP/mackes-shell.rpm" 2>&1 | tee "$LOG" \
        | while IFS= read -r line; do
            # Print in a Carbon-dimmed style, truncated to ~72 chars
            printf '    %b%s%b\n' "$C_DIM" "${line:0:72}" "$C_RESET"
        done
)
dnf_rc=${PIPESTATUS[0]}
if [ "$dnf_rc" -ne 0 ]; then
    phase_start 4 $TOTAL "Install RPM"
    phase_fail "dnf exited rc=$dnf_rc — log at $LOG"
    err "Check $LOG for the full dnf transcript"
fi
phase_start 4 $TOTAL "Install RPM"
phase_ok "done"

# ---- Phase 5: hand off ----------------------------------------------------
phase_start 5 $TOTAL "Launch first-run wizard"
if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
    phase_ok "starting"
    printf '\n%b%s%b  %bRun %bmackes --wizard%b%b to re-open setup anytime.%b\n\n' \
        "$C_DIM" "$DOT" "$C_RESET" \
        "$C_DIM" \
        "$C_TEXT" "$C_DIM" \
        "$C_RESET" "$C_RESET"
    exec mackes
else
    phase_ok "headless — run mackes later"
    printf '\n%b%s%b  %bGraphical wizard:%b run %bmackes --wizard%b in any GUI session.%b\n' \
        "$C_DIM" "$DOT" "$C_RESET" \
        "$C_DIM" "$C_RESET" \
        "$C_TEXT" "$C_RESET" "$C_RESET"
    printf '%b%s%b  %bHeadless TUI:%b run %bmackes --tui%b in a terminal.%b\n\n' \
        "$C_DIM" "$DOT" "$C_RESET" \
        "$C_DIM" "$C_RESET" \
        "$C_TEXT" "$C_RESET" "$C_RESET"
fi
