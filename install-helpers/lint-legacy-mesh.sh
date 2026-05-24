#!/bin/sh
# install-helpers/lint-legacy-mesh.sh — NF-20.6 pre-commit gate.
#
# Catches NET-NEW `tailscale` / `headscale` / `derper` references
# in v2.5+ Nebula-native source. The v1.x legacy tree (Python
# mackes/, the legacy crates/mackes-panel/, the legacy mackesd
# workers/transport modules that NF-4.5 will retire) is allow-
# listed because those files still reference the legacy stack
# for backward-compat or as deletion targets. Going forward,
# any tailscale/headscale/derper hit OUTSIDE the allow-list is a
# regression — net-new code shouldn't be reaching for the dead
# stack.
#
# Per `.claude/CLAUDE.md` §0.7 gate #7. Mirrors `lint-voice.sh`'s
# scan + allow-list shape.
#
# Exits 0 = clean, exits 1 = violations found.

set -eu

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# Where to look. `.rs` + `.py` only — docs are not scanned.
SCAN_INCLUDES='--include=*.rs --include=*.py'
SCAN_PATHS='crates/ mackes/ tests/'

# Directory + file-prefix allow-list. Lines whose file path
# starts with ANY of these are allowed to mention the legacy
# vocabulary. Each entry is a prefix (matched via `case`-style
# pattern); add new entries one per line.
#
# The v1.x Python tree (`mackes/*`) is wholesale allow-listed —
# the wholesale-Python-retire epic (NF-5.x cluster) is the path
# that actually removes those files; until each lands a `[✓]`,
# their tailscale references are pre-existing, not net-new.
#
# crates/mackes-panel/ is the v1.x GTK panel, frozen by the
# v2.0.0 Iced rewrite. Won't be relabeled before retirement.
#
# Specific files under crates/mackesd/src/ that NF-4.5 will
# retire are also allow-listed pending that cascade.
ALLOWLIST_PREFIXES='
mackes/
crates/mackes-panel/
crates/mackesd/src/legacy_inventory.rs
crates/mackesd/src/workers/derp.rs
crates/mackesd/src/workers/perf.rs
crates/mackesd/src/workers/stun_gather.rs
crates/mackesd/src/workers/mesh_router.rs
crates/mackesd/src/https_fallback.rs
crates/mackesd/src/stun.rs
crates/mackesd/src/transport/https443.rs
crates/mackesd/src/topology/mod.rs
crates/mackes-transport/src/peer_path.rs
crates/mackes-nebula-https-tunnel/src/activation.rs
crates/mackesd/tests/integration_testcontainers.rs
tests/
crates/mde-workbench/src/panels/mesh_services.rs
'

LEGACY_PATTERN='\btailscale\|\bheadscale\|\bderper'

TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT

# shellcheck disable=SC2086
grep -rIn -i ${SCAN_INCLUDES} -e "$LEGACY_PATTERN" \
    $SCAN_PATHS 2>/dev/null > "$TMPFILE" || true

VIOLATIONS=0

while IFS= read -r raw; do
    [ -n "$raw" ] || continue
    fname=$(printf '%s' "$raw" | cut -d: -f1)
    body=$(printf '%s' "$raw" | cut -d: -f3-)

    # Allow lines that include a retraction-comment tag — these
    # are intentional "we retired X" notes left behind in
    # otherwise-Nebula-native files. Matches NF-N.M, GF-N.M,
    # RD-N, KDC2-N, or any of the verbs "retired", "retire",
    # "superseded", "deprecat", "legacy" appearing in the line.
    if printf '%s' "$body" | grep -qE '(NF-[0-9]+(\.[0-9]+)?|GF-[0-9]+(\.[0-9]+)?|RD-[0-9]+|KDC2-[0-9]+|\blegacy\b|\bretired\b|\bretire\b|\bsuperseded\b|\bdeprecat)'; then
        continue
    fi

    # Allow lines that are entirely inside a // or # comment.
    trimmed=$(printf '%s' "$body" | sed -e 's/^[[:space:]]*//')
    case "$trimmed" in
        '#'*|'//'*|'///'*|'//!'*|'/*'*|'*'*) continue ;;
    esac

    # Walk the allow-list. If the file path starts with any
    # prefix, allow.
    allowed=0
    while IFS= read -r prefix; do
        [ -n "$prefix" ] || continue
        case "$fname" in
            "$prefix"*) allowed=1; break ;;
        esac
    done <<EOF
$ALLOWLIST_PREFIXES
EOF
    if [ "$allowed" = 1 ]; then
        continue
    fi

    printf '%s\n' "$raw"
    VIOLATIONS=$((VIOLATIONS + 1))
done < "$TMPFILE"

if [ "$VIOLATIONS" -eq 0 ]; then
    printf 'lint-legacy-mesh.sh: clean (no net-new tailscale/headscale/derper hits in v2.5+ source)\n'
    exit 0
fi

printf '\nlint-legacy-mesh.sh: %d violation(s) — net-new legacy mesh vocabulary in v2.5+ source.\n' \
    "$VIOLATIONS" >&2
printf 'See .claude/CLAUDE.md §0.7 gate #7 + the allow-list inside this script.\n' >&2
exit 1
