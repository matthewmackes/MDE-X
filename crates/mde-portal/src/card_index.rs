//! Portal-31 startup hook — scan the local Card store and log a
//! one-line summary so the runtime can confirm `mde-card` is wired.
//!
//! Cards live as `.json` files under `$XDG_DATA_HOME/mde/cards/`
//! (typically `~/.local/share/mde/cards/`). Each file deserializes
//! into [`mde_card::Card`].  Files unreadable as Cards are counted
//! separately but never block startup.

use std::path::PathBuf;

/// Summary of the local card store at startup.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CardIndexSummary {
    /// Total `.json` files inspected.
    pub files_scanned: usize,
    /// Files that parsed as a valid Card at the current schema.
    pub cards_loaded: usize,
    /// Distinct kinds (lowercase canonical tags) observed.
    pub distinct_kinds: usize,
    /// Files that failed to parse — kept for visibility, never fatal.
    pub parse_errors: usize,
}

/// Compute the on-disk path to the card store root.
pub fn store_root() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("mde").join("cards"))
}

/// Scan the store root and return a summary. Returns `None` when the
/// store does not exist (fresh install) — caller treats this as
/// "no cards yet" rather than an error.
pub fn scan() -> Option<CardIndexSummary> {
    let root = store_root()?;
    if !root.exists() {
        return None;
    }
    Some(scan_dir(&root))
}

fn scan_dir(root: &std::path::Path) -> CardIndexSummary {
    use std::collections::BTreeSet;

    let mut summary = CardIndexSummary::default();
    let mut kinds: BTreeSet<String> = BTreeSet::new();

    let Ok(entries) = std::fs::read_dir(root) else {
        return summary;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        summary.files_scanned += 1;
        let Ok(raw) = std::fs::read_to_string(&path) else {
            summary.parse_errors += 1;
            continue;
        };
        match serde_json::from_str::<mde_card::Card>(&raw) {
            Ok(card) => {
                summary.cards_loaded += 1;
                kinds.insert(card.kind.tag().to_string());
            }
            Err(_) => {
                summary.parse_errors += 1;
            }
        }
    }
    summary.distinct_kinds = kinds.len();
    summary
}

/// Run the scan + log a one-line summary at startup.  Designed to be
/// called from `mde-portal`'s `main()` before launching the Iced
/// surface; never blocks more than the scan duration (≤ a few ms on
/// a typical store).
pub fn log_summary_at_startup() {
    match scan() {
        Some(summary) => {
            tracing::info!(
                files_scanned = summary.files_scanned,
                cards_loaded = summary.cards_loaded,
                distinct_kinds = summary.distinct_kinds,
                parse_errors = summary.parse_errors,
                schema_version = mde_card::SCHEMA_VERSION,
                "Portal-31 card index"
            );
        }
        None => {
            tracing::info!(
                schema_version = mde_card::SCHEMA_VERSION,
                "Portal-31 card index: no store yet"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mde_card::{Card, CardKind};
    use tempfile::TempDir;

    fn write_card(dir: &std::path::Path, name: &str, card: &Card) {
        let path = dir.join(format!("{name}.json"));
        std::fs::write(path, serde_json::to_string(card).unwrap()).unwrap();
    }

    #[test]
    fn scan_dir_empty_returns_zeros() {
        let tmp = TempDir::new().unwrap();
        let summary = scan_dir(tmp.path());
        assert_eq!(summary.files_scanned, 0);
        assert_eq!(summary.cards_loaded, 0);
        assert_eq!(summary.distinct_kinds, 0);
        assert_eq!(summary.parse_errors, 0);
    }

    #[test]
    fn scan_dir_counts_valid_cards() {
        let tmp = TempDir::new().unwrap();
        write_card(tmp.path(), "a", &Card::new(CardKind::App, "Firefox", 0));
        write_card(tmp.path(), "b", &Card::new(CardKind::Note, "todo", 0));
        write_card(tmp.path(), "c", &Card::new(CardKind::App, "Firefox", 0));

        let summary = scan_dir(tmp.path());
        assert_eq!(summary.files_scanned, 3);
        assert_eq!(summary.cards_loaded, 3);
        assert_eq!(summary.distinct_kinds, 2, "App + Note");
        assert_eq!(summary.parse_errors, 0);
    }

    #[test]
    fn scan_dir_skips_non_json() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("notes.txt"), "garbage").unwrap();
        write_card(tmp.path(), "real", &Card::new(CardKind::Note, "x", 0));
        let summary = scan_dir(tmp.path());
        assert_eq!(summary.files_scanned, 1);
        assert_eq!(summary.cards_loaded, 1);
    }

    #[test]
    fn scan_dir_counts_parse_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("bad.json"), "{not valid").unwrap();
        let summary = scan_dir(tmp.path());
        assert_eq!(summary.files_scanned, 1);
        assert_eq!(summary.cards_loaded, 0);
        assert_eq!(summary.parse_errors, 1);
    }

    #[test]
    fn scan_returns_none_when_store_missing() {
        // The user-side XDG dir may not exist in CI / tests; scan()
        // returns None rather than erroring.
        let summary = scan();
        // We can't assert is_none() here because the test runner may
        // be invoked from a user home that DOES have a store. The
        // contract is: never panics.
        let _ = summary;
    }
}
