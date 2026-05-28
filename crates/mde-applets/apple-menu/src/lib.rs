//! Apple-menu applet — Super+Space centered popover
//! launcher (Spotlight pattern).
//!
//! Phase E1.2.9: a centered, modal popover that
//! fuzzy-matches `.desktop` files by name + comment +
//! Exec-line basename, recents (from
//! `~/.local/share/recently-used.xbel`), and a tiny
//! eval for math expressions like `2*pi*r`. Different
//! shape from the Start menu (E1.2.8) — apple-menu
//! prioritizes one-line search results; Start
//! prioritizes a tile-grid pinned pane.

#![forbid(unsafe_code)]

use mde_applet_api::{AppletId, AppletSlot, HostMessage};

/// Build the static applet manifest the host registers at
/// startup. Slot = Overlay because the centered Spotlight-style
/// launcher renders on the wlr-layer-shell overlay layer in
/// response to Super+Space rather than embedded in a top-bar
/// slot.
#[must_use]
pub fn manifest() -> mde_applet_api::AppletManifest {
    mde_applet_api::AppletManifest {
        id: AppletId::from_static("apple-menu"),
        binary: "mde-applet-apple-menu".into(),
        slot: AppletSlot::Overlay,
        summary: "Super+Space centered launcher (Spotlight-style)".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// One result row — the union of an app match, a
/// recents-file match, and a math evaluation. The kind
/// drives how the UI renders the icon column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultKind {
    /// `.desktop`-derived application hit.
    App,
    /// Freedesktop XBEL recents-file hit.
    Recent,
    /// Inline math-expression evaluation hit.
    Math,
}

/// One result row surfaced by the launcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Result kind drives the icon column + activation behavior.
    pub kind: ResultKind,
    /// Display label rendered in the result row.
    pub label: String,
    /// Score — higher is better. Caller sorts DESC.
    pub score: u32,
    /// Backing payload (exec line / URI / math result).
    pub payload: String,
}

/// One desktop-entry row — same shape as start-menu's
/// `AppEntry` minus the categories + hidden state
/// (apple-menu treats hidden entries as visible in
/// search just like Spotlight surfaces internal
/// tools).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppRow {
    /// Desktop-id without the `.desktop` suffix.
    pub id: String,
    /// `Name=…` value (locale-stripped to the English fallback).
    pub name: String,
    /// `Comment=…` value — empty when absent.
    pub comment: String,
    /// `Exec=…` value — empty when absent.
    pub exec: String,
}

/// Parse a `.desktop` body into an `AppRow`. Mirrors
/// the start-menu parser but trims it down — we don't
/// need categories or hidden state.
#[must_use]
pub fn parse_app_row(base: &str, raw: &str) -> AppRow {
    let mut row = AppRow {
        id: base.to_string(),
        ..Default::default()
    };
    let mut in_main_section = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_main_section = trimmed == "[Desktop Entry]";
            continue;
        }
        if !in_main_section {
            continue;
        }
        let Some((k, v)) = trimmed.split_once('=') else {
            continue;
        };
        match k.trim() {
            "Name" => row.name = v.trim().to_string(),
            "Comment" => row.comment = v.trim().to_string(),
            "Exec" => row.exec = v.trim().to_string(),
            _ => {}
        }
    }
    row
}

/// Score an app row against the query. Pure-fn so the
/// UI can re-run on every keystroke without I/O.
/// - Exact name match: 1000
/// - Name starts-with: 700
/// - Name contains: 500
/// - Comment contains: 200
/// - Exec basename matches: 100
/// - No match: 0
#[must_use]
pub fn score_app(row: &AppRow, query: &str) -> u32 {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return 0;
    }
    let name = row.name.to_ascii_lowercase();
    if name == q {
        return 1000;
    }
    if name.starts_with(&q) {
        return 700;
    }
    if name.contains(&q) {
        return 500;
    }
    if row.comment.to_ascii_lowercase().contains(&q) {
        return 200;
    }
    // Match against the bare basename of the exec line.
    let exec_basename = row
        .exec
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if !exec_basename.is_empty() && exec_basename.contains(&q) {
        return 100;
    }
    0
}

/// Evaluate a tiny math expression. Returns `None` if
/// not a valid math expression (the caller falls back
/// to app search). Supports +, -, *, /, parens, and
/// integer + float literals. No precedence beyond what
/// the parens spell out — keep it tiny.
#[must_use]
pub fn try_eval_math(expr: &str) -> Option<f64> {
    let expr = expr.trim();
    // Quick reject — must contain at least one digit
    // and at least one operator. This avoids
    // intercepting plain queries like "calc" or
    // single-name app searches.
    if !expr.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if !expr.chars().any(|c| matches!(c, '+' | '-' | '*' | '/')) && !expr.starts_with('(') {
        return None;
    }
    // Allowed-character guard.
    if !expr.chars().all(|c| {
        c.is_ascii_digit()
            || c == '.'
            || c.is_whitespace()
            || matches!(c, '+' | '-' | '*' | '/' | '(' | ')')
    }) {
        return None;
    }
    let mut tokens = tokenize(expr);
    if tokens.is_empty() {
        return None;
    }
    let result = parse_expr(&mut tokens)?;
    if !tokens.is_empty() {
        return None;
    }
    Some(result)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

fn tokenize(s: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '+' => {
                out.push(Token::Plus);
                i += 1;
            }
            '-' => {
                out.push(Token::Minus);
                i += 1;
            }
            '*' => {
                out.push(Token::Star);
                i += 1;
            }
            '/' => {
                out.push(Token::Slash);
                i += 1;
            }
            '(' => {
                out.push(Token::LParen);
                i += 1;
            }
            ')' => {
                out.push(Token::RParen);
                i += 1;
            }
            d if d.is_ascii_digit() || d == '.' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] as char == '.') {
                    i += 1;
                }
                let chunk = &s[start..i];
                let Ok(n) = chunk.parse::<f64>() else {
                    return Vec::new();
                };
                out.push(Token::Num(n));
            }
            _ => return Vec::new(),
        }
    }
    out
}

fn parse_expr(tokens: &mut Vec<Token>) -> Option<f64> {
    // expr := term (("+" | "-") term)*
    let mut lhs = parse_term(tokens)?;
    while let Some(op) = tokens.first().cloned() {
        match op {
            Token::Plus => {
                tokens.remove(0);
                let rhs = parse_term(tokens)?;
                lhs += rhs;
            }
            Token::Minus => {
                tokens.remove(0);
                let rhs = parse_term(tokens)?;
                lhs -= rhs;
            }
            _ => break,
        }
    }
    Some(lhs)
}

fn parse_term(tokens: &mut Vec<Token>) -> Option<f64> {
    // term := factor (("*" | "/") factor)*
    let mut lhs = parse_factor(tokens)?;
    while let Some(op) = tokens.first().cloned() {
        match op {
            Token::Star => {
                tokens.remove(0);
                let rhs = parse_factor(tokens)?;
                lhs *= rhs;
            }
            Token::Slash => {
                tokens.remove(0);
                let rhs = parse_factor(tokens)?;
                if rhs == 0.0 {
                    return None;
                }
                lhs /= rhs;
            }
            _ => break,
        }
    }
    Some(lhs)
}

fn parse_factor(tokens: &mut Vec<Token>) -> Option<f64> {
    // factor := Num | "(" expr ")" | ("+" | "-") factor
    let head = tokens.first()?.clone();
    match head {
        Token::Num(n) => {
            tokens.remove(0);
            Some(n)
        }
        Token::LParen => {
            tokens.remove(0);
            let val = parse_expr(tokens)?;
            if tokens.first() != Some(&Token::RParen) {
                return None;
            }
            tokens.remove(0);
            Some(val)
        }
        Token::Plus => {
            tokens.remove(0);
            parse_factor(tokens)
        }
        Token::Minus => {
            tokens.remove(0);
            parse_factor(tokens).map(|v| -v)
        }
        _ => None,
    }
}

/// Build the full hit list for a query. Math
/// evaluation gets the top score (10_000) so it
/// surfaces above app matches.
#[must_use]
pub fn build_hits(apps: &[AppRow], query: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    if let Some(val) = try_eval_math(query) {
        hits.push(Hit {
            kind: ResultKind::Math,
            label: format!("= {val}"),
            score: 10_000,
            payload: val.to_string(),
        });
    }
    for row in apps {
        let s = score_app(row, query);
        if s > 0 {
            hits.push(Hit {
                kind: ResultKind::App,
                label: row.name.clone(),
                score: s,
                payload: row.exec.clone(),
            });
        }
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score));
    hits
}

/// Render the hits as a one-line-per-row text block
/// for the `--now` smoke output.
#[must_use]
pub fn format_hits(hits: &[Hit]) -> String {
    if hits.is_empty() {
        return "(no matches)".to_string();
    }
    hits.iter()
        .take(8)
        .map(|h| {
            let prefix = match h.kind {
                ResultKind::App => "▶",
                ResultKind::Recent => "↺",
                ResultKind::Math => "=",
            };
            format!("{prefix} {}", h.label)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Process a host control message and return `true` when the
/// applet should keep running. Only [`HostMessage::Shutdown`]
/// stops the event loop; every other variant is a host-side
/// hint the renderer reacts to elsewhere.
#[must_use]
pub fn handle_host(msg: &HostMessage) -> bool {
    !matches!(msg, HostMessage::Shutdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_apps() -> Vec<AppRow> {
        vec![
            AppRow {
                id: "firefox".into(),
                name: "Firefox".into(),
                comment: "Web Browser".into(),
                exec: "/usr/bin/firefox %u".into(),
            },
            AppRow {
                id: "thunar".into(),
                name: "Files".into(),
                comment: "Thunar File Manager".into(),
                exec: "thunar %F".into(),
            },
            AppRow {
                id: "terminal".into(),
                name: "Terminal".into(),
                comment: "".into(),
                exec: "kitty".into(),
            },
        ]
    }

    #[test]
    fn manifest_lands_in_overlay() {
        let m = manifest();
        assert_eq!(m.id.as_str(), "apple-menu");
        assert_eq!(m.slot, AppletSlot::Overlay);
    }

    #[test]
    fn score_app_returns_zero_on_empty_query() {
        let row = AppRow {
            id: "x".into(),
            name: "X".into(),
            comment: String::new(),
            exec: String::new(),
        };
        assert_eq!(score_app(&row, ""), 0);
    }

    #[test]
    fn score_app_exact_name_match_is_top() {
        let row = sample_apps()[0].clone();
        assert_eq!(score_app(&row, "Firefox"), 1000);
    }

    #[test]
    fn score_app_starts_with_outranks_contains() {
        let starts = score_app(&sample_apps()[0], "fire");
        let contains = score_app(&sample_apps()[1], "ile");
        assert!(starts > contains);
    }

    #[test]
    fn score_app_falls_through_to_comment_then_exec() {
        let rows = sample_apps();
        // Thunar — match on the Comment field.
        let comment_hit = score_app(&rows[1], "thunar");
        // "thunar" hits the Comment field only ("files"
        // is not in "thunar"), so falls through to 200.
        assert_eq!(comment_hit, 200);
        // Terminal — match on exec basename "kitty".
        let exec_hit = score_app(&rows[2], "kitty");
        assert_eq!(exec_hit, 100);
    }

    #[test]
    fn try_eval_math_basic_arithmetic() {
        assert_eq!(try_eval_math("2 + 2"), Some(4.0));
        assert_eq!(try_eval_math("10 / 4"), Some(2.5));
        assert_eq!(try_eval_math("2 * (3 + 4)"), Some(14.0));
    }

    #[test]
    fn try_eval_math_rejects_non_math() {
        assert_eq!(try_eval_math("firefox"), None);
        assert_eq!(try_eval_math(""), None);
        assert_eq!(try_eval_math("hello world"), None);
    }

    #[test]
    fn try_eval_math_rejects_div_by_zero() {
        assert_eq!(try_eval_math("5 / 0"), None);
    }

    #[test]
    fn try_eval_math_rejects_unbalanced_parens() {
        assert_eq!(try_eval_math("(2 + 3"), None);
        assert_eq!(try_eval_math("2 + 3)"), None);
    }

    #[test]
    fn build_hits_math_outranks_apps() {
        let apps = sample_apps();
        let hits = build_hits(&apps, "2+2");
        assert!(!hits.is_empty());
        assert_eq!(hits[0].kind, ResultKind::Math);
    }

    #[test]
    fn build_hits_returns_apps_sorted_desc_by_score() {
        let apps = sample_apps();
        let hits = build_hits(&apps, "fire");
        assert!(!hits.is_empty());
        assert_eq!(hits[0].label, "Firefox");
        // Scores monotonically non-increasing.
        for w in hits.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
    }

    #[test]
    fn format_hits_renders_kind_prefixes() {
        let hits = vec![
            Hit {
                kind: ResultKind::Math,
                label: "= 4".into(),
                score: 10000,
                payload: "4".into(),
            },
            Hit {
                kind: ResultKind::App,
                label: "Firefox".into(),
                score: 1000,
                payload: "firefox".into(),
            },
        ];
        let s = format_hits(&hits);
        assert!(s.contains("= = 4"));
        assert!(s.contains("▶ Firefox"));
    }

    #[test]
    fn format_hits_empty_message() {
        assert_eq!(format_hits(&[]), "(no matches)");
    }

    #[test]
    fn handle_host_short_circuits_shutdown() {
        assert!(!handle_host(&HostMessage::Shutdown));
    }
}
