#!/bin/sh
# Conky helper — Mackes admin-session lock state.
# Output: "unlocked" or "locked"
#
# Detection: the AdminSession keeps `sudo -n true` warm while it's
# unlocked. We probe with a non-prompting `sudo -n true` — exit 0 means
# we have cached creds, anything else means we're locked.
if sudo -n true 2>/dev/null; then
    printf 'unlocked'
else
    printf 'locked'
fi
