#!/bin/sh
# Conky helper — QNM-Shared bucket usage.
SHARED="$HOME/QNM-Shared"
if [ ! -d "$SHARED" ]; then
    echo "  (no shared mount)"
    exit 0
fi
# Total size of the bucket
SIZE=$(du -sh "$SHARED" 2>/dev/null | awk '{print $1}')
# Available space on the underlying filesystem
AVAIL=$(df -h --output=avail "$SHARED" 2>/dev/null | tail -n1 | tr -d ' ')
# Bucket count under .qnm-sync/
BUCKETS=$(ls -d "$SHARED/.qnm-sync/"*/ 2>/dev/null | wc -l)
printf '  size · %s  free · %s\n' "${SIZE:-?}" "${AVAIL:-?}"
printf '  buckets · %s\n' "${BUCKETS:-0}"
