//! `mde-bus topic` — list every known topic in the registry
//! or match against a wildcard pattern.
//!
//! Two sub-verbs:
//!
//! - `list` — print every seeded + dynamically-created topic
//!   as `<name>\t<priority>\t<description>` (TSV-friendly).
//! - `match <pattern>` — print topics matching an MQTT wildcard
//!   (`+` / `#`), useful for previewing a `tail` or `sub` glob.

use anyhow::Result;
use clap::Subcommand;

use crate::seed;
use crate::topic::Registry;

/// CLI sub-verbs for `mde-bus topic`.
#[derive(Subcommand, Debug)]
pub enum TopicOp {
    /// Print every known topic.
    List {
        /// Emit JSON Lines instead of TSV. Each line is a
        /// `{name, priority, description}` object suitable for
        /// piping to `jq`.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Print topics matching the given pattern.
    Match {
        /// MQTT-style pattern (`+` single-level, `#` multi-level).
        pattern: String,
    },
}

/// Build a registry pre-loaded with the 12 default topics. Used
/// by both `list` and `match` so they have something to enumerate
/// even when the daemon hasn't been started yet.
fn build_seeded_registry() -> Result<Registry> {
    let mut reg = Registry::new();
    seed::seed_defaults(&mut reg)?;
    Ok(reg)
}

/// Execute the `topic` verb.
pub fn run(op: TopicOp) -> Result<()> {
    let reg = build_seeded_registry()?;
    match op {
        TopicOp::List { json } => {
            for t in reg.iter() {
                if json {
                    let priority_str = format!("{:?}", t.priority_default).to_lowercase();
                    let val = serde_json::json!({
                        "name": t.name,
                        "priority": priority_str,
                        "description": t.description,
                    });
                    println!("{val}");
                } else {
                    println!("{}\t{:?}\t{}", t.name, t.priority_default, t.description);
                }
            }
        }
        TopicOp::Match { pattern } => {
            for t in reg.iter() {
                if crate::wildcard::matches(&pattern, &t.name) {
                    println!("{}", t.name);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_runs_without_error() {
        run(TopicOp::List { json: false }).unwrap();
        run(TopicOp::List { json: true }).unwrap();
    }

    #[test]
    fn match_filters_by_pattern() {
        // Verify against the registry directly to avoid stdout
        // capture.
        let reg = build_seeded_registry().unwrap();
        let mut matched: Vec<&str> = reg
            .iter()
            .filter(|t| crate::wildcard::matches("mon/#", &t.name))
            .map(|t| t.name.as_str())
            .collect();
        matched.sort();
        assert!(matched.contains(&"mon/cpu"));
        assert!(matched.contains(&"mon/memory"));
        assert!(matched.contains(&"mon/disk"));
        assert!(matched.contains(&"mon/network"));
    }

    #[test]
    fn match_verb_runs_without_error() {
        run(TopicOp::Match {
            pattern: "mon/+".to_string(),
        })
        .unwrap();
    }
}
