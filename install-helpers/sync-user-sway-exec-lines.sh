#!/bin/bash
# sync-user-sway-exec-lines.sh — v4.0.1 BUG-11 systemic fix.
#
# Some users' ~/.config/sway/config was copied at v1.x by the birthright
# wizard and never refreshed when later MDE versions added new
# `exec mde-*` lines to data/sway/config. The most common symptom:
# the Win10 watermark + toast popovers never spawn (their `exec
# mde-popover {watermark,toast}` lines exist in the shipped config but
# not in the user's home copy).
#
# This script idempotently appends every required `exec mde-popover *`
# line that's MISSING from ~/.config/sway/config. It NEVER:
#   - reorders existing lines,
#   - removes user customizations,
#   - copies the whole system config over the user's,
#   - touches anything beyond the named exec lines.
#
# Each line we own is fingerprinted by an exact-match `grep -Fq` against
# the user's file. If the line is already present (with any leading
# whitespace), it's left alone.
#
# Invoked from mde-session.service ExecStartPost on every login.

set -u

USERCFG="${HOME}/.config/sway/config"

# If the user's config doesn't exist yet (fresh login on a brand-new
# account) the birthright wizard hasn't fired — bail out, the wizard
# will write the right file.
[ -f "$USERCFG" ] || exit 0

# Required exec / bindsym lines, in the order they should appear if
# appended. Adding a new entry here makes it stick on the next login
# of every existing operator without forcing them through the wizard
# again.
#
# v4.0.1 (2026-05-23): watermark stays here even though the visible
# widget retired — `mde-popover watermark` is now the headless dnf-
# poll daemon that maintains ~/.cache/mde/dnf-updates.count for the
# start-menu footer chip.
# v4.0.1 BUG-15: Super+Shift+M binding lets the operator recover
# minimized windows from sway's scratchpad — the panel's centered
# minimize button (BUG-6) has no native sway equivalent so the
# `move scratchpad` + `scratchpad show` pair is the closest UX.
# v4.0.1 BUG-17 (2026-05-23): `exec mde-popover toast` removed from
# REQUIRED_LINES until BUG-17 fixes the empty-stack grey-box
# surface. Users who already have the line in their config don't
# need it stripped here — they can manually remove it or it'll
# linger as a comment; the operator killed the running toast
# process in the same session so the visible bug is gone.
REQUIRED_LINES=(
    "exec mde-popover watermark"
    "bindsym \$mod+Shift+m exec swaymsg scratchpad show"
)

append_if_missing() {
    local line="$1"
    if grep -Fq "$line" "$USERCFG"; then
        return 0
    fi
    {
        printf '\n# v4.0.1 BUG-11 sync — appended by sync-user-sway-exec-lines.sh\n'
        printf '%s\n' "$line"
    } >>"$USERCFG"
    SWAY_NEEDS_RELOAD=1
}

# v4.0.1 BUG-16 sync — enforce the locked border + title-bar
# settings. Unlike append_if_missing, these are key=value-shaped:
# we replace the entire existing line in-place if its key matches,
# or append if absent. Pre-2026-05-23 user configs had
# `default_border pixel <N>` + `smart_borders on`; this commit
# locks `normal 4` + `title_align center` + `smart_borders no`
# so sway renders the title-bar that hosts the per-window
# min/max/close cluster.
ENFORCED_LINES=(
    "default_border normal 4"
    "default_floating_border normal 4"
    "title_align center"
    "smart_borders no"
)
SWAY_NEEDS_RELOAD=0

enforce_setting() {
    local target="$1"
    local key
    key="$(printf '%s' "$target" | awk '{print $1}')"
    # If the key already exists with the target value, no-op.
    if grep -qE "^${key}\\b" "$USERCFG"; then
        local current
        current="$(grep -E "^${key}\\b" "$USERCFG" | head -1)"
        if [ "$current" = "$target" ]; then
            return 0
        fi
        # Replace the first occurrence in-place. Use sed with a
        # delimiter that won't appear in sway config strings.
        sed -i "0,/^${key}\\b.*/{s||$target|}" "$USERCFG"
    else
        # Key absent — append.
        {
            printf '\n# v4.0.1 BUG-16 sync — enforced by sync-user-sway-exec-lines.sh\n'
            printf '%s\n' "$target"
        } >>"$USERCFG"
    fi
    SWAY_NEEDS_RELOAD=1
}

for line in "${REQUIRED_LINES[@]}"; do
    append_if_missing "$line"
done

for setting in "${ENFORCED_LINES[@]}"; do
    enforce_setting "$setting"
done

if [ "$SWAY_NEEDS_RELOAD" = "1" ]; then
    # Reload sway so the changes take effect this session, not just
    # the next one. `|| true` so a missing swaymsg (running outside
    # sway) doesn't fail the session bring-up.
    swaymsg reload >/dev/null 2>&1 || true
fi
