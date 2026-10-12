//! UI chrome themes.
//!
//! The chrome palette covers everything around the text (tabs, status bar,
//! prompts). Syntax token colors live in `highlight.rs` and stay Dark+-based
//! until the light-mode milestone introduces a matching syntax theme.

use crate::config::ThemeChoice;
use ratatui::style::Color;

/// Chrome palette: tabs, status bar, editor background and prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub tab_active_bg: Color,
    pub tab_inactive_bg: Color,
    pub accent: Color,
    pub modified: Color,
    pub prompt_bg: Color,
    pub prompt_fg: Color,
}

/// VS Code Dark+ chrome palette (milestone default).
pub const DARK_PLUS: Theme = Theme {
    bg: Color::Rgb(0x1E, 0x1E, 0x1E),
    fg: Color::Rgb(0xD4, 0xD4, 0xD4),
    tab_active_bg: Color::Rgb(0x09, 0x47, 0x7B),
    tab_inactive_bg: Color::Rgb(0x2D, 0x2D, 0x2D),
    accent: Color::Rgb(0x00, 0x7A, 0xCC),
    modified: Color::Rgb(0xE2, 0xC0, 0x89),
    prompt_bg: Color::Rgb(0xE2, 0xC0, 0x89),
    prompt_fg: Color::Black,
};

/// Light+ placeholder: proves the theme-switch path end to end; the real
/// light palette lands with the configuration milestone (plan.md, hito 4).
pub const LIGHT_PLUS_STUB: Theme = Theme {
    bg: Color::Rgb(0xFF, 0xFF, 0xFF),
    fg: Color::Rgb(0x33, 0x33, 0x33),
    tab_active_bg: Color::Rgb(0x00, 0x7A, 0xCC),
    tab_inactive_bg: Color::Rgb(0xDD, 0xDD, 0xDD),
    accent: Color::Rgb(0x00, 0x7A, 0xCC),
    modified: Color::Rgb(0xB3, 0x5C, 0x00),
    prompt_bg: Color::Rgb(0xFF, 0xF8, 0xDC),
    prompt_fg: Color::Black,
};

impl Theme {
    pub fn for_choice(choice: ThemeChoice) -> Self {
        match choice {
            ThemeChoice::DarkPlus => DARK_PLUS,
            ThemeChoice::LightPlus => LIGHT_PLUS_STUB,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_plus_palette_is_vs_code_standard() {
        assert_eq!(DARK_PLUS.bg, Color::Rgb(0x1E, 0x1E, 0x1E));
        assert_eq!(DARK_PLUS.accent, Color::Rgb(0x00, 0x7A, 0xCC));
        assert_ne!(DARK_PLUS.tab_active_bg, DARK_PLUS.tab_inactive_bg);
        assert_ne!(DARK_PLUS.bg, DARK_PLUS.fg);
    }

    #[test]
    fn light_stub_differs_from_dark() {
        assert_ne!(LIGHT_PLUS_STUB.bg, DARK_PLUS.bg);
        assert_ne!(LIGHT_PLUS_STUB.fg, DARK_PLUS.fg);
    }

    #[test]
    fn for_choice_resolves_both_themes() {
        assert_eq!(Theme::for_choice(ThemeChoice::DarkPlus), DARK_PLUS);
        assert_eq!(Theme::for_choice(ThemeChoice::LightPlus), LIGHT_PLUS_STUB);
    }
}
