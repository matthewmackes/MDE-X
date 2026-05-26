#!/bin/sh
# install-helpers/pre-cut-check.sh — §0.17 NO INCOMPLETE RELEASES
# enforcement (TUNE-7, locked 2026-05-26 per Q11 + Q12 of the 25-Q
# tuning survey).
#
# Refuses the cut when any §11 1.0-roadmap item from
# `docs/AI_GOVERNANCE.md` still has an open task in the worklist.
# Hard block per Q12 — no operator override flag, no env-var
# bypass, no "force" mode. The only legitimate path past this gate
# is the operator typing "amend Q91 to drop <item>" (which removes
# the line from AI_GOVERNANCE.md §11 — then re-running this script
# passes automatically).
#
# Each row in `docs/AI_GOVERNANCE.md` §11 names an epic prefix
# (BUS-, GF-, DEAD-, CR-, INST-, DM-, etc.). The script greps the
# active section of `docs/PROJECT_WORKLIST.md` for `[ ] **<PREFIX>`
# or `[>] **<PREFIX>` markers — any hit means at least one task is
# open or in-progress for that roadmap item, and the cut refuses.
#
# Per §0.6 cut-release shorthand step 0: this script must exit 0
# before `cut release X.Y.Z` proceeds to step 1 (version bump).
#
# Per §0.15: HW-* tasks gate the cut via their per-bullet schema;
# this script verifies the task-level [✓] but the operator-typed
# bench-green confirmation on each HW-* sub-bullet is the
# substantive check. (The schema is "every sub-bullet [✓]" + then
# the task-level [✓] is set; this script reads the task-level mark.)
#
# Exit codes:
#   0 = clean. Every §11 roadmap epic is fully closed (or all rows
#       carry a §0.16-style operator-issued lift recorded inline).
#   1 = at least one §11 roadmap item has open work in the worklist.
#       The output lists the offending epic prefix + the count of
#       open tasks + a sample title.
#
# Usage:
#   make pre-cut-check       (the canonical entry point)
#   install-helpers/pre-cut-check.sh   (standalone)

set -eu

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

GOVERNANCE="docs/AI_GOVERNANCE.md"
WORKLIST="docs/PROJECT_WORKLIST.md"

if [ ! -f "$GOVERNANCE" ]; then
    echo "$0: $GOVERNANCE not found — script is for the MDE workspace." >&2
    exit 1
fi
if [ ! -f "$WORKLIST" ]; then
    echo "$0: $WORKLIST not found — script is for the MDE workspace." >&2
    exit 1
fi

# Locked roadmap-epic prefixes per `docs/AI_GOVERNANCE.md` §11
# (line 270 forward). Each prefix must show zero open tasks for
# the cut to proceed. If §11 grows, add the new prefix here.
#
# The list is intentionally explicit (not auto-extracted from the
# §11 table) so an unexpected design-doc edit doesn't silently
# change what the gate checks. Each entry must be reviewed at
# write-time.
ROADMAP_PREFIXES='
BUS-
GF-
DEAD-
CR-
INST-
DM-
TUNE-
Portal-
EPIC-RETIRE-PY-WORKBENCH
EPIC-RETIRE-PY-DAEMONS
EPIC-RETIRE-DBUS
EPIC-RETIRE-CARBON
EPIC-RETIRE-QNM
EPIC-RETIRE-CADDY
EPIC-MASTER-
EPIC-UI-MATERIAL
EPIC-UI-PRESETS
EPIC-PROC-
EPIC-SEC-
EPIC-SCOPE-
MON-
'

# Active section bounds — §11 only gates tasks in the "Active"
# section of the worklist; History / Future-deliverables / SUPERSEDED
# sections don't count. The Active section starts at the literal
# header `## Active` (around line 69) and ends at the next
# top-level header.
ACTIVE_START=$(grep -n '^## Active' "$WORKLIST" | head -1 | cut -d: -f1)
if [ -z "$ACTIVE_START" ]; then
    echo "$0: $WORKLIST has no '## Active' section header — schema drift." >&2
    exit 1
fi
ACTIVE_END=$(awk -v start="$ACTIVE_START" '
    NR > start && /^## / { print NR-1; found=1; exit }
    END { if (!found && NR > start) print NR }
' "$WORKLIST")

# Slice the active section into a working tempfile so subsequent
# greps are bounded.
ACTIVE_TMP=$(mktemp)
trap 'rm -f "$ACTIVE_TMP"' EXIT
sed -n "${ACTIVE_START},${ACTIVE_END}p" "$WORKLIST" > "$ACTIVE_TMP"

# Walk each roadmap prefix.
OPEN_COUNT=0
INPROGRESS_COUNT=0
OPEN_PREFIXES=""

for prefix in $ROADMAP_PREFIXES; do
    [ -z "$prefix" ] && continue
    # `[ ] **<prefix>` matches Open tasks.
    # `[>] **<prefix>` and `[>] session=... **<prefix>` match In-Progress.
    # We count both — In-Progress at cut time means the task didn't
    # close, so the cut refuses.
    open=$(grep -cE "^- \[ \] \*\*${prefix}" "$ACTIVE_TMP" || true)
    inprog=$(grep -cE "^- \[>\][^*]*\*\*${prefix}" "$ACTIVE_TMP" || true)
    blocked=$(grep -cE "^- \[!\] \*\*${prefix}" "$ACTIVE_TMP" || true)
    total=$((open + inprog + blocked))
    if [ "$total" -gt 0 ]; then
        OPEN_COUNT=$((OPEN_COUNT + open + blocked))
        INPROGRESS_COUNT=$((INPROGRESS_COUNT + inprog))
        OPEN_PREFIXES="${OPEN_PREFIXES}\n  ${prefix}: ${open} open, ${inprog} in-progress, ${blocked} blocked"
        # First sample title for each prefix, so the operator sees
        # what's blocking.
        sample=$(grep -E "^- \[( |>|!)\][^*]*\*\*${prefix}" "$ACTIVE_TMP" | head -1)
        if [ -n "$sample" ]; then
            # Truncate at 120 chars for readability.
            short=$(echo "$sample" | cut -c1-120)
            OPEN_PREFIXES="${OPEN_PREFIXES}\n    sample: ${short}..."
        fi
    fi
done

TOTAL=$((OPEN_COUNT + INPROGRESS_COUNT))

if [ "$TOTAL" -gt 0 ]; then
    cat >&2 <<EOF
$0: REFUSING THE CUT — §11 roadmap items still have open work.

$OPEN_COUNT open / blocked tasks + $INPROGRESS_COUNT in-progress tasks
across §11 roadmap epic prefixes. Per CLAUDE.md §0.17 (Q11 + Q12 of
25-Q tuning survey, 2026-05-26) this is a HARD BLOCK with no operator
override flag.

Open epics:
$(printf '%b' "$OPEN_PREFIXES")

The cut proceeds when one of these is true:
  (a) Every task above is marked [✓] in docs/PROJECT_WORKLIST.md
      with the §0.8 Definition of Done satisfied.
  (b) The operator types "amend Q91 to drop <epic>" and that epic
      line is removed from docs/AI_GOVERNANCE.md §11 + this script's
      ROADMAP_PREFIXES list. (Lock-amendment per §0.16 operator-
      override path; never auto-proposed by Claude per §0.17.)

Until then, /ship the remaining work. See CLAUDE.md §0.17.
EOF
    exit 1
fi

echo "$0: clean — every §11 roadmap epic prefix shows zero open work in the active worklist."
exit 0
