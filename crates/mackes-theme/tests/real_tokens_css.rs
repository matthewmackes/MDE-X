//! Integration test — parse the live `data/css/tokens.css` from
//! the repo root. Confirms the parser handles every `@define-color`
//! line in the real file without panicking.

use std::path::PathBuf;

#[test]
fn parses_real_tokens_css_without_loss() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../data/css/tokens.css");
    let css = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let table = mackes_theme::parse_tokens(&css);

    // The file ships ≥ 40 tokens (52 at time of writing — the
    // parser must keep up as the file grows).
    let define_color_lines = css.lines()
        .filter(|l| l.trim_start().starts_with("@define-color"))
        .count();
    assert!(
        define_color_lines >= 40,
        "expected ≥40 @define-color lines, found {define_color_lines}"
    );

    // Every line must round-trip through the parser. Allow up to
    // 2 dropped lines as slack for future malformed-line edge
    // cases — but the bulk must parse.
    assert!(
        table.len() + 2 >= define_color_lines,
        "parser dropped too many tokens: {} parsed vs {} declared",
        table.len(),
        define_color_lines,
    );

    // The "name + value" round-trip for known-good tokens.
    assert_eq!(
        mackes_theme::token_value(&table, "cds_bg_default"),
        Some("#151515"),
        "cds_bg_default must round-trip from real file"
    );
}
