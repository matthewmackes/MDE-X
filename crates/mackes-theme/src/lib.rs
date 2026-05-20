//! Phase E3.1 — Carbon → cosmic-theme adapter.
//!
//! Parses the canonical `data/css/tokens.css` GTK token file into
//! typed Rust values. The output is a [`TokenTable`] keyed by
//! token name (`cds_bg_default`, `mackes_accent`, …) with hex
//! color values like `"#151515"`.
//!
//! Why pure data: the cosmic-theme `Theme` type lives upstream and
//! is heavy. By emitting just the value table here, the cosmic-
//! theme consumer (mackes-panel + future Iced applets) can plug
//! these into their own theme builders without forcing this crate
//! to link cosmic-theme. A `into_cosmic_theme()` builder lands
//! alongside Phase E.1 when the panel actually switches to Iced.
//!
//! The parser understands GTK's CSS-with-extensions dialect:
//!
//! ```css
//! @define-color cds_bg_default       #151515;
//! @define-color cds_text_primary     #f0f0f0;
//! ```
//!
//! Lines that aren't `@define-color` are ignored (comments,
//! `@import`, `@keyframes`, etc.) so the parser stays robust
//! against the token file growing new sections.
//!
//! The accent override surface (Mackes-per-preset) consumes the
//! same `cds_*` token names + adds the `mackes_accent`
//! parameter. [`accent_override`] applies one such override to a
//! base [`TokenTable`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;

/// One parsed token. The hex value is held as a `String` so this
/// crate stays dep-free; downstream cosmic-theme builders convert
/// it to `palette::Srgb` or whatever their color type is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Token name without the `cds_` / `mackes_` prefix split —
    /// matches the source file's identifier verbatim
    /// (`cds_bg_default`, `mackes_accent`, …).
    pub name:  String,
    /// Color value as written in the source — hex (`#RGB`,
    /// `#RRGGBB`, `#RRGGBBAA`) or a CSS named color. The parser
    /// doesn't normalize; if you need a `Srgb` build, use
    /// [`Token::as_rgb`].
    pub value: String,
}

impl Token {
    /// Parse the hex value to `(r, g, b, a)` u8 components. `None`
    /// when the value isn't a 3-, 6-, or 8-char hex literal.
    #[must_use]
    pub fn as_rgb(&self) -> Option<(u8, u8, u8, u8)> {
        parse_hex_color(&self.value)
    }
}

/// Map of every parsed token, keyed by name.
pub type TokenTable = BTreeMap<String, Token>;

/// Parse the contents of a `tokens.css` file. Returns the full
/// token map.
///
/// Robust to malformed lines: any line that doesn't match
/// `@define-color NAME VALUE;` is skipped without raising. Errors
/// are intentional only for the `value` parser when a downstream
/// `as_rgb()` call requests one — the token table itself never
/// fails to build.
#[must_use]
pub fn parse_tokens(css: &str) -> TokenTable {
    let mut out = TokenTable::new();
    for line in css.lines() {
        if let Some(token) = parse_define_color_line(line) {
            out.insert(token.name.clone(), token);
        }
    }
    out
}

/// Parse one line. Returns `None` if the line isn't a
/// `@define-color` declaration.
fn parse_define_color_line(line: &str) -> Option<Token> {
    let line = line.trim();
    let rest = line.strip_prefix("@define-color")?;
    // Strip trailing `;` if present (it's optional in GTK CSS).
    let body = rest.trim().trim_end_matches(';').trim();
    let mut parts = body.split_whitespace();
    let name = parts.next()?.to_string();
    // Name must be a valid identifier — bail on anything weird.
    if !is_ident(&name) {
        return None;
    }
    // The value is the remainder. Could be a hex color, a named
    // color, or even a `rgb(…)` call; we don't validate here, just
    // preserve.
    let value: String = parts.collect::<Vec<_>>().join(" ");
    if value.is_empty() {
        return None;
    }
    Some(Token { name, value })
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Parse a `#RGB`, `#RRGGBB`, or `#RRGGBBAA` hex literal into
/// `(r, g, b, a)` components. Returns `None` for any other input.
#[must_use]
pub fn parse_hex_color(s: &str) -> Option<(u8, u8, u8, u8)> {
    let s = s.trim();
    let hex = s.strip_prefix('#')?;
    let bytes = hex.as_bytes();
    fn nyb(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    match bytes.len() {
        3 => {
            // #RGB → expand each nibble.
            let r = nyb(bytes[0])?;
            let g = nyb(bytes[1])?;
            let b = nyb(bytes[2])?;
            Some((r * 17, g * 17, b * 17, 0xFF))
        }
        6 => {
            let r = nyb(bytes[0])? << 4 | nyb(bytes[1])?;
            let g = nyb(bytes[2])? << 4 | nyb(bytes[3])?;
            let b = nyb(bytes[4])? << 4 | nyb(bytes[5])?;
            Some((r, g, b, 0xFF))
        }
        8 => {
            let r = nyb(bytes[0])? << 4 | nyb(bytes[1])?;
            let g = nyb(bytes[2])? << 4 | nyb(bytes[3])?;
            let b = nyb(bytes[4])? << 4 | nyb(bytes[5])?;
            let a = nyb(bytes[6])? << 4 | nyb(bytes[7])?;
            Some((r, g, b, a))
        }
        _ => None,
    }
}

/// Apply an accent override to a base token table. Replaces the
/// `mackes_accent` entry (and `cds_focus` if `also_focus` is
/// set — the per-preset spec ties the focus ring colour to the
/// accent). Other tokens are untouched.
pub fn accent_override(base: &mut TokenTable, accent_hex: &str, also_focus: bool) {
    base.insert(
        "mackes_accent".into(),
        Token {
            name: "mackes_accent".into(),
            value: accent_hex.to_string(),
        },
    );
    if also_focus {
        base.insert(
            "cds_focus".into(),
            Token {
                name: "cds_focus".into(),
                value: accent_hex.to_string(),
            },
        );
    }
}

/// Convenience accessor — returns the token's value if present.
#[must_use]
pub fn token_value<'a>(table: &'a TokenTable, name: &str) -> Option<&'a str> {
    table.get(name).map(|t| t.value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../testdata/sample-tokens.css");

    #[test]
    fn parse_extracts_every_define_color_line() {
        let table = parse_tokens(SAMPLE);
        // sample-tokens.css has 4 @define-color entries.
        assert_eq!(table.len(), 4);
        assert_eq!(token_value(&table, "cds_bg_default"), Some("#151515"));
        assert_eq!(token_value(&table, "cds_text_primary"), Some("#f0f0f0"));
        assert_eq!(token_value(&table, "mackes_accent"), Some("#ff8b3d"));
        assert_eq!(token_value(&table, "cds_focus"), Some("#73bcf7"));
    }

    #[test]
    fn parse_skips_comments_and_at_rules() {
        let css = "
            /* a comment */
            @import url(foo.css);
            @define-color real #abcdef;
            @keyframes blip { from { x: 1 } to { x: 2 } }
        ";
        let table = parse_tokens(css);
        assert_eq!(table.len(), 1);
        assert!(table.contains_key("real"));
    }

    #[test]
    fn parse_rejects_invalid_identifiers() {
        let css = "@define-color 1bad #abcdef;";
        let table = parse_tokens(css);
        assert!(table.is_empty());
    }

    #[test]
    fn parse_handles_missing_semicolon() {
        let css = "@define-color foo #112233";
        let table = parse_tokens(css);
        assert_eq!(token_value(&table, "foo"), Some("#112233"));
    }

    #[test]
    fn token_as_rgb_handles_6_char_hex() {
        let t = Token { name: "x".into(), value: "#abcdef".into() };
        assert_eq!(t.as_rgb(), Some((0xAB, 0xCD, 0xEF, 0xFF)));
    }

    #[test]
    fn parse_hex_color_handles_3_char_shorthand() {
        assert_eq!(parse_hex_color("#abc"), Some((0xAA, 0xBB, 0xCC, 0xFF)));
        assert_eq!(parse_hex_color("#fff"), Some((0xFF, 0xFF, 0xFF, 0xFF)));
        assert_eq!(parse_hex_color("#000"), Some((0x00, 0x00, 0x00, 0xFF)));
    }

    #[test]
    fn parse_hex_color_handles_8_char_with_alpha() {
        assert_eq!(parse_hex_color("#abcdef80"), Some((0xAB, 0xCD, 0xEF, 0x80)));
    }

    #[test]
    fn parse_hex_color_rejects_non_hex() {
        assert!(parse_hex_color("not a color").is_none());
        assert!(parse_hex_color("#xyzxyz").is_none());
        assert!(parse_hex_color("abcdef").is_none(), "must start with #");
    }

    #[test]
    fn parse_hex_color_rejects_wrong_length() {
        assert!(parse_hex_color("#ab").is_none());
        assert!(parse_hex_color("#abcd").is_none());
        assert!(parse_hex_color("#abcde").is_none());
        assert!(parse_hex_color("#abcdefab12").is_none());
    }

    #[test]
    fn accent_override_replaces_accent_only() {
        let mut t = parse_tokens(SAMPLE);
        let before_focus = token_value(&t, "cds_focus").unwrap().to_string();
        accent_override(&mut t, "#aabbcc", false);
        assert_eq!(token_value(&t, "mackes_accent"), Some("#aabbcc"));
        assert_eq!(token_value(&t, "cds_focus"), Some(before_focus.as_str()));
    }

    #[test]
    fn accent_override_with_focus_replaces_both() {
        let mut t = parse_tokens(SAMPLE);
        accent_override(&mut t, "#deadbe", true);
        assert_eq!(token_value(&t, "mackes_accent"), Some("#deadbe"));
        assert_eq!(token_value(&t, "cds_focus"), Some("#deadbe"));
    }

    #[test]
    fn accent_override_inserts_when_missing() {
        let mut t = TokenTable::new();
        accent_override(&mut t, "#112233", true);
        assert_eq!(t.len(), 2);
        assert_eq!(token_value(&t, "mackes_accent"), Some("#112233"));
        assert_eq!(token_value(&t, "cds_focus"), Some("#112233"));
    }

    #[test]
    fn is_ident_handles_dashes_and_underscores() {
        assert!(is_ident("mackes_accent"));
        assert!(is_ident("cds-focus"));
        assert!(is_ident("_private"));
        assert!(!is_ident(""));
        assert!(!is_ident("1leading-digit"));
        assert!(!is_ident("has space"));
    }

    #[test]
    fn parser_handles_real_tokens_css_excerpt() {
        // The real file ships ~150 @define-color lines; this test
        // confirms the parser handles a representative chunk
        // without panicking + that the lookups round-trip.
        let css = "
            /* leading comment */
            @define-color cds_bg_default       #151515;
            @define-color cds_bg_layer_01      #1b1d21;
            @define-color cds_text_primary     #f0f0f0;
            @define-color cds_focus            #73bcf7;
            @define-color mackes_accent        #ff8b3d;
        ";
        let table = parse_tokens(css);
        assert_eq!(table.len(), 5);
        assert!(table["cds_bg_default"].as_rgb().is_some());
    }
}
