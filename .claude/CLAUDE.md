# Mackes Shell — Claude Workspace Instructions

**Last updated:** 2026-05-17
**Worklist:** `docs/PROJECT_WORKLIST.md` (canonical)
**Memory:** `~/.claude/projects/-home-mm-Desktop-files-mackes-shell/memory/MEMORY.md`

Adapted from the MAP2 audio platform's `.claude/CLAUDE.md` §0 rulebook,
ported to a single-repo GTK3 Python application with RPM packaging.

---

## 0. Commit & Push Rulebook (APPLIES TO EVERY CHANGE)

These rules govern every commit and push Claude performs in mackes-shell.
They override any conflicting default behavior.

### 0.1 Branch discipline

- **Always stay on `main`.** Never create feature branches unless the user
  explicitly asks.
- **Never force-push to `main`.** If asked, warn and confirm first.
- **No `--no-verify`, `--no-gpg-sign`, `--amend` of pushed commits**, or
  hook skipping — unless the user explicitly requests it. Always prefer a
  new commit over amending.

### 0.2 Single-remote push (mackes-shell-specific)

Unlike MAP2's dual-remote setup, mackes-shell has one remote:
`origin → github.com:matthewmackes/MAP2-RELEASES.git`. Every push is just
`git push origin main`. If a second remote is ever added, update this
section before adopting the dual-push pattern.

### 0.3 Staging hygiene

- Prefer `git add <file>` with explicit paths over `git add -A` / `.`.
- Never commit a file that likely contains secrets. Warn first.
- Never modify `git config`.
- Never touch `mackes/__init__.py:__version__`, `pyproject.toml`,
  `setup.py`, or `packaging/fedora/mackes-shell.spec` versions manually —
  they're bumped via the cut-release flow (see §0.6).

### 0.4 Commit message format

- Follow the existing log style (inspect `git log` before drafting).
- Focus on **why**, not what — the diff already shows what.
- Pass the body via HEREDOC so newlines survive intact:
  ```bash
  git commit -m "$(cat <<'EOF'
  Concise summary

  Optional paragraph explaining motivation and user-visible impact.

  Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
  EOF
  )"
  ```
- "add" = new feature, "update" = enhancement, "fix" = bug fix,
  "refactor" = no behavior change.

### 0.5 Only commit and push when the user asks

- **Never commit unsolicited.** Writing code, running tests, and making
  edits does not license a commit.
- **Never push unsolicited.** Even after the user approves a commit, a
  push is a separate authorization.
- One commit/push approval is not a standing license — ask each time
  unless the user has said "autonomous" / "execute" / "no confirmation
  needed" for the current scope.

### 0.6 `cut release` shorthand

When the user types `cut release X.Y.Z` (or says "cut a release"), treat
it as a single executable command with seven ordered steps — execute all
seven without asking for confirmation between steps unless a step fails:

1. **Bump version** in four files:
   - `mackes/__init__.py:__version__`
   - `pyproject.toml:version`
   - `setup.py:version=`
   - `packaging/fedora/mackes-shell.spec:Version`
2. **CHANGELOG entry** at the top of `CHANGELOG.md` under
   `## X.Y.Z — <one-line summary> (YYYY-MM-DD)`. Describe what shipped.
3. **Smoke test:** `python3 -c "import mackes; print(mackes.__version__)"`.
4. **Local RPM build:** `make rpm` (clean dist + rpmbuild first).
   **Always go through `make rpm`.** Never invoke `rpmbuild` directly
   with `--short-circuit` (or any subset flag like `-bi` / `-bb`
   without a clean source tree). Short-circuit builds stamp the
   `rpmlib(ShortCircuited) <= 4.9.0-1` dep on the output, which
   makes the RPM uninstallable (`dnf install` rejects it with
   `is needed by mde-X.Y.Z`). `make rpm` now verifies this and
   fails loudly — if you see that guard fire, blow away
   `rpmbuild/{BUILD,BUILDROOT,RPMS,SRPMS}` and rerun `make rpm`
   without flags. v2.0.1's `rpmbuild/RPMS/x86_64/mde-2.0.1-…rpm`
   was the first one this caught (2026-05-21).
5. **Commit:** `git commit` with the rulebook's HEREDOC format.
6. **Push + tag:**
   ```bash
   git push origin main && \
       git tag -a vX.Y.Z -m "Mackes Desktop Environment X.Y.Z — …" && \
       git push origin vX.Y.Z
   ```
   (Tag annotation uses the v2.0.0 rebrand-locked name "Mackes
   Desktop Environment", not the legacy "Mackes Shell" / "Mackes
   XFCE Workstation" strings. Operator-verification 2026-05-22
   on the v2.0.3 cut surfaced the legacy string still in
   `release.yml`'s release-name field; both the cut-release
   shorthand and the workflow are now aligned.)
7. **Watch the workflow:** `gh run watch <id> --exit-status`, then
   confirm with `gh release view vX.Y.Z`.

If just `build the RPM for testing` is requested instead, run only steps
1–4 (no commit, no tag, no push).

### 0.7 Pre-commit gates

Before every commit, when applicable:

1. **Module import smoke:** `python3 -c "<every module touched>"` —
   must run without error.
2. **Tests:** `make test-nodeps` (if `tests/` or `mackes/` touched).
3. **Ruff lint:** `make lint` (if `mackes/` or `tests/` touched).
   Mirrors the exact `ruff check --select F401,F541,F811,F841 mackes/
   tests/` ci runs, so a local pass means ci will pass too.
4. **RPM build:** `make rpm` (if `packaging/`, `setup.py`,
   `pyproject.toml`, `data/`, or `mackes/birthright.py` touched).
5. **CSS lint:** `install-helpers/lint-css.sh` (if `data/css/` touched).

If a pre-commit hook fails, the commit did **not** happen — fix the
issue, re-stage, and create a **new** commit. Never `--amend` in that
scenario.

### 0.8 Definition of Done

A worklist task is not `[✓] Done` until **every** gate passes:

1. **Code committed to `main`** — in git history, not only in working tree.
2. **Pushed to origin** — `git push origin main` completed without error.
3. **RPM builds** — `make rpm` exited 0 (for spec / data / module
   changes).
4. **Tagged + released** — for shipping versions, `vX.Y.Z` tag pushed and
   the release workflow lands on GitHub.
5. **All module imports clean** — `python3 -c "import mackes.<module>"`
   passes for everything touched.
6. **CHANGELOG updated** — user-visible change documented.

Writing code alone never satisfies Done. If gates 1–6 aren't confirmed,
the task stays `[>] In Progress` with a note on which gate is incomplete.

### 0.9 Destructive operations require explicit authorization

Before running any of these, pause and confirm:

- `git push --force` / `--force-with-lease` to `main`
- `git reset --hard`, `git checkout -- .`, `git restore .`, `git clean -f`
- `git branch -D`
- Amending an already-pushed commit
- `rm -rf` on anything outside `dist/` or `rpmbuild/`
- Any action that modifies GitHub state (closing PRs, deleting remote
  branches)

### 0.10 When a commit/push fails

- **Pre-commit hook failed** → fix the issue; re-stage; make a **new**
  commit. Don't `--amend`.
- **Remote rejected push (non-fast-forward)** →
  `git fetch origin main`; merge or rebase; resolve conflicts; re-run
  the push.
- **`make rpm` failed** → diagnose, fix, re-run; don't push the
  half-built tree.

### 0.11 Visual / design work uses a PR-based branch lane (WF-1, 2026-05-21)

§0.1's "always stay on `main`" default is wrong for visual / design
work, where a human-eye review of before/after screenshots is the
load-bearing gate. UX-* tasks (and any work whose primary diff is
visual rather than logical) follow this branch protocol:

- **Branch name:** `ux/<task-id>` (e.g., `ux/UX-14-command-palette`,
  `ux/UX-5-sidebar`). Short-lived; deleted after merge.
- **PR description requirements:**
  1. Before/after screenshots **in dark + light + all density modes
     touched**, embedded inline.
  2. Cite the design-lock IDs (Q-*, FU-*, NFU-*, UX-*) the change
     implements.
  3. Snapshot-diff output from UX-23 (once that gate lands), or
     manual `make snapshots-local` artifact during the HW-3 deferral
     window.
- **Merge gate:** explicit user OK on the PR. CI-green is necessary
  but not sufficient — a code-passing-tests change can still look
  wrong.
- **Code-only tasks (no visible UI diff)** retain the main-only
  default from §0.1. The branch lane is for visual diffs only.

This rule supersedes §0.1 for UX-* and visual work; §0.1 continues
to apply everywhere else.

---

## 1. Worklist Rule

`.claude/skills/mackes-worklist-management/SKILL.md` is the canonical
project worklist protocol. Apply on every substantive change unless the
user explicitly says `DISABLE WORKLIST RULE`.

**Single worklist — the only one.** `docs/PROJECT_WORKLIST.md` is the
single durable worklist for the project. There is no parallel
tracker:

- The Claude Code in-session `TaskList` / `TaskCreate` / `TaskUpdate`
  tools are an **ephemeral scratchpad** for the active conversation
  only. Anything that needs to survive the session gets written to
  `docs/PROJECT_WORKLIST.md` directly. Do not let the in-session
  task list and the file diverge — the file wins on every conflict.
- No side trackers in CHANGELOG drafts, memory notes, or comments.
- **Design docs (`docs/design/*.md`) are not a parallel worklist.**
  Every actionable item from a locked design doc gets lifted into
  `docs/PROJECT_WORKLIST.md` as `[ ] Open`. The design doc keeps the
  rationale + locks; the worklist keeps the actionable list.

**No silent deferrals (user directive 2026-05-19).** Items are
either `[ ] Open`, `[>] In Progress`, `[✓] Done`, or `[!] Blocked`.
The `[~] Deferred` status is retired.

**When directives contradict, the newer one wins silently
(user directive 2026-05-19).** Don't maintain a separate
"Conflicts" section, don't ask the user to adjudicate older locks
— just update the affected worklist items in place and reflect
the latest decision. The original design doc can keep its old
text as historical context; the worklist tracks live policy only.

The status legend, task schema, and disable phrase are documented in
the skill file.

### 1.1 Release-tag schema on every worklist task (WF-5, 2026-05-21)

Every task title in `docs/PROJECT_WORKLIST.md` must declare its
target release via an explicit prefix. Two acceptable forms:

- **Version prefix:** `v2.0.1: <short title>` or
  `v2.1: <short title>` — fastest for one-off bug-fix / hotfix
  tasks pinned to a specific release.
- **Workstream prefix:** `UX-14: <short title>`,
  `CB-1.5.a: <short title>`, `XOrg-1.2: <short title>`,
  `HW-3: <short title>`, `WF-2: <short title>` — for tasks that
  belong to a named workstream whose target release is declared in
  the section header (e.g., the **UX-10..UX-23: Round 2 (v2.2
  scope)** header sets v2.2 as the target for every UX-N task
  inside).

**Active section filtering:** `target_release >= current_release`.
**History section filtering:** `target_release < current_release`.
Tasks without a recognizable prefix are a worklist-hygiene defect;
the WF-5.a pre-commit hook (when it lands) will block commits that
introduce an untagged task. Until WF-5.a ships, this is enforced by
manual review on every worklist edit.

Rationale: prefixless tasks accumulate as "is this in the next
release or three releases away?" ambiguity — the prefix kills that
ambiguity at write-time, not at scheduling-time.

## 2. Autonomy Rule (`complete-remaining-work`)

`.claude/skills/complete-remaining-work/SKILL.md` defines the autonomy
policy. When the user says "execute" / "continue" / "complete remaining
work" / "ship it", default behavior is:

- Highest-priority unfinished tasks first.
- Independent bundles in parallel.
- Mark `[>] In Progress` before substantive edits.
- Implement fully — no stubs, no placeholders.
- Add follow-up tasks for any tech debt or deferrals introduced.
- Escalate only when blocked by missing facts, required approvals, or
  contradictory requirements.

## 3. Code-style locks (project-specific)

Mirror the existing patterns rather than introducing new ones:

- **Module imports:** `from __future__ import annotations` at top.
  Standard library before third-party; third-party before mackes.*.
- **GTK panels:** must follow the Carbon refresh pattern — breadcrumb +
  `_page_title` + `_page_subtitle` + `_section_title` helpers. See
  `mackes/workbench/network/mesh_ssh.py` for the canonical layout.
- **Privileged ops:** route through `mackes.admin_session.AdminSession`,
  not raw `pkexec`. The session-unlock pattern (v1.4.0) holds creds for
  the whole session.
- **Memory writes:** when the user shares preferences, lock-in
  decisions, or project state worth carrying across conversations,
  follow the auto-memory protocol (every memory in
  `~/.claude/projects/-home-mm-Desktop-files-mackes-shell/memory/`).

## 4. Index — frequently-visited files

| Topic | Path |
|---|---|
| Carbon design tokens | `data/css/tokens.css` |
| Layout classes | `data/css/carbon-layout.css` |
| Shell window | `mackes/workbench/shell/sidebar_window.py` |
| Sidebar nav model | `mackes/workbench/shell/sidebar_window.py:_build_nav` |
| Wizard pipeline | `mackes/wizard/pages/apply.py` |
| Birthright steps | `mackes/birthright.py` |
| Admin session | `mackes/admin_session.py` |
| Mesh fabric core | `mackes/mesh_vpn.py`, `mesh_ssh.py`, `mesh_services.py` |
| RPM spec | `packaging/fedora/mackes-shell.spec` |
| Release workflow | `.github/workflows/release.yml` |
| Help docs | `docs/help/*.md` |
| Design source (v1.1.0 refresh) | `docs/design/v1.1.0-carbon-refresh/` |
