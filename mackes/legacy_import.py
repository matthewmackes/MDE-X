"""Legacy 2.x → 1.x state import (Phase 10.2; v3.0.0 Q49).

`xfce11-unified v2.2` (and the early 2.x mackes prototypes that shared
its layout) wrote user state under `~/.config/mackes-shell/`. The
v3.0.0 rewrite — shipping as `mackes-shell 1.x` per Q48 — moved every
panel-relevant setting into `~/.config/mackes-panel/panel.toml` and
keeps only "is the machine provisioned + which preset is active" in
`~/.config/mackes-shell/state.json`.

Q49 lock: on first launch after the upgrade, the wizard scans the old
layout and folds it forward — preset, wallpaper, pinned apps. This
module implements that scan + translation. The wizard's
``LegacyImportPage`` (``mackes/wizard/pages/legacy_import.py``)
displays the result and triggers the import on user click.

Legacy schema we read (best-effort, every field optional):

  ``~/.config/mackes-shell/state.json``
      Top-level JSON document.

      Accepted preset-name fields (in priority order)::

          preset_name      — 2.x canonical key
          preset           — early-2.x alias
          active_preset    — 1.x current schema; tolerated so a re-run
                              on a 1.x state.json is a no-op

      Accepted wallpaper-path fields (in priority order)::

          wallpaper
          wallpaper_path
          desktop_wallpaper

  ``~/.config/mackes-shell/pinned/``
      Directory of pinned `.desktop` files. Each entry may be the
      `.desktop` itself, a symlink to one under `/usr/share/applications`,
      or a plain text file whose name we treat as the desktop id.

  ``~/.config/mackes-shell/recents.json``
      JSON list (or `{"recents": [...]}` envelope) of recently-used
      `.desktop` ids. 1.x has no recents surface so these are recorded
      and dropped — surfaced in the migration log for transparency.

  ``~/.config/mackes-shell/drawer-overrides.json``
      Free-form JSON dict of drawer customizations. We map the
      narrow set that has a 1.x equivalent::

          show_appmenu          → top_bar.appmenu
          status_items          → top_bar.status_items
          mesh_replicate        → mesh.replicate
          mesh_drift_seconds    → mesh.drift_check_seconds

The translation target lives at ``~/.config/mackes-panel/panel.toml``
and follows the schema in ``crates/mackes-config/src/lib.rs`` (the
authoritative serde model). We emit TOML by hand (a small,
schema-specific writer in this module) so we never grow a runtime
TOML dependency just for migration.

Idempotency: ``import_to_panel_toml`` is safe to re-run with the same
``LegacyState`` — pinned apps deduplicate by ``.desktop`` id,
``status_items`` is replaced wholesale (not appended to), and the
``top_bar.appmenu`` / ``mesh.*`` scalars overwrite. Calling twice
produces the same panel.toml byte-for-byte (modulo the file's prior
state being preserved on the second call).
"""
from __future__ import annotations

import json
import logging
import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, List, Optional

logger = logging.getLogger(__name__)


# Legacy and current paths. We re-resolve $HOME / $XDG_* on every call
# so the test suite's `isolated_xdg` fixture can re-root them.
def _config_home() -> Path:
    return Path(os.environ.get("XDG_CONFIG_HOME") or
                (Path(os.environ.get("HOME", str(Path.home()))) / ".config"))


def _legacy_dir() -> Path:
    return _config_home() / "mackes-shell"


def _panel_toml_path() -> Path:
    return _config_home() / "mackes-panel" / "panel.toml"


# Known 1.x state field names (current MackesState schema). When the
# legacy file already uses these we still ingest it — the import becomes
# a no-op for preset (it's already where 1.x expects it) but pinned apps
# / drawer overrides may still need folding into panel.toml.
_CURRENT_STATE_FIELDS: frozenset[str] = frozenset(
    {"provisioned", "active_preset", "last_apply", "schema_version", "notes"}
)


@dataclass
class LegacyState:
    """Parsed view of every legacy field we know how to migrate.

    Every field is optional — a fresh install yields an instance with
    all defaults (and ``detect()`` returns ``None`` in that case). The
    presence of *any* of the listed paths produces a non-None result
    so the wizard can show the user what's being preserved.
    """

    preset_name: Optional[str] = None
    """Legacy v1.x/2.x preset name. Pre-rebrand presets: hashbang /
    mackes / daylight / vanilla / node. Retired 2026-05-26 per Q79
    of the 100-Q tightening survey; replaced by the 4-preset locked
    set: chromeos-classic-{light,dark} + ableton-12-{light,dark}.
    Legacy values found by `legacy_import` are preserved here for
    the wizard's preservation report but get mapped to a current
    preset at apply time (defaulting to chromeos-classic-dark per
    AI_GOVERNANCE.md §6)."""

    wallpaper_path: Optional[str] = None
    """Absolute path to the user's last wallpaper."""

    pinned_apps: List[str] = field(default_factory=list)
    """Ordered list of `.desktop` ids (basename, e.g. `firefox.desktop`)."""

    recents: List[str] = field(default_factory=list)
    """Ordered list of recently-used `.desktop` ids — dropped on import
    (no 1.x recents surface), surfaced in the migration log."""

    drawer_overrides: dict[str, Any] = field(default_factory=dict)
    """Free-form drawer customizations from the 2.x drawer; only known
    keys are folded into panel.toml. The rest are surfaced in the log
    so the user knows they were dropped."""

    source_dir: Optional[Path] = None
    """Resolved legacy-dir path — kept for debugging + log lines."""

    def is_empty(self) -> bool:
        """True when every field is at its default (no migration needed)."""
        return (
            self.preset_name is None
            and self.wallpaper_path is None
            and not self.pinned_apps
            and not self.recents
            and not self.drawer_overrides
        )


# ---------------------------------------------------------------------------
# detect()
# ---------------------------------------------------------------------------


def detect() -> Optional[LegacyState]:
    """Scan ``~/.config/mackes-shell/`` for migratable 2.x leftovers.

    Returns ``None`` when nothing is found (fresh install). Returns a
    ``LegacyState`` with whichever fields the scan resolved. Every
    sub-probe is wrapped in a try/except so a single bad file doesn't
    sink the whole scan — the wizard's contract is *graceful
    degradation*, not strictness.
    """
    legacy = _legacy_dir()
    if not legacy.is_dir():
        return None

    state = LegacyState(source_dir=legacy)

    # ---- state.json ------------------------------------------------------
    state_path = legacy / "state.json"
    if state_path.is_file():
        try:
            doc = json.loads(state_path.read_text(encoding="utf-8"))
            if not isinstance(doc, dict):
                logger.warning(
                    "legacy_import: state.json is not a JSON object — ignoring"
                )
                doc = {}
        except (OSError, json.JSONDecodeError) as exc:
            logger.warning(
                "legacy_import: state.json unreadable (%s) — degrading gracefully",
                exc,
            )
            doc = {}

        # Preset name — try every accepted key in priority order.
        for key in ("preset_name", "preset", "active_preset"):
            value = doc.get(key)
            if isinstance(value, str) and value.strip():
                state.preset_name = value.strip()
                break

        # Wallpaper path — try every accepted key in priority order.
        for key in ("wallpaper", "wallpaper_path", "desktop_wallpaper"):
            value = doc.get(key)
            if isinstance(value, str) and value.strip():
                state.wallpaper_path = value.strip()
                break

    # ---- pinned/ subdir --------------------------------------------------
    pinned_dir = legacy / "pinned"
    if pinned_dir.is_dir():
        try:
            entries = sorted(pinned_dir.iterdir(), key=lambda p: p.name)
        except OSError as exc:
            logger.warning(
                "legacy_import: pinned/ unreadable (%s) — skipping pins", exc
            )
            entries = []

        seen: set[str] = set()
        for entry in entries:
            desktop_id = _resolve_pinned_entry(entry)
            if desktop_id and desktop_id not in seen:
                seen.add(desktop_id)
                state.pinned_apps.append(desktop_id)

    # ---- recents.json ----------------------------------------------------
    recents_path = legacy / "recents.json"
    if recents_path.is_file():
        try:
            raw = json.loads(recents_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            logger.warning(
                "legacy_import: recents.json unreadable (%s) — skipping", exc
            )
            raw = None

        items: List[Any]
        if isinstance(raw, list):
            items = raw
        elif isinstance(raw, dict) and isinstance(raw.get("recents"), list):
            items = raw["recents"]
        else:
            items = []

        for item in items:
            if isinstance(item, str) and item.strip():
                state.recents.append(_normalize_desktop_id(item.strip()))

    # ---- drawer-overrides.json ------------------------------------------
    drawer_path = legacy / "drawer-overrides.json"
    if drawer_path.is_file():
        try:
            raw = json.loads(drawer_path.read_text(encoding="utf-8"))
            if isinstance(raw, dict):
                state.drawer_overrides = raw
            else:
                logger.warning(
                    "legacy_import: drawer-overrides.json is not a JSON object"
                )
        except (OSError, json.JSONDecodeError) as exc:
            logger.warning(
                "legacy_import: drawer-overrides.json unreadable (%s)", exc
            )

    if state.is_empty():
        return None
    return state


def _resolve_pinned_entry(entry: Path) -> Optional[str]:
    """Translate one entry in ``pinned/`` to a normalized `.desktop` id.

    The 2.x layout dropped any of:

      * a `.desktop` file (we use the basename)
      * a symlink to a system `.desktop` (basename of the target)
      * a plain-text file whose first line is `Desktop=<id>` or whose
        name (sans extension) is the desktop id

    Returns None for entries we can't make sense of so the caller
    silently drops them.
    """
    try:
        name = entry.name
        if name.startswith("."):
            return None

        # Symlink-to-system case: resolve and take the target basename.
        if entry.is_symlink():
            try:
                target = entry.resolve(strict=False)
                if target.name.endswith(".desktop"):
                    return target.name
            except (OSError, RuntimeError):
                pass

        # Regular .desktop file — use the basename as-is.
        if name.endswith(".desktop"):
            return name

        # Plain text "pointer" file: look for `Desktop=<id>` lines.
        if entry.is_file():
            try:
                content = entry.read_text(encoding="utf-8", errors="ignore")
                for line in content.splitlines():
                    line = line.strip()
                    if line.startswith("Desktop=") and line[len("Desktop=") :].strip():
                        return _normalize_desktop_id(line[len("Desktop=") :].strip())
            except OSError:
                pass

        # Last resort: treat the filename (sans extension) as a bare id.
        if "." not in name and name:
            return _normalize_desktop_id(name)
    except OSError:
        return None
    return None


def _normalize_desktop_id(raw: str) -> str:
    """Append `.desktop` if missing — schema requires the basename."""
    if raw.endswith(".desktop"):
        return raw
    return f"{raw}.desktop"


# ---------------------------------------------------------------------------
# import_to_panel_toml()
# ---------------------------------------------------------------------------


# Status-items field validation — must be a list of strings, otherwise
# we silently drop it (we don't crash the wizard on bad legacy data).
_KNOWN_STATUS_ITEMS = frozenset(
    {"mesh", "clipboard", "volume", "battery", "notifications", "user", "appmenu"}
)


def import_to_panel_toml(legacy: LegacyState) -> List[str]:
    """Translate ``legacy`` into ``~/.config/mackes-panel/panel.toml``.

    Returns a list of human-readable migration lines suitable for the
    wizard's progress log. The list is non-empty even when nothing
    changed (we record "no-op" entries so the user understands).

    Side effects, in order:

      1. Reads the current ``panel.toml`` (or starts from the schema
         default if missing / unparseable).
      2. Folds in known legacy fields (idempotent).
      3. Writes the result to ``panel.toml`` atomically (write to
         ``panel.toml.new`` + rename).
      4. Updates ``~/.config/mackes-shell/state.json`` to record the
         preset name as ``active_preset`` so a re-run of the wizard
         skips the import branch.

    Does *not* touch xfconf, the wallpaper file, or any system service —
    the apply pipeline (later in the wizard) is the only writer of
    those surfaces. The wallpaper path is recorded in ``[migration]``
    inside ``panel.toml`` so the user / later code can act on it.
    """
    log: List[str] = []

    # ---- load / construct base config -----------------------------------
    cfg = _read_panel_toml_or_default()

    # ---- pinned_apps → dock.items ---------------------------------------
    existing_apps = {
        item["desktop"]
        for item in cfg["dock"]["items"]
        if item.get("kind") == "app" and isinstance(item.get("desktop"), str)
    }
    new_pins = 0
    for desktop in legacy.pinned_apps:
        if desktop in existing_apps:
            log.append(f"  · pinned app already present: {desktop}")
            continue
        cfg["dock"]["items"].append({"kind": "app", "desktop": desktop})
        existing_apps.add(desktop)
        new_pins += 1
        log.append(f"  · imported pinned app: {desktop}")
    if legacy.pinned_apps:
        log.append(
            f"imported {new_pins} pinned app(s) "
            f"({len(legacy.pinned_apps) - new_pins} already present)"
        )

    # ---- drawer_overrides → top_bar / mesh ------------------------------
    if legacy.drawer_overrides:
        applied, dropped = _apply_drawer_overrides(cfg, legacy.drawer_overrides)
        for line in applied:
            log.append(f"  · {line}")
        if dropped:
            log.append(
                f"dropped {len(dropped)} drawer-override key(s) without a "
                f"1.x equivalent: {', '.join(sorted(dropped))}"
            )

    # ---- preset_name → migration sidecar AND state.json ----------------
    if legacy.preset_name:
        cfg.setdefault("migration", {})["legacy_preset"] = legacy.preset_name
        log.append(f"recorded legacy preset: {legacy.preset_name}")
        _write_active_preset(legacy.preset_name, log)

    # ---- wallpaper_path → migration sidecar ----------------------------
    if legacy.wallpaper_path:
        cfg.setdefault("migration", {})["legacy_wallpaper"] = (
            legacy.wallpaper_path
        )
        wp = Path(legacy.wallpaper_path).expanduser()
        if wp.is_file():
            log.append(f"recorded legacy wallpaper: {legacy.wallpaper_path}")
        else:
            log.append(
                f"recorded legacy wallpaper path (file missing on disk): "
                f"{legacy.wallpaper_path}"
            )

    # ---- recents → dropped (logged only) -------------------------------
    if legacy.recents:
        log.append(
            f"dropped {len(legacy.recents)} recents entry "
            f"(no 1.x recents surface)"
        )

    # ---- write panel.toml + done banner --------------------------------
    _write_panel_toml(cfg)
    log.append(f"wrote {_panel_toml_path()}")
    if not any(line.startswith("imported ") or line.startswith("recorded ")
               for line in log):
        log.append("no migratable fields — panel.toml left at defaults")
    return log


def _apply_drawer_overrides(
    cfg: dict, overrides: dict[str, Any]
) -> tuple[List[str], List[str]]:
    """Fold known keys into ``cfg``. Returns (applied_lines, dropped_keys)."""
    applied: List[str] = []
    dropped: List[str] = []

    for key, value in overrides.items():
        if key == "show_appmenu" and isinstance(value, bool):
            cfg["top_bar"]["appmenu"] = value
            applied.append(f"top_bar.appmenu = {str(value).lower()}")
        elif key == "status_items" and isinstance(value, list):
            cleaned = [
                v for v in value
                if isinstance(v, str) and v in _KNOWN_STATUS_ITEMS
            ]
            if cleaned:
                cfg["top_bar"]["status_items"] = cleaned
                applied.append(
                    f"top_bar.status_items = [{', '.join(cleaned)}]"
                )
            else:
                dropped.append(key)
        elif key == "mesh_replicate" and isinstance(value, bool):
            cfg["mesh"]["replicate"] = value
            applied.append(f"mesh.replicate = {str(value).lower()}")
        elif key == "mesh_drift_seconds" and isinstance(value, int) and value >= 0:
            cfg["mesh"]["drift_check_seconds"] = value
            applied.append(f"mesh.drift_check_seconds = {value}")
        else:
            dropped.append(key)
    return applied, dropped


def _write_active_preset(preset_name: str, log: List[str]) -> None:
    """Best-effort: poke ``active_preset`` into ``~/.config/mackes-shell/state.json``.

    1.x's ``MackesState`` only consumes a handful of fields and ignores
    everything else, so writing here is safe: if the legacy file had
    additional 2.x keys, they're preserved on disk.
    """
    state_path = _legacy_dir() / "state.json"
    try:
        doc: dict = {}
        if state_path.is_file():
            try:
                raw = json.loads(state_path.read_text(encoding="utf-8"))
                if isinstance(raw, dict):
                    doc = raw
            except json.JSONDecodeError:
                # Corrupted — start fresh; we already logged this in detect().
                doc = {}
        doc["active_preset"] = preset_name
        state_path.parent.mkdir(parents=True, exist_ok=True)
        state_path.write_text(
            json.dumps(doc, indent=2) + "\n", encoding="utf-8"
        )
        log.append(f"  · wrote active_preset = {preset_name} to state.json")
    except OSError as exc:
        log.append(f"  · could not update state.json ({exc})")


# ---------------------------------------------------------------------------
# panel.toml read / write
# ---------------------------------------------------------------------------


def _default_panel_dict() -> dict:
    """Schema-default panel.toml as a Python dict.

    Mirrors ``mackes_config::default_config()`` in
    ``crates/mackes-config/src/lib.rs``. Kept here (instead of shelling
    out) so the importer has no Rust dependency — the Rust side reads
    this back via ``parse()`` and gets the same defaults via serde's
    ``#[serde(default = ...)]`` attributes.
    """
    return {
        "top_bar": {
            "status_items": [
                "mesh", "clipboard", "volume",
                "battery", "notifications", "user",
            ],
            "appmenu": True,
        },
        "dock": {"items": []},
        "mesh": {"replicate": True, "drift_check_seconds": 300},
    }


def _read_panel_toml_or_default() -> dict:
    """Read existing panel.toml or return the schema default."""
    path = _panel_toml_path()
    cfg = _default_panel_dict()
    if not path.is_file():
        return cfg
    try:
        import tomllib  # py311+; project minimum is 3.11
        with path.open("rb") as fp:
            parsed = tomllib.load(fp)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        logger.warning(
            "legacy_import: existing panel.toml unreadable (%s) — "
            "starting from defaults",
            exc,
        )
        return cfg

    # Merge parsed onto defaults so missing tables stay populated.
    for table in ("top_bar", "dock", "mesh"):
        if isinstance(parsed.get(table), dict):
            cfg[table].update(parsed[table])
    # Preserve unknown top-level tables (e.g. existing [migration]) so a
    # re-run doesn't drop fields written by an earlier import.
    for key, value in parsed.items():
        if key not in cfg and isinstance(value, dict):
            cfg[key] = value
    # Items list — overwrite (we'll re-merge inside import_to_panel_toml).
    if isinstance(parsed.get("dock"), dict):
        items = parsed["dock"].get("items")
        if isinstance(items, list):
            cfg["dock"]["items"] = [i for i in items if isinstance(i, dict)]
    return cfg


def _write_panel_toml(cfg: dict) -> None:
    """Emit ``cfg`` as TOML and atomically replace ``panel.toml``.

    Hand-rolled writer because we don't ship `tomli_w`. The shape we
    emit is tight to the schema in ``crates/mackes-config/src/lib.rs``;
    a schema change there is expected to land alongside an update
    here.
    """
    path = _panel_toml_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    text = _serialize_panel_toml(cfg)
    tmp = path.with_suffix(path.suffix + ".new")
    tmp.write_text(text, encoding="utf-8")
    os.replace(tmp, path)


def _serialize_panel_toml(cfg: dict) -> str:
    """Produce the on-disk TOML representation of ``cfg``.

    The shape is::

        [top_bar]
        status_items = [...]
        appmenu = true|false

        [[dock.items]]
        kind = "app"
        desktop = "..."

        [[dock.items]]
        kind = "mesh"
        id = "..."

        [mesh]
        replicate = true|false
        drift_check_seconds = N

        [migration]
        legacy_preset = "..."
        legacy_wallpaper = "..."

    Order is fixed for byte-for-byte idempotency.
    """
    parts: List[str] = []

    # [top_bar]
    tb = cfg.get("top_bar", {})
    parts.append("[top_bar]")
    items = tb.get("status_items", [])
    parts.append("status_items = " + _toml_string_list(items))
    parts.append("appmenu = " + _toml_bool(bool(tb.get("appmenu", True))))
    parts.append("")

    # [[dock.items]] — one table-array per entry
    for item in cfg.get("dock", {}).get("items", []):
        if not isinstance(item, dict):
            continue
        kind = item.get("kind")
        if kind == "app" and isinstance(item.get("desktop"), str):
            parts.append("[[dock.items]]")
            parts.append('kind = "app"')
            parts.append(f"desktop = {_toml_string(item['desktop'])}")
            parts.append("")
        elif kind == "mesh" and isinstance(item.get("id"), str):
            parts.append("[[dock.items]]")
            parts.append('kind = "mesh"')
            parts.append(f"id = {_toml_string(item['id'])}")
            parts.append("")

    # [mesh]
    mesh = cfg.get("mesh", {})
    parts.append("[mesh]")
    parts.append("replicate = " + _toml_bool(bool(mesh.get("replicate", True))))
    parts.append(
        "drift_check_seconds = "
        + str(int(mesh.get("drift_check_seconds", 300)))
    )

    # [migration] (only when we have something to record)
    migration = cfg.get("migration")
    if isinstance(migration, dict) and migration:
        parts.append("")
        parts.append("[migration]")
        # Sorted-key order keeps the file deterministic on re-runs.
        for key in sorted(migration):
            value = migration[key]
            if isinstance(value, str):
                parts.append(f"{key} = {_toml_string(value)}")
            elif isinstance(value, bool):
                parts.append(f"{key} = {_toml_bool(value)}")
            elif isinstance(value, int):
                parts.append(f"{key} = {value}")
            else:
                # Skip anything we can't represent — never silently
                # corrupt the file with bad TOML.
                logger.warning(
                    "legacy_import: dropping non-scalar migration key %s", key
                )

    # Trailing newline so editors don't complain.
    return "\n".join(parts) + "\n"


def _toml_string(value: str) -> str:
    """Quote a string for TOML (basic strings).

    We escape only the characters that the TOML 1.0 spec requires for
    basic strings: backslash, double-quote, control chars. The legacy
    paths and desktop ids we emit never contain newlines.
    """
    out: List[str] = ['"']
    for ch in value:
        codepoint = ord(ch)
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif codepoint < 0x20:
            out.append(f"\\u{codepoint:04x}")
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def _toml_string_list(items: list) -> str:
    """Render a list of strings as a TOML array literal."""
    body = ", ".join(_toml_string(s) for s in items if isinstance(s, str))
    return f"[{body}]"


def _toml_bool(value: bool) -> str:
    return "true" if value else "false"
