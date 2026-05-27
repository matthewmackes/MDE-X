#!/bin/sh
# install-helpers/lint-runtime-reachability.sh — TUNE-3 / 25-Q Q10
# pre-commit gate #14.
#
# Automates §0.8 Definition-of-Done gate 7 + §0.12 runtime-
# reachability check: every `pub mod foo;` declaration in
# `crates/*/src/lib.rs` + `crates/*/src/*/mod.rs` must have at
# least one external `foo::` reference somewhere in the workspace,
# OR the declaring file itself uses `pub use foo::*` to re-export
# (which counts as "wired" for the binary's public API).
#
# This is the upstream-prevention for the v3.x dead-panel-module
# audit (2026-05-22 caught 13 dead modules in mde-panel). Per
# §0.12 + 25-Q Q10 the lint is now mechanical, not manual.
#
# Per CLAUDE.md §0.7 gate #14.
#
# Exits 0 = clean, exits 1 = dead modules found.

set -eu

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# Collect every `pub mod foo;` declaration in lib.rs + mod.rs +
# main.rs files. We skip `pub mod foo { ... }` inline-module
# declarations because those are by-definition reachable from
# inside the same file.
#
# main.rs is included as of 2026-05-27 (TUNE-3.d extension): the
# Portal-14.d breath_line + Portal-14.c marquee primitives were
# both declared via `pub mod` in main.rs; lib.rs/mod.rs-only
# scanning would have missed any dead module under a binary
# crate. Catching those at lint-time closes the §0.12 no-stubs
# upstream gap.
DECL_FILES="$(find crates -type f \( -name 'lib.rs' -o -name 'mod.rs' -o -name 'main.rs' \) 2>/dev/null)"

# Snapshot allow-list file. Pre-existing dead modules captured
# on 2026-05-26 at lint introduction (48 entries — the v3.x dead-
# panel audit's residue extended across mde-peer-card enrich/,
# mde-kdc-proto plugins/, mackesd settings/, mackesd workers/,
# and several scattered modules). Each entry is tracked for
# cleanup in TUNE-3.b (deferred follow-on cleanup epic — see
# worklist). The file shrinks as modules are wired or deleted.
#
# Format: one `<file>:<module-name>` per line. Lines starting
# with `#` are comments. Blank lines ignored.
ALLOWLIST_FILE="${REPO_ROOT}/install-helpers/lint-runtime-reachability.allowlist"

VIOLATIONS=""

for decl_file in $DECL_FILES; do
    # Parse each `pub mod <name>;` line, capturing the module
    # name. Skip lines with inline bodies (`pub mod foo {`),
    # cfg-gated test-only modules, and lines inside comments.
    while IFS= read -r line; do
        # Extract `pub mod <name>;` — name is one identifier.
        # POSIX sed; `\<` not portable, use explicit boundary.
        name="$(printf '%s' "$line" | sed -n 's/^[[:space:]]*pub mod \([a-z_][a-z0-9_]*\)[[:space:]]*;[[:space:]]*$/\1/p')"
        if [ -z "$name" ]; then
            continue
        fi

        # Determine the module's own file paths to exclude from
        # the "external reference" search.
        decl_dir="$(dirname "$decl_file")"
        own_file="${decl_dir}/${name}.rs"
        own_mod_file="${decl_dir}/${name}/mod.rs"

        # Search for `<name>::` references in the workspace's
        # Rust source, EXCLUDING the module's own files. The
        # decl_file (parent mod.rs / lib.rs) IS allowed to be the
        # consumer — that's the canonical Rust parent-uses-child
        # pattern (`mod foo;` + `foo::bar()` in the same file).
        # Doc comments (lines starting with `//!` or `///`) are
        # excluded since they're documentation, not runtime
        # references.
        hits="$(grep -rn --include='*.rs' "\\b${name}::" crates/ 2>/dev/null | \
                grep -v "^${own_file}:" | \
                grep -v "^${own_mod_file}:" | \
                grep -v "^[^:]*:[0-9]*:[[:space:]]*//[!/]" | \
                head -5)"

        # Also accept `pub use <name>` lines as reachability —
        # those re-export the module's content as the crate's
        # public API. The decl_file itself often contains the
        # `pub use foo::{Bar}` re-export; we DO count that as
        # reachability (the crate exports it for downstream
        # consumers).
        pub_use_hits="$(grep -rn --include='*.rs' "pub use ${name}\\b" crates/ 2>/dev/null | head -3)"

        # Also accept aliased imports: `<name> as <alias>` inside
        # a `use crate::foo::{...}` block re-binds the module
        # under a new identifier — consumers then write
        # `<alias>::Type`, not `<name>::Type`, so the original
        # name appears only at the alias site. The mde-workbench
        # panels use this pattern (`use crate::panels::{
        # apps_install as apps_install_panel, ... };`).
        # Restrict to `\b<name> as ` so we don't false-positive on
        # `apps_install_complete as foo` matching `apps_install`.
        as_alias_hits="$(grep -rn --include='*.rs' "\\b${name} as " crates/ 2>/dev/null | \
                grep -v "^${own_file}:" | \
                grep -v "^${own_mod_file}:" | \
                head -3)"

        if [ -z "$hits" ] && [ -z "$pub_use_hits" ] && [ -z "$as_alias_hits" ]; then
            # Check allow-list file before recording violation.
            allowlist_key="${decl_file}:${name}"
            if [ -f "$ALLOWLIST_FILE" ] && grep -v '^[[:space:]]*#' "$ALLOWLIST_FILE" | grep -v '^[[:space:]]*$' | grep -Fxq "${allowlist_key}"; then
                continue
            fi
            VIOLATIONS="${VIOLATIONS}
${decl_file}: pub mod ${name}; (no external <crate>::${name}:: or pub use ${name} references)"
        fi
    done < "$decl_file"
done

if [ -n "$VIOLATIONS" ]; then
    echo "lint-runtime-reachability.sh: §0.8 gate 7 violations — dead modules:"
    printf '%s\n' "$VIOLATIONS"
    echo
    echo "Each line above declares a module that nothing references from"
    echo "outside its own file. Per CLAUDE.md §0.8 gate 7 + §0.12, every"
    echo "pub mod must have a runtime entry point. Either wire the module"
    echo "into a caller, re-export via pub use, or remove the declaration."
    exit 1
fi

echo "lint-runtime-reachability.sh: clean (all pub mod declarations have external references)"
exit 0
