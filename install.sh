#!/usr/bin/env bash
# Mackes Desktop Environment (MDE) — bootstrap installer.
#
#   curl -fsSL https://raw.githubusercontent.com/matthewmackes/MAP2-RELEASES/main/install.sh | bash
#
# Phases (each shown as a styled box in the terminal):
#   1. Detect Fedora release + architecture
#   2. Resolve the latest release tag from GitHub
#   3. Download the base mde-core RPM (with spinner)
#   4. Install via dnf (live dimmed log lines, not a silent multi-minute wait)
#   5. Add the MDE dnf repo (so mde-desktop + upgrades resolve)
#   6. Converge the node with `sudo mde-install` (profile picker → birthrights)

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
    printf '%b│%b  %bMackes Desktop Environment (MDE)%b  %b· installer%b%*s%b│%b\n' \
        "$C_ACCENT" "$C_RESET" \
        "$C_BOLD" "$C_RESET" \
        "$C_DIM" "$C_RESET" \
        $((61 - 50)) " " \
        "$C_ACCENT" "$C_RESET"
    printf '%b│%b  %bChromeOS Classic · Wayland · Fedora%b%*s%b│%b\n' \
        "$C_ACCENT" "$C_RESET" \
        "$C_DIM" "$C_RESET" \
        $((61 - 39)) " " \
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

TOTAL=6

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
# Pull the full /releases/latest JSON once and use the assets list it ships
# so we don't have to guess the RPM filename. The Phase 10.1 rename
# (mackes-shell → mackes-xfce-workstation) moved the package name, and any
# future rename would break a hardcoded URL again. Parsing assets[].name
# keeps the installer working across renames.
release_json="$(curl -fsSL "$GH_API" 2>"$LOG" || true)"
tag="$(printf '%s' "$release_json" | grep -oP '"tag_name":\s*"\K[^"]+' | head -1 || true)"
if [ -z "$tag" ]; then
    phase_fail "no release tag for $REPO"
    err "Could not resolve latest release tag — see $LOG"
fi
version="${tag#v}"

# Look up the matching x86_64 RPM by suffix so the package can rename
# without breaking the installer. Accepts either the legacy mackes-shell-
# prefix or the renamed mackes-xfce-workstation- prefix.
rpm_name="$(printf '%s' "$release_json" \
    | grep -oP '"name":\s*"\K[^"]+\.fc'"$fedora_ver"'\.'"$arch"'\.rpm' \
    | grep -v '\.src\.rpm$' \
    | head -1 || true)"
if [ -z "$rpm_name" ]; then
    phase_fail "no .fc${fedora_ver}.${arch} RPM in $tag"
    err "Latest release ($tag) ships no fc${fedora_ver}.${arch} RPM — see $LOG"
fi
rpm_url="https://github.com/$REPO/releases/download/$tag/$rpm_name"
phase_ok "$tag · $rpm_name"

# ---- Phase 3: download RPM ------------------------------------------------
phase_start 3 $TOTAL "Download RPM"
run_with_spinner "$LOG" "downloading $tag…" -- \
    curl -fL --silent --show-error -o "$TMP/$rpm_name" "$rpm_url"
if [ ! -f "$TMP/$rpm_name" ] || [ ! -s "$TMP/$rpm_name" ]; then
    phase_fail "download failed"
    err "$(tail -n 3 "$LOG" 2>/dev/null)"
fi
size="$(du -h "$TMP/$rpm_name" | cut -f1)"
phase_ok "$size"

# ---- Phase 4: dnf install -------------------------------------------------
phase_start 4 $TOTAL "Install RPM (dnf — can take a few minutes)"
printf '\n'                # newline before live tail

(
    sudo dnf install -y "$TMP/$rpm_name" 2>&1 | tee "$LOG" \
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

# ---- Phase 5: add the MDE dnf repo ----------------------------------------
# So `mde-install --profile=full` can pull mde-desktop, and so future
# `dnf upgrade` resolves. The helper ships in the base RPM we just
# installed (A1: data lives under /usr/share/mde/).
phase_start 5 $TOTAL "Add MDE dnf repo"
if [ -x /usr/share/mde/install-helpers/add-mackes-repo.sh ]; then
    if sudo /usr/share/mde/install-helpers/add-mackes-repo.sh >>"$LOG" 2>&1; then
        phase_ok "repo configured"
    else
        phase_fail "repo helper failed (continuing; mde-desktop may need manual install)"
    fi
else
    phase_ok "skipped (helper not present)"
fi

# ---- Phase 6: converge with mde-install -----------------------------------
# A3 — the base RPM is installed; now converge the node. `mde-install`
# is the canonical convergence tool (the profile picker → wipe →
# birthrights flow). On `full` it pulls mde-desktop itself (A4), so the
# Fedora-Server-CLI build-up path works from one command.
phase_start 6 $TOTAL "Converge with mde-install"
if [ ! -x /usr/bin/mde-install ]; then
    phase_fail "mde-install missing — base RPM may be incomplete"
    err "Expected /usr/bin/mde-install from the mde-core RPM; see $LOG"
fi
phase_ok "starting"
printf '\n%b%s%b  %bRunning %bsudo mde-install%b — pick a profile (Full builds the desktop).%b\n\n' \
    "$C_DIM" "$DOT" "$C_RESET" \
    "$C_DIM" "$C_TEXT" "$C_DIM" "$C_RESET"
# Hand the real terminal to mde-install for its interactive picker +
# typed-NUKE confirm. Under `curl … | bash`, this script's stdin is
# the pipe (not a TTY), so we redirect mde-install's stdin from
# /dev/tty. When there's no controlling terminal (CI / fully
# unattended), fall back to printing the manual command.
if [ -e /dev/tty ]; then
    exec sudo /usr/bin/mde-install < /dev/tty
else
    printf '%b%s%b  %bNo terminal for the profile picker.%b Finish setup with:\n' \
        "$C_ACCENT" "$DOT" "$C_RESET" "$C_BOLD" "$C_RESET"
    printf '       %bsudo mde-install --profile=full%b   (or lighthouse|headless)\n\n' \
        "$C_TEXT" "$C_RESET"
fi
