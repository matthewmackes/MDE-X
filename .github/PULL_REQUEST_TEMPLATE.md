## Summary

What does this PR do? Focus on **why**, not what — the diff shows what.

## Type of change

- [ ] Bug fix (non-breaking)
- [ ] New feature (non-breaking)
- [ ] Breaking change (would cause existing setups to misbehave)
- [ ] Documentation update only
- [ ] Refactor (no user-visible change)
- [ ] Test / CI update only

## Definition of Done checklist

(See `.claude/CLAUDE.md §0.8` for the full rulebook.)

- [ ] Code committed to a branch off `main`
- [ ] All touched modules import cleanly:
      `python3 -c "import mackes.<module>"` passes
- [ ] `make rpm` builds (for spec / data / module changes)
- [ ] CHANGELOG entry added (for user-visible changes)
- [ ] Carbon refresh pattern followed for any new panel
- [ ] Privileged ops routed through `mackes.admin_session.AdminSession`
- [ ] Spacing follows 8px grid; CSS lint passes (if `data/css/` touched)

## Test plan

How did you verify this works?

- [ ] Smoke-tested the GUI on a real XFCE session
- [ ] TUI tested with `python3 -m mackes --tui`
- [ ] `make rpm` builds the RPM cleanly
- [ ] RPM installs without conflicts on Fedora 44

## Screenshots

For UI changes, attach before/after screenshots (or terminal recordings
for TUI changes).

## Notes for reviewer

Anything else? Surprises, deferrals, follow-up tasks?
