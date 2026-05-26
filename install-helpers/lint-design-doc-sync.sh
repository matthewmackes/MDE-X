#!/bin/sh
# install-helpers/lint-design-doc-sync.sh — TUNE-4 / 25-Q Q18
# pre-commit gate #15.
#
# Forces design-doc → worklist sync at write-time: every commit
# that touches `docs/design/<epic>.md` must ALSO touch
# `docs/PROJECT_WORKLIST.md`. The assumption: any design-doc
# edit either locks new actionable items (which must lift into
# the worklist) OR annotates existing items (which must reflect
# in the worklist's task bodies).
#
# Closes the failure mode where design-doc actions sit un-lifted
# for sessions, going unnoticed until a quarterly audit catches
# them. Per Q20 the quarterly audit retires anyway; this lint
# replaces the catch-up with at-write-time enforcement.
#
# Per CLAUDE.md §0.7 gate #15.
#
# Exits 0 = clean, exits 1 = sync violation.

set -eu

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# Get the staged files. If invoked outside a pre-commit context
# (no staged files), the lint short-circuits clean — no edits
# to evaluate.
STAGED="$(git diff --cached --name-only 2>/dev/null || true)"

if [ -z "$STAGED" ]; then
    # If we're running ad-hoc with no staging, compare working
    # tree to HEAD instead (useful for full-repo smoke tests).
    STAGED="$(git diff --name-only HEAD 2>/dev/null || true)"
fi

if [ -z "$STAGED" ]; then
    echo "lint-design-doc-sync.sh: clean (no changes to evaluate)"
    exit 0
fi

# Check whether the staged set includes a design doc. Exclude
# the README + .gitkeep-style files that don't represent
# actionable locks.
DESIGN_DOC_HITS="$(printf '%s\n' "$STAGED" | grep -E '^docs/design/.*\.md$' | grep -v '^docs/design/README\.md$' | grep -v '\.gitkeep$' || true)"

if [ -z "$DESIGN_DOC_HITS" ]; then
    echo "lint-design-doc-sync.sh: clean (no design-doc edits)"
    exit 0
fi

# Design docs touched — was the worklist also touched?
WORKLIST_HITS="$(printf '%s\n' "$STAGED" | grep -E '^docs/PROJECT_WORKLIST\.md$' || true)"

if [ -n "$WORKLIST_HITS" ]; then
    echo "lint-design-doc-sync.sh: clean (design-doc + worklist both touched)"
    exit 0
fi

echo "lint-design-doc-sync.sh: §0.7 gate 15 violation."
echo
echo "Commit touches these design docs:"
printf '%s\n' "$DESIGN_DOC_HITS" | sed 's/^/  /'
echo
echo "but does NOT touch docs/PROJECT_WORKLIST.md."
echo
echo "Per the 25-Q Q18 lock, design-doc edits must lift actionable"
echo "items into the worklist at write-time. Either:"
echo "  - Edit docs/PROJECT_WORKLIST.md to lift the new locks as"
echo "    [ ] Open user-story tasks, OR"
echo "  - If the design-doc edit is genuinely documentation-only"
echo "    (typo fix, prose polish, no new actionable items), use"
echo "    git commit --no-verify with operator approval AND record"
echo "    the override in the commit body."
exit 1
