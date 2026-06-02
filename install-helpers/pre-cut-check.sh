#!/bin/sh
# install-helpers/pre-cut-check.sh — §0.17 "v5.0.0 SHIPS EVERYTHING"
# enforcement (TUNE-7, locked 2026-05-26 per Q11 + Q12 of the 25-Q
# tuning survey; broadened from the retired INST-/DM- subset to the
# FULL Active worklist 2026-06-02 per §0.17 + §0 cut-shorthand step 0).
#
# Refuses the cut when ANY task in the worklist's Active section is
# still Open `[ ]` or In-Progress `[>]`. Per the 2026-05-30 operator
# directive (§0.17, "nothing is post 5.0"), v5.0.0 ships the FULL
# locked worklist — every open/in-progress Active task is a cut-
# blocker, regardless of epic prefix. The old §11.1 "shippable core"
# gate (INST-/DM- only) is retired; this scans the whole section.
#
# Hard block per Q12 — no operator override flag, no env-var bypass,
# no "force" mode. The legitimate paths past this gate: ship the work
# to `[✓]`, or the operator deletes the task from the worklist as
# out-of-scope (lock-amendment per §0.16; never auto-proposed per §0.17).
#
# `[!] Blocked` tasks are reported as INFORMATIONAL, not hard blocks:
# §0.17's literal text names only `[ ]`/`[>]` as cut-blockers, and the
# worklist's pre-release-verification note records the cut-process
# `[!]` items (version-bump-at-cut-time, CHANGELOG, etc.) as expected
# to remain blocked through step 0. They surface in the report for the
# operator's eye but do not refuse the cut.
#
# Per §0.6 cut-release shorthand step 0: this script must exit 0
# before `cut release X.Y.Z` proceeds to step 1 (version bump).
#
# Per §0.15: HW-* tasks gate the cut via their per-bullet schema;
# this script verifies the task-level [✓] but the operator-typed
# bench-green confirmation on each HW-* sub-bullet is the
# substantive check.
#
# Exit codes:
#   0 = clean. Every Active-section task is [✓] Done (or [!] Blocked,
#       informational), and every HW-* task is closed (task-level +
#       per-bullet).
#   1 = at least one Active task is [ ] Open or [>] In-Progress, or an
#       HW-* task has open work. Output lists the counts, a per-epic-
#       family histogram, and a sample title.
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

# §0.17 full-worklist gate (2026-06-02): the cut blocks on EVERY open
# / in-progress task in the Active section, not a hand-picked prefix
# subset. The retired INST-/DM- "shippable core" list is gone — per
# §0.17 there is no core-vs-rest split; the whole locked worklist is
# the cut definition, so there's no ROADMAP_PREFIXES list to maintain.

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

# §0.17 full-section scan. Count every open / in-progress / blocked
# task header in the Active slice, regardless of epic prefix.
#   `^- [ ] **`     = Open        → blocks the cut
#   `^- [>] ... **` = In-Progress → blocks the cut (it didn't close)
#   `^- [!] **`     = Blocked     → informational only (see header note)
OPEN_COUNT=$(grep -cE '^- \[ \] \*\*' "$ACTIVE_TMP" || true)
INPROGRESS_COUNT=$(grep -cE '^- \[>\][^*]*\*\*' "$ACTIVE_TMP" || true)
BLOCKED_COUNT=$(grep -cE '^- \[!\] \*\*' "$ACTIVE_TMP" || true)

# Per-epic-family histogram of the blocking (open + in-progress)
# tasks so the operator sees which epics still carry work. Family =
# the alnum run right after the `**` (Portal-5.c → Portal, BUS-2.7.c
# → BUS, KDC2-6.8 → KDC2, NF-6.x → NF).
EPIC_HIST=$(awk '
    /^- \[ \] \*\*/ || /^- \[>\][^*]*\*\*/ {
        if (match($0, /\*\*[A-Za-z0-9]+/)) {
            fam = substr($0, RSTART + 2, RLENGTH - 2);
            count[fam]++;
        }
    }
    END { for (f in count) printf "  %-14s %d\n", f, count[f]; }
' "$ACTIVE_TMP" | sort -k2 -rn)

# First open / in-progress sample title, truncated for the report.
SAMPLE=$(grep -E '^- \[( |>)\][^*]*\*\*' "$ACTIVE_TMP" | head -1 | cut -c1-120)

# TUNE-8 (Q13 of 25-Q tuning survey, 2026-05-26) — HW-* per-bullet
# verification. HW tasks live in their own "Epic: Hardware Testing"
# section OUTSIDE the Active block, so the prefix scan above doesn't
# see them. §11 row 15 ("Operator's full 8-peer fleet HW bench
# green") makes HW gating real for the cut; per-bullet schema means
# each `[ ]` sub-bullet under an HW-* task must be `[✓]` before the
# task counts as done.
#
# Behavior: scan the WHOLE worklist (Active + Hardware Testing
# section). For each `^- \[<mark>\] \*\*HW-` task header found,
# advance to the next blank line or next top-level list item +
# collect every indented `^    - \[<mark>\]` sub-bullet within
# that range. If the task header is `[ ]` OR any sub-bullet is
# `[ ]`, the HW item counts as incomplete.
#
# Current state (2026-05-26): HW-1..HW-4 are all [✓] with no
# per-bullet sub-bullets — the operator-typed bench-green
# confirmations from the original task bodies. Future HW-5+
# tasks (the 8-peer bench fleet per Q13) will use per-bullet
# schema; this scan catches incomplete bullets at cut time.

HW_OPEN=0
HW_DETAIL=""

# awk pass: emit "OPEN <line> <title>" for every HW-* task header
# that's still `[ ]` OR has at least one `[ ]` sub-bullet. The
# scan starts at the task header line + ends at the next list-
# item header (`^- ` not nested) or section header.
HW_RESULTS=$(awk '
    function emit(line, title, reason) {
        print "OPEN|" line "|" reason "|" title;
    }
    function flush_and_reset(   reason) {
        if (in_task) {
            if (task_open || bullet_open) {
                reason = task_open ? "task-level" : "per-bullet";
                emit(task_line, title, reason);
            }
            in_task = 0;
        }
    }
    # HW-* task header — open mark.
    /^- \[[ >!]\] \*\*HW-/ {
        flush_and_reset();
        in_task = 1;
        task_line = NR;
        task_open = 1;
        bullet_open = 0;
        match($0, /\*\*HW-[^*]+\*\*/);
        title = substr($0, RSTART, RLENGTH);
        gsub(/\*\*/, "", title);
        next;
    }
    # HW-* task header — closed mark.
    /^- \[✓\] \*\*HW-/ {
        flush_and_reset();
        in_task = 1;
        task_line = NR;
        task_open = 0;
        bullet_open = 0;
        match($0, /\*\*HW-[^*]+\*\*/);
        title = substr($0, RSTART, RLENGTH);
        gsub(/\*\*/, "", title);
        next;
    }
    # End-of-task sentinels: next top-level list item OR section
    # header. The 4-space-indented sub-bullets dont match `^- `.
    in_task && /^- \[/ { flush_and_reset(); }
    in_task && /^## / { flush_and_reset(); }
    in_task && /^### / { flush_and_reset(); }
    # Indented sub-bullet (4 spaces) inside the task body.
    in_task && /^    - \[ \]/ { bullet_open = 1; }
    in_task && /^    - \[>\]/ { bullet_open = 1; }
    in_task && /^    - \[!\]/ { bullet_open = 1; }
    END { flush_and_reset(); }
' "$WORKLIST")

if [ -n "$HW_RESULTS" ]; then
    HW_OPEN=$(printf '%s\n' "$HW_RESULTS" | wc -l | tr -d ' ')
    HW_DETAIL=$(printf '%s\n' "$HW_RESULTS" | awk -F'|' '
        { printf("  %s (line %s, %s open)\n", $4, $2, $3) }
    ')
fi

TOTAL=$((OPEN_COUNT + INPROGRESS_COUNT + HW_OPEN))

if [ "$TOTAL" -gt 0 ]; then
    cat >&2 <<EOF
$0: REFUSING THE CUT — the Active worklist still has open work.

§0.17 (v5.0.0 ships the FULL worklist): every [ ] Open and [>] In-
Progress task in the Active section of docs/PROJECT_WORKLIST.md is a
cut-blocker. Current Active state:

  ${OPEN_COUNT} open          ([ ])
  ${INPROGRESS_COUNT} in-progress   ([>])
  ${HW_OPEN} open HW-*       (task-level OR per-bullet, §0.15)
  ${BLOCKED_COUNT} blocked       ([!] — informational, NOT counted toward the block)

Open / in-progress work by epic family:
${EPIC_HIST}

Sample: ${SAMPLE}...

Per CLAUDE.md §0.17 + §0 cut-shorthand step 0 this is a HARD BLOCK
with no operator override flag.
EOF
    if [ "$HW_OPEN" -gt 0 ]; then
        cat >&2 <<EOF

Open HW-* tasks (§0.15 + Q13 per-bullet schema — bench-green required):
${HW_DETAIL}
EOF
    fi
    if [ "$BLOCKED_COUNT" -gt 0 ]; then
        printf '\n[!] Blocked (informational — do NOT block the cut per §0.17; each\nshould be a cut-process or operator-tracked carve-out):\n' >&2
        grep -E '^- \[!\] \*\*' "$ACTIVE_TMP" \
            | cut -c1-100 | sed 's/^- \[!\] /  /' | head -20 >&2
    fi
    cat >&2 <<EOF

The cut proceeds when:
  (a) Every [ ]/[>] task above is [✓] Done per the §0.8 Definition of
      Done (HW-* also needs every per-bullet AC [✓] with operator-
      confirmed bench results per §0.15), or
  (b) The operator deletes a task from the worklist as out-of-scope
      (lock-amendment per §0.16; never auto-proposed by Claude per
      §0.17).

Until then, /ship the remaining work. See CLAUDE.md §0.17.
EOF
    exit 1
fi

echo "$0: clean — every Active-section task is [✓] Done (or [!] blocked/informational) and every HW-* task is closed (task-level + per-bullet)."
exit 0
