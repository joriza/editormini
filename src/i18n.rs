//! UI message catalog.
//!
//! Every user-visible string routes through [`t`] so the interface language can
//! switch between English (default) and Spanish, mirroring VS Code's
//! "Configure Display Language" behavior. The catalog stays table-driven and
//! dependency-free on purpose.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Supported interface languages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    En,
    Es,
}

/// Catalog keys. One variant per user-visible string; add variants as features
/// grow. A missing `(lang, key)` arm is a compile error, which keeps both
/// languages complete by construction.
#[derive(Debug, Clone, Copy)]
pub enum Key {
    /// Scaffold notice printed until the TUI lands (milestone task 4).
    ScaffoldNotice,
    /// Config file could not be loaded.
    ConfigLoadError,
}

/// Translate `key` for `lang`.
pub fn t(lang: Lang, key: Key) -> &'static str {
    match (lang, key) {
        (Lang::En, Key::ScaffoldNotice) => {
            "editormini scaffold OK — the TUI arrives at milestone task 4."
        }
        (Lang::Es, Key::ScaffoldNotice) => {
            "andamiaje de editormini listo — la TUI llega en la tarea 4 del hito."
        }
        (Lang::En, Key::ConfigLoadError) => "Failed to load editormini.json",
        (Lang::Es, Key::ConfigLoadError) => "No se pudo cargar editormini.json",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_resolves_in_every_language() {
        for key in [Key::ScaffoldNotice, Key::ConfigLoadError] {
            for lang in [Lang::En, Lang::Es] {
                let msg = t(lang, key);
                assert!(!msg.is_empty(), "empty message for {lang:?}/{key:?}");
            }
        }
    }

    #[test]
    fn languages_are_distinct() {
        assert_ne!(
            t(Lang::En, Key::ScaffoldNotice),
            t(Lang::Es, Key::ScaffoldNotice)
        );
    }
}
