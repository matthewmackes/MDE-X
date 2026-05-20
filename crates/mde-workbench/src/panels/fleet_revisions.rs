//! Fleet Revisions panel — lists desired_config revisions
//! from mded + rolls back to a chosen revision id.
//!
//! CB-1.5 partial: replaces the v1.x
//! `mackes/workbench/fleet/revisions.py` GTK3 panel. F.12
//! shipped the Python wrapper around the same `mded
//! revisions` subcommand tree; this Iced port mirrors it.

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length, Padding, Task};
use serde::Deserialize;

use crate::panels::fleet_settings::run_mded;

/// One row from `mded revisions list --json`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Revision {
    pub revision_id: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct FleetRevisionsPanel {
    pub revisions: Vec<Revision>,
    pub status: String,
    pub busy: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    RefreshClicked,
    RefreshCompleted(Result<Vec<Revision>, String>),
    RollbackClicked(String),
    RollbackCompleted(Result<String, String>),
}

impl FleetRevisionsPanel {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Initial load when the panel is navigated to. Same shape
    /// as `RefreshClicked` — kept distinct so the load-fire
    /// path can be unit-tested without the spawn.
    pub fn load() -> Task<crate::Message> {
        Self::dispatch_refresh()
    }

    fn dispatch_refresh() -> Task<crate::Message> {
        let args = list_args();
        Task::perform(
            async move {
                let stdout = run_mded(&args).await?;
                parse_revisions_json(&stdout).map_err(|e| e.to_string())
            },
            |result| {
                crate::Message::FleetRevisions(Message::RefreshCompleted(result))
            },
        )
    }

    pub fn update(&mut self, message: Message) -> Task<crate::Message> {
        match message {
            Message::RefreshClicked => {
                if self.busy {
                    return Task::none();
                }
                self.busy = true;
                self.status = "Refreshing…".into();
                Self::dispatch_refresh()
            }
            Message::RefreshCompleted(Ok(rows)) => {
                let n = rows.len();
                self.revisions = rows;
                self.busy = false;
                self.status = if n == 0 {
                    "No revisions yet.".into()
                } else {
                    format!("Loaded {n} revisions.")
                };
                Task::none()
            }
            Message::RefreshCompleted(Err(e)) => {
                self.busy = false;
                self.status = format!("Refresh failed: {e}");
                Task::none()
            }
            Message::RollbackClicked(id) => {
                if self.busy {
                    return Task::none();
                }
                self.busy = true;
                self.status = format!("Rolling back to {id}…");
                let args = rollback_args(&id, "all");
                Task::perform(
                    async move { run_mded(&args).await },
                    |result| {
                        crate::Message::FleetRevisions(Message::RollbackCompleted(
                            result,
                        ))
                    },
                )
            }
            Message::RollbackCompleted(Ok(out)) => {
                self.busy = false;
                self.status = if out.trim().is_empty() {
                    "Rollback queued.".into()
                } else {
                    format!("Rollback queued: {}", out.trim())
                };
                Task::none()
            }
            Message::RollbackCompleted(Err(e)) => {
                self.busy = false;
                self.status = format!("Rollback failed: {e}");
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, crate::Message> {
        let refresh_label = if self.busy { "Refreshing…" } else { "Refresh" };
        let refresh_btn = {
            let mut b = button(text(refresh_label));
            if !self.busy {
                b = b.on_press(crate::Message::FleetRevisions(
                    Message::RefreshClicked,
                ));
            }
            b
        };

        let rows: Vec<Element<'_, crate::Message>> = self
            .revisions
            .iter()
            .map(|rev| revision_row(rev, self.busy))
            .collect();

        let list_body: Element<'_, crate::Message> = if rows.is_empty() {
            text("(no revisions yet — `mded` returned an empty list)")
                .size(13)
                .into()
        } else {
            scrollable(column(rows).spacing(4))
                .height(Length::Fixed(360.0))
                .into()
        };

        column![
            row![refresh_btn, text(&self.status).size(13)].spacing(12),
            container(list_body).width(Length::Fill),
        ]
        .spacing(12)
        .width(Length::Fill)
        .padding(Padding::new(0.0))
        .into()
    }
}

fn revision_row<'a>(rev: &'a Revision, busy: bool) -> Element<'a, crate::Message> {
    let label = format!(
        "r-{}  [{}]  by {}  ({})  — {}",
        rev.revision_id,
        if rev.state.is_empty() {
            "?"
        } else {
            &rev.state
        },
        if rev.author.is_empty() {
            "?"
        } else {
            &rev.author
        },
        if rev.created_at.is_empty() {
            "?"
        } else {
            &rev.created_at
        },
        if rev.summary.is_empty() {
            "(no summary)"
        } else {
            &rev.summary
        },
    );
    let mut rollback_btn = button(text("Rollback"));
    if !busy {
        rollback_btn = rollback_btn.on_press(crate::Message::FleetRevisions(
            Message::RollbackClicked(rev.revision_id.clone()),
        ));
    }
    row![text(label).size(13).width(Length::Fill), rollback_btn]
        .spacing(12)
        .into()
}

/// Pure-fn arg builder for `mded revisions list --json`.
#[must_use]
pub fn list_args() -> Vec<String> {
    vec!["revisions".to_string(), "list".to_string(), "--json".to_string()]
}

/// Pure-fn arg builder for `mded revisions rollback <id> --peers <sel>`.
#[must_use]
pub fn rollback_args(id: &str, peers: &str) -> Vec<String> {
    vec![
        "revisions".to_string(),
        "rollback".to_string(),
        id.to_string(),
        "--peers".to_string(),
        peers.to_string(),
    ]
}

/// Parse the JSON array `mded revisions list --json` prints.
/// Empty input parses as an empty list — matches the
/// `(no revisions)` text-mode fallback.
pub fn parse_revisions_json(s: &str) -> Result<Vec<Revision>, serde_json::Error> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<Revision>>(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_args_match_locked_grammar() {
        assert_eq!(list_args(), vec!["revisions", "list", "--json"]);
    }

    #[test]
    fn rollback_args_carry_id_and_peers() {
        assert_eq!(
            rollback_args("r-2026-05-20-0007", "all"),
            vec![
                "revisions",
                "rollback",
                "r-2026-05-20-0007",
                "--peers",
                "all",
            ]
        );
    }

    #[test]
    fn parse_revisions_json_handles_empty_input() {
        assert_eq!(parse_revisions_json("").unwrap(), vec![]);
        assert_eq!(parse_revisions_json("[]").unwrap(), vec![]);
    }

    #[test]
    fn parse_revisions_json_decodes_single_row() {
        let json = r#"[
            {
                "revision_id": "1",
                "author": "alice",
                "summary": "Bump theme.name",
                "state": "applied",
                "created_at": "2026-05-20T12:00:00Z"
            }
        ]"#;
        let rows = parse_revisions_json(json).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].author, "alice");
        assert_eq!(rows[0].state, "applied");
    }

    #[test]
    fn parse_revisions_json_tolerates_missing_optional_fields() {
        let json = r#"[{"revision_id": "1"}]"#;
        let rows = parse_revisions_json(json).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].author.is_empty());
        assert!(rows[0].summary.is_empty());
    }

    #[test]
    fn parse_revisions_json_rejects_malformed_input() {
        assert!(parse_revisions_json("not json").is_err());
        assert!(parse_revisions_json("{}").is_err()); // object, not array
    }

    #[test]
    fn refresh_completed_ok_sets_loaded_status_with_count() {
        let mut panel = FleetRevisionsPanel::new();
        panel.busy = true;
        let rows = vec![
            Revision {
                revision_id: "1".into(),
                author: "a".into(),
                summary: "x".into(),
                state: "applied".into(),
                created_at: "z".into(),
            },
            Revision {
                revision_id: "2".into(),
                author: "b".into(),
                summary: "y".into(),
                state: "approved".into(),
                created_at: "z".into(),
            },
        ];
        let _ = panel.update(Message::RefreshCompleted(Ok(rows)));
        assert!(!panel.busy);
        assert_eq!(panel.revisions.len(), 2);
        assert!(panel.status.contains("Loaded 2"));
    }

    #[test]
    fn refresh_completed_empty_list_surfaces_friendly_status() {
        let mut panel = FleetRevisionsPanel::new();
        let _ = panel.update(Message::RefreshCompleted(Ok(Vec::new())));
        assert_eq!(panel.status, "No revisions yet.");
    }

    #[test]
    fn refresh_completed_err_clears_busy_and_surfaces_message() {
        let mut panel = FleetRevisionsPanel::new();
        panel.busy = true;
        let _ = panel.update(Message::RefreshCompleted(Err("timeout".into())));
        assert!(!panel.busy);
        assert!(panel.status.contains("timeout"));
    }

    #[test]
    fn rollback_completed_ok_clears_busy() {
        let mut panel = FleetRevisionsPanel::new();
        panel.busy = true;
        let _ = panel.update(Message::RollbackCompleted(Ok(String::new())));
        assert!(!panel.busy);
        assert_eq!(panel.status, "Rollback queued.");
    }

    #[test]
    fn rollback_clicked_while_busy_is_noop() {
        let mut panel = FleetRevisionsPanel::new();
        panel.busy = true;
        panel.status = "Refreshing…".into();
        let _ = panel.update(Message::RollbackClicked("1".into()));
        // Status unchanged — second click during a refresh
        // should not jump to a rollback.
        assert_eq!(panel.status, "Refreshing…");
    }

    #[test]
    fn refresh_clicked_while_busy_is_noop() {
        let mut panel = FleetRevisionsPanel::new();
        panel.busy = true;
        panel.status = "Rolling back to 1…".into();
        let _ = panel.update(Message::RefreshClicked);
        assert_eq!(panel.status, "Rolling back to 1…");
    }
}
