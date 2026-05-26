//! YAML schema for `bus-hooks.yaml`.
//!
//! Top-level layout:
//!
//! ```yaml
//! adapters:
//!   github:
//!     rules:
//!       - name: github-push
//!         match:
//!           event: push
//!         publish:
//!           topic: gh/push
//!           priority: default
//!           title: "{{ repo }} push to {{ branch }}"
//!           body: "{{ pusher }} pushed {{ commit_count }} commits"
//! ```
//!
//! Each adapter is a named source of webhooks. Built-in adapters
//! (github, gitea, sonarr, nut, home_assistant, generic) ship with
//! per-adapter Rust extractors that pre-populate template fields
//! from the payload before rules evaluate. Rules within an adapter
//! match in declaration order — first hit wins.
//!
//! `match` blocks support three predicate forms:
//!
//! - `event: <name>` — equal to the per-adapter event name
//!   (e.g. GitHub's `X-GitHub-Event` header value).
//! - `header.<Name>: <value>` — exact-equal HTTP header (case-
//!   insensitive name lookup).
//! - `body.<dotted.path>: <value>` — exact-equal JSON field at the
//!   dotted path (`repository.full_name`, `commits.0.author.name`).
//!
//! `priority` accepts `min`/`default`/`high`/`urgent` (matches
//! BUS-2.1 + design-doc §6 lock). The default is `default`.

use std::collections::BTreeMap;

use serde::Deserialize;
use thiserror::Error;

/// Top-level YAML config — `bus-hooks.yaml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HooksConfig {
    /// One entry per adapter the operator wants to handle.
    /// Adapters not listed receive a 404 from the server (visible
    /// in the access log so operators see misconfigured webhooks).
    #[serde(default)]
    pub adapters: BTreeMap<String, AdapterConfig>,
}

impl HooksConfig {
    /// Parse a YAML body into a [`HooksConfig`].
    ///
    /// # Errors
    /// Returns [`ConfigError::Parse`] when the YAML is malformed
    /// or fails the `deny_unknown_fields` check.
    pub fn parse_yaml(body: &str) -> Result<Self, ConfigError> {
        serde_yaml::from_str(body).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Read + parse the config at `path`. Returns
    /// [`ConfigError::Missing`] when the file isn't present so the
    /// listener can spawn with an empty config (every adapter
    /// returns 404 until the operator drops a `bus-hooks.yaml` in
    /// place).
    ///
    /// # Errors
    /// - [`ConfigError::Missing`] — file does not exist
    /// - [`ConfigError::Io`] — read failed (permission, etc.)
    /// - [`ConfigError::Parse`] — invalid YAML
    pub fn load(path: &std::path::Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Err(ConfigError::Missing(path.display().to_string()));
        }
        let body = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::Io(format!("{}: {e}", path.display())))?;
        Self::parse_yaml(&body)
    }
}

/// One adapter's rule list.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterConfig {
    /// Match-evaluation order is declaration order — first hit
    /// wins. Empty list = adapter accepts requests but never
    /// publishes (no rule fired); useful for staging a new source
    /// while still seeing the access-log line.
    #[serde(default)]
    pub rules: Vec<Rule>,
}

/// A single transform rule.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Human-readable name. Appears in logs + audit (BUS-7.1).
    pub name: String,
    /// Predicates the request must satisfy.
    #[serde(rename = "match", default)]
    pub r#match: Match,
    /// What to publish when the predicates match.
    pub publish: PublishSpec,
}

/// Match predicates.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Match {
    /// Equality on the per-adapter event name (GitHub
    /// `X-GitHub-Event`, Gitea `X-Gitea-Event`, etc.). `None`
    /// matches any event.
    #[serde(default)]
    pub event: Option<String>,
    /// Equality on extracted template fields (after the adapter's
    /// Rust extractor runs). Useful for narrowing on `action: opened`
    /// or `state: closed`.
    #[serde(default)]
    pub field: BTreeMap<String, String>,
}

/// What to publish on a match.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishSpec {
    /// Topic path (Tera-templated against the adapter's
    /// extracted fields).
    pub topic: String,
    /// Priority (`min` / `default` / `high` / `urgent`). Defaults
    /// to `default` when omitted.
    #[serde(default)]
    pub priority: Priority,
    /// Title template (Tera; rendered against extracted fields).
    /// Maps to ntfy's `X-Title` header.
    #[serde(default)]
    pub title: String,
    /// Body template (Tera; rendered against extracted fields).
    /// Becomes the ntfy message body.
    #[serde(default)]
    pub body: String,
}

/// Priority levels — matches BUS-2.1 (surface dispatch table) +
/// `docs/design/v6.x-mackes-bus.md` §6.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// Silent log only; no UI surface.
    Min,
    /// Tray + dock badge (Round 19 default).
    #[default]
    Default,
    /// Status-zone strip + sound, persistent until ack.
    High,
    /// Theater takeover + wallpaper stripe + phone push.
    Urgent,
}

impl Priority {
    /// Map to ntfy's `X-Priority` header (1..5, 3 = default).
    /// ntfy doesn't ship a "min/default/high/urgent" enum; this
    /// translation matches the closest semantic.
    #[must_use]
    pub const fn ntfy_header(self) -> &'static str {
        match self {
            Self::Min => "1",
            Self::Default => "3",
            Self::High => "4",
            Self::Urgent => "5",
        }
    }
}

/// Errors loading or parsing `bus-hooks.yaml`.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// File at the configured path doesn't exist. Distinct from
    /// `Io` so the listener can degrade gracefully (empty config).
    #[error("hooks config not present: {0}")]
    Missing(String),
    /// `read_to_string` failed (permission, encoding, etc.).
    #[error("hooks config read failed: {0}")]
    Io(String),
    /// `serde_yaml` rejected the body.
    #[error("hooks config parse failed: {0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_github_push_rule() {
        let yaml = r#"
adapters:
  github:
    rules:
      - name: github-push
        match:
          event: push
        publish:
          topic: gh/push
          priority: default
          title: "{{ repo }} push to {{ branch }}"
          body: "{{ pusher }} pushed {{ commit_count }} commits"
"#;
        let cfg = HooksConfig::parse_yaml(yaml).expect("parses ok");
        let gh = cfg.adapters.get("github").expect("github adapter present");
        assert_eq!(gh.rules.len(), 1);
        let rule = &gh.rules[0];
        assert_eq!(rule.name, "github-push");
        assert_eq!(rule.r#match.event.as_deref(), Some("push"));
        assert_eq!(rule.publish.topic, "gh/push");
        assert_eq!(rule.publish.priority, Priority::Default);
    }

    #[test]
    fn missing_priority_defaults_to_default() {
        let yaml = r#"
adapters:
  github:
    rules:
      - name: github-push
        match:
          event: push
        publish:
          topic: gh/push
          title: "x"
          body: "y"
"#;
        let cfg = HooksConfig::parse_yaml(yaml).expect("parses ok");
        let rule = &cfg.adapters["github"].rules[0];
        assert_eq!(rule.publish.priority, Priority::Default);
    }

    #[test]
    fn unknown_top_level_field_rejected() {
        let yaml = r#"
adapters: {}
extra: nope
"#;
        let err = HooksConfig::parse_yaml(yaml).expect_err("should reject");
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn priority_maps_to_ntfy_header_levels() {
        assert_eq!(Priority::Min.ntfy_header(), "1");
        assert_eq!(Priority::Default.ntfy_header(), "3");
        assert_eq!(Priority::High.ntfy_header(), "4");
        assert_eq!(Priority::Urgent.ntfy_header(), "5");
    }

    #[test]
    fn empty_adapter_block_parses() {
        let yaml = "adapters:\n  gitea: {}\n";
        let cfg = HooksConfig::parse_yaml(yaml).expect("parses ok");
        assert!(cfg.adapters["gitea"].rules.is_empty());
    }

    #[test]
    fn field_match_predicate_parses() {
        let yaml = r#"
adapters:
  github:
    rules:
      - name: pr-opened
        match:
          event: pull_request
          field:
            action: opened
        publish:
          topic: gh/pr
          title: "x"
          body: "y"
"#;
        let cfg = HooksConfig::parse_yaml(yaml).expect("parses ok");
        let rule = &cfg.adapters["github"].rules[0];
        assert_eq!(rule.r#match.field.get("action").map(String::as_str), Some("opened"));
    }

    #[test]
    fn load_missing_returns_missing_variant() {
        let p = std::path::Path::new("/nonexistent/path/bus-hooks.yaml");
        let err = HooksConfig::load(p).expect_err("missing file should error");
        assert!(matches!(err, ConfigError::Missing(_)));
    }

    #[test]
    fn load_existing_file_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hooks.yaml");
        std::fs::write(
            &path,
            "adapters:\n  github:\n    rules:\n      - name: x\n        match:\n          event: push\n        publish:\n          topic: gh/push\n          title: t\n          body: b\n",
        )
        .unwrap();
        let cfg = HooksConfig::load(&path).expect("loads ok");
        assert_eq!(cfg.adapters["github"].rules.len(), 1);
    }
}
