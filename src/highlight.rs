//! Syntax highlighting by file extension (syntect), rendered as ratatui spans.
//!
//! Trade-off (milestone 1): highlighting is STATELESS per frame — the visible
//! window is parsed without carrying `HighlightLines` state from previous
//! lines, so a window opened mid-block-comment colors the remainder as plain
//! text until the comment closes. Per-tab incremental state is the later
//! optimization. Never highlight beyond the visible window.
//!
//! Token colors are a curated Dark+-flavored scope map over syntect's default
//! grammars; unmatched scopes fall back to the theme default foreground
//! (#D4D4D4), matching VS Code's base text. External grammars (Typst & co.)
//! arrive with the `languages/` data-dir loader (see docs/plan.md).

use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SyntectColor, FontStyle, ScopeSelectors, StyleModifier, Theme as SyntectTheme,
    ThemeItem, ThemeSettings,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static TOKEN_THEME: OnceLock<SyntectTheme> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn token_theme() -> &'static SyntectTheme {
    TOKEN_THEME.get_or_init(build_dark_plus_tokens)
}

/// Dark+-flavored token colors over the default TextMate grammars.
fn build_dark_plus_tokens() -> SyntectTheme {
    fn rgb(r: u8, g: u8, b: u8) -> SyntectColor {
        SyntectColor { r, g, b, a: 0xFF }
    }
    fn item(selector: &str, color: SyntectColor, italic: bool) -> Option<ThemeItem> {
        Some(ThemeItem {
            scope: ScopeSelectors::from_str(selector).ok()?,
            style: StyleModifier {
                foreground: Some(color),
                background: None,
                font_style: Some(if italic {
                    FontStyle::ITALIC
                } else {
                    FontStyle::empty()
                }),
            },
        })
    }

    let scopes = [
        ("comment", rgb(0x6A, 0x99, 0x55), true),
        ("string", rgb(0xCE, 0x91, 0x78), false),
        ("constant.numeric", rgb(0xB5, 0xCE, 0xA8), false),
        ("constant.language", rgb(0x56, 0x9C, 0xD6), false),
        ("keyword", rgb(0x56, 0x9C, 0xD6), false),
        ("storage", rgb(0x56, 0x9C, 0xD6), false),
        ("entity.name.function", rgb(0xDC, 0xDC, 0xAA), false),
        ("entity.name.type", rgb(0x4E, 0xC9, 0xB0), false),
        ("support.type", rgb(0x4E, 0xC9, 0xB0), false),
        // Markdown / prose (VS Code Dark+ markup colors). Without these,
        // every Markdown token fell back to the base foreground and .md
        // files rendered colorless (owner review round 2026-10-15).
        ("markup.heading", rgb(0x56, 0x9C, 0xD6), false),
        ("markup.quote", rgb(0x6A, 0x99, 0x55), false),
        ("markup.raw", rgb(0xCE, 0x91, 0x78), false),
    ];

    SyntectTheme {
        name: Some("editormini-dark-plus".to_string()),
        author: Some("editormini".to_string()),
        settings: ThemeSettings {
            foreground: Some(rgb(0xD4, 0xD4, 0xD4)),
            background: Some(rgb(0x1E, 0x1E, 0x1E)),
            ..ThemeSettings::default()
        },
        scopes: scopes
            .iter()
            .filter_map(|(selector, color, italic)| item(selector, *color, *italic))
            .collect(),
    }
}

/// Extension of a document path, lowercased, when present.
pub fn extension_of(path: Option<&Path>) -> Option<String> {
    path?
        .extension()?
        .to_str()
        .map(|ext| ext.to_ascii_lowercase())
}

/// Return the syntax language name for a file extension, or `None`
/// for unknown extensions (caller should map to the i18n `PlainText` key).
pub fn language_name(ext: Option<&str>) -> Option<&'static str> {
    ext.and_then(syntax_for_extension).map(|s| s.name.as_str())
}

fn syntax_for_extension(ext: &str) -> Option<&'static SyntaxReference> {
    /// Extensions absent from syntect's default set map to their closest
    /// structural cousin until the external-grammar loader (docs/plan.md)
    /// lands. `ps1` has no close cousin and stays plain for now.
    const FALLBACKS: &[(&str, &str)] =
        &[("toml", "yml"), ("ts", "js"), ("tsx", "js"), ("jsx", "js")];
    syntax_set().find_syntax_by_extension(ext).or_else(|| {
        FALLBACKS
            .iter()
            .find(|(missing, _)| *missing == ext)
            .and_then(|(_, fallback)| syntax_set().find_syntax_by_extension(fallback))
    })
}

fn to_ratatui_style(style: syntect::highlighting::Style, bg: Color) -> Style {
    let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
    let mut out = Style::new().fg(fg).bg(bg);
    if style.font_style.contains(FontStyle::ITALIC) {
        out = out.add_modifier(Modifier::ITALIC);
    }
    out
}

/// Highlight the visible lines into owned ratatui lines. Unknown extensions
/// render plain with the theme foreground; every span carries the theme
/// background so the editor block stays uniform.
pub fn styled_lines(
    lines: &[&str],
    extension: Option<&str>,
    fg: Color,
    bg: Color,
) -> Vec<Line<'static>> {
    let plain = Style::new().fg(fg).bg(bg);
    let Some(syntax) = extension.and_then(syntax_for_extension) else {
        return lines
            .iter()
            .map(|line| Line::from(vec![Span::styled((*line).to_string(), plain)]))
            .collect();
    };

    // The dumped grammars expect the trailing newline on each line.
    lines
        .iter()
        .map(|line| {
            let mut highlighter = HighlightLines::new(syntax, token_theme());
            let owned = format!("{line}\n");
            let spans: Vec<Span> = match highlighter.highlight_line(&owned, syntax_set()) {
                Ok(regions) => regions
                    .iter()
                    .filter_map(|(style, segment)| {
                        let text = segment.trim_end_matches('\n');
                        if text.is_empty() {
                            return None;
                        }
                        Some(Span::styled(text.to_string(), to_ratatui_style(*style, bg)))
                    })
                    .collect(),
                Err(_) => vec![Span::styled((*line).to_string(), plain)],
            };
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_extensions_resolve_to_syntaxes() {
        for ext in ["rs", "md", "json", "go", "py", "toml", "html", "ts", "jsx"] {
            assert!(
                syntax_for_extension(ext).is_some(),
                "extension {ext} must resolve to a grammar (direct or fallback)"
            );
        }
    }

    #[test]
    fn toml_falls_back_to_yaml_grammar() {
        let name = syntax_for_extension("toml").map(|s| s.name.clone());
        assert_eq!(name.as_deref(), Some("YAML"));
    }

    #[test]
    fn typescript_falls_back_to_javascript_grammar() {
        let name = syntax_for_extension("ts").map(|s| s.name.clone());
        assert_eq!(name.as_deref(), Some("JavaScript"));
    }

    #[test]
    fn powershell_has_no_grammar_yet_and_renders_plain() {
        assert!(syntax_for_extension("ps1").is_none());
    }

    #[test]
    fn unknown_extension_has_no_grammar() {
        assert!(syntax_for_extension("xyz").is_none());
    }

    #[test]
    fn extension_of_lowercases_and_reads_path() {
        assert_eq!(
            extension_of(Some(Path::new("src/Main.RS"))).as_deref(),
            Some("rs")
        );
        assert_eq!(extension_of(Some(Path::new("no-ext"))), None);
        assert_eq!(extension_of(None), None);
    }

    #[test]
    fn rust_line_produces_styled_spans() {
        let lines = ["fn main() {}"];
        let out = styled_lines(
            &lines,
            Some("rs"),
            Color::Rgb(0xD4, 0xD4, 0xD4),
            Color::Black,
        );
        assert_eq!(out.len(), 1);
        assert!(
            out[0].spans.iter().any(|span| {
                span.style
                    .fg
                    .is_some_and(|c| c != Color::Rgb(0xD4, 0xD4, 0xD4))
            }),
            "at least one token (e.g. keyword fn) must differ from base fg"
        );
    }

    #[test]
    fn unknown_extension_renders_plain_with_theme_fg() {
        let lines = ["just text"];
        let out = styled_lines(
            &lines,
            Some("xyz"),
            Color::Rgb(0xAA, 0xAA, 0xAA),
            Color::Black,
        );
        assert_eq!(out.len(), 1);
        assert!(
            out[0]
                .spans
                .iter()
                .all(|span| span.style.fg == Some(Color::Rgb(0xAA, 0xAA, 0xAA)))
        );
    }

    #[test]
    fn every_span_carries_the_theme_background() {
        let lines = ["let x = 1;"];
        let out = styled_lines(
            &lines,
            Some("rs"),
            Color::Rgb(0xD4, 0xD4, 0xD4),
            Color::Black,
        );
        assert!(
            out[0]
                .spans
                .iter()
                .all(|span| span.style.bg == Some(Color::Black))
        );
    }

    #[test]
    fn markdown_markup_scopes_get_dark_plus_colors() {
        // Owner review round: .md rendered colorless — the curated Dark+ map
        // had no markup.* scopes, so every Markdown token fell back to base
        // foreground. Dark+ reference colors: heading blue, quote green,
        // inline code orange.
        let lines = ["# Title", "> quoted", "a `code` span"];
        let out = styled_lines(
            &lines,
            Some("md"),
            Color::Rgb(0xD4, 0xD4, 0xD4),
            Color::Black,
        );
        assert!(
            out[0]
                .spans
                .iter()
                .any(|span| span.style.fg == Some(Color::Rgb(0x56, 0x9C, 0xD6))),
            "heading line must carry the markup.heading blue"
        );
        assert!(
            out[1]
                .spans
                .iter()
                .any(|span| span.style.fg == Some(Color::Rgb(0x6A, 0x99, 0x55))),
            "block quote must carry the markup.quote green"
        );
        assert!(
            out[2]
                .spans
                .iter()
                .any(|span| span.style.fg == Some(Color::Rgb(0xCE, 0x91, 0x78))),
            "inline code must carry the markup.raw orange"
        );
    }
}
