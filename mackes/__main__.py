"""Entry point. `python -m mackes` or the installed `mackes` binary lands here.

Delegates to `mackes.app.main`, which handles:
  - auto-detect headless (no $DISPLAY) and dispatch to the CLI subcommand
    router (mackes.headless.cli), OR launch the GTK Application
  - `--gui` / `--headless` flag overrides
  - every CLI subcommand (status, peers, ssh, snapshot, services, etc.)

Recognized fast-path flags handled here (before delegating):
  --uninstall  Run the headless uninstall sequence (Q9 lock).
  --yes        With --uninstall, skip the interactive confirmation.
"""
from __future__ import annotations

import sys


def _run_cli_uninstall(yes: bool) -> int:
    from mackes.uninstall import run_uninstall
    if not yes:
        try:
            print("This will remove Mackes Shell and all its files, reset xfconf to")
            print("XFCE defaults, reinstall replaced XFCE components, and clean up")
            print("xfce11-unified v2.2 leftovers. A final snapshot will be tarballed")
            print("to ~/Desktop/. Type 'UNINSTALL' to proceed:")
            line = input("> ").strip()
        except EOFError:
            line = ""
        if line != "UNINSTALL":
            print("Aborted.")
            return 130

    def _emit(line: str) -> None:
        print(line, flush=True)

    report = run_uninstall(progress=_emit)
    print()
    print(f"Failed steps: {report.failed_count} of {len(report.steps)}")
    if report.log_path is not None:
        print(f"Full log: {report.log_path}")
    if report.desktop_tarball is not None:
        print(f"Final snapshot: {report.desktop_tarball}")
    return 0 if report.failed_count == 0 else 1


def main(argv: list[str] | None = None) -> int:
    raw = list(argv if argv is not None else sys.argv)

    # Q9 lock — `mackes --uninstall` runs headless without launching the GUI
    # or the subcommand router.
    flags = {a for a in raw[1:] if a.startswith("--")}
    if "--uninstall" in flags:
        return _run_cli_uninstall(yes="--yes" in flags)

    # v1.4.0: TUI as the default headless entry point.
    #
    # Launch the Textual TUI when:
    #   - no $DISPLAY / $WAYLAND_DISPLAY is set, AND
    #   - no subcommand or non-flag argument was given, AND
    #   - --tui is forced OR --gui isn't forced, AND
    #   - textual is importable.
    #
    # If textual is missing, we silently fall through to the argparse CLI
    # in mackes.headless.cli (the legacy v1.3.0 behavior).
    import os
    args = raw[1:]
    no_display = not (os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY"))
    no_positional = not any(a for a in args if not a.startswith("-"))
    force_tui = "--tui" in flags
    force_cli = "--cli" in flags
    force_gui = "--gui" in flags
    force_headless = "--headless" in flags

    if force_tui or (no_display and no_positional and not force_cli
                     and not force_gui and not force_headless):
        try:
            from mackes.tui import available, run as run_tui
        except Exception:  # noqa: BLE001
            available = lambda: False  # type: ignore[assignment]
        if available():
            # Strip the --tui flag so it doesn't confuse downstream code.
            args = [a for a in args if a != "--tui"]
            return run_tui()

    # Everything else: delegate to mackes.app.main, which routes between
    # the headless CLI (auto-detected or via `--headless`) and the GTK GUI.
    from mackes.app import main as app_main
    return app_main(args)


if __name__ == "__main__":
    raise SystemExit(main())
