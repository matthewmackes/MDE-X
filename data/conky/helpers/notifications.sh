#!/bin/sh
# Conky helper — Global mesh notifications count.
NOTIF_BUCKET="$HOME/QNM-Shared/.qnm-sync/notifications"
LOCAL_BUCKET="$HOME/QNM-Notifications/mine"
if [ -d "$NOTIF_BUCKET" ]; then
    TOTAL=$(find "$NOTIF_BUCKET" -type f 2>/dev/null | wc -l)
else
    TOTAL=0
fi
if [ -d "$LOCAL_BUCKET" ]; then
    LOCAL=$(find "$LOCAL_BUCKET" -type f 2>/dev/null | wc -l)
else
    LOCAL=0
fi
printf '  mesh · %s  me · %s\n' "${TOTAL}" "${LOCAL}"
# Most recent notification (file mtime) if any
LATEST=$(find "$NOTIF_BUCKET" "$LOCAL_BUCKET" -type f -printf '%T@ %p\n' 2>/dev/null \
         | sort -nr | head -n1 | awk '{print $2}')
if [ -n "$LATEST" ]; then
    NAME=$(basename "$LATEST")
    printf '  latest · %.40s\n' "$NAME"
fi
