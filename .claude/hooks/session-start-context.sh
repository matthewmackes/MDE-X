#!/usr/bin/env bash
# EPIC-PROC-HARNESS-INJECT (Q90) — SessionStart context injector.
#
# Summarizes the governance compass (docs/AI_GOVERNANCE.md §0 master
# rule + §11 1.0-roadmap pointer), the auto-memory index (MEMORY.md),
# and the last 3 commits, then injects them into a fresh Claude
# session via the SessionStart hook's `additionalContext` field. The
# goal (Q90): a new session is a competent design partner without the
# operator manually pasting in 5 files.
#
# Wired into .claude/settings.json under hooks.SessionStart with the
# startup/clear/resume matchers (NOT compact — a compacted session
# already carries its context forward, so re-injecting there is noise).
#
# Degrades gracefully: every section is optional. A missing file or a
# non-git checkout just drops that section rather than failing the
# hook (a non-zero exit from a SessionStart hook would surface an
# error to the operator on every launch).

set -uo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." 2>/dev/null && pwd)"
[ -n "$repo_root" ] && cd "$repo_root" || exit 0

gov="docs/AI_GOVERNANCE.md"
worklist="docs/PROJECT_WORKLIST.md"
rulebook=".claude/CLAUDE.md"
mem="${HOME}/.claude/projects/-home-mm-Desktop-files-mackes-shell/memory/MEMORY.md"

block=""
add() { block+="$1"$'\n'; }

add "# Project orientation (auto-injected by session-start-context.sh)"
add ""

# --- Governance compass (AI_GOVERNANCE.md) ---------------------------
if [ -f "$gov" ]; then
    master="$(grep -m1 -E '"\*\*"?Secure, Simple, Centerless' "$gov" 2>/dev/null \
        || grep -m1 'Secure, Simple, Centerless' "$gov" 2>/dev/null)"
    roadmap_n="$(grep -cE '^\| [0-9]+ \| ' "$gov" 2>/dev/null || echo '?')"
    add "## Governance — docs/AI_GOVERNANCE.md"
    [ -n "$master" ] && add "- Master rule (§0): ${master#> }"
    add "- §11 is the 1.0 cut definition: ${roadmap_n} roadmap items, ALL must be green (NO INCOMPLETE RELEASES, §0.17)."
    add "- Rulebook: ${rulebook} (§0 commit/push/gate rules). Worklist: ${worklist} (canonical task state)."
    add "- Authority order on conflict (newest wins): Memory > CLAUDE.md > AI_GOVERNANCE > design docs > worklist."
    add ""
fi

# --- Auto-memory index (MEMORY.md) ----------------------------------
if [ -f "$mem" ]; then
    add "## Memory index — ${mem}"
    # MEMORY.md is itself a one-line-per-memory index; inject it
    # verbatim (skip blank lines to stay compact).
    while IFS= read -r line; do
        [ -n "$line" ] && add "$line"
    done < "$mem"
    add ""
fi

# --- Recent commits -------------------------------------------------
if git rev-parse --git-dir >/dev/null 2>&1; then
    log="$(git log -3 --oneline 2>/dev/null)"
    if [ -n "$log" ]; then
        add "## Last 3 commits"
        while IFS= read -r line; do add "- $line"; done <<< "$log"
        add ""
    fi
fi

# --- Emit as SessionStart additionalContext -------------------------
# python3 is a hard dependency of this project, so it is always
# present; it gives us robust JSON string encoding without a jq dep.
printf '%s' "$block" | python3 -c '
import json, sys
ctx = sys.stdin.read()
print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "SessionStart",
        "additionalContext": ctx,
    }
}))
'
