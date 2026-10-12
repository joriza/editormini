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
    /// Config file could not be loaded.
    ConfigLoadError,
    /// Placeholder name for documents with no file path.
    Untitled,
    /// Quit confirmation shown when unsaved changes exist.
    SaveChangesPrompt,
    /// stdout is not a terminal (piped or redirected).
    TerminalRequired,
    /// Active document saved successfully.
    Saved,
    /// Cursor position template in the status bar.
    LnCol,
    /// Startup keymap hint shown in the status bar.
    StartupHints,
    /// Overwrite mode enabled (Insert key).
    OverwriteOn,
    /// Overwrite mode disabled (Insert key).
    OverwriteOff,
    /// Open-file prompt title.
    OpenFile,
    /// Save-as prompt title.
    SaveAs,
    /// Status: all modified files saved.
    SavedAll,
    /// Status: no modified files with paths to save.
    NoModifiedFiles,
    /// Status: file opened as a new tab.
    FileOpened,
    /// Status: save-as succeeded.
    SavedAs,
}

/// Translate `key` for `lang`.
pub fn t(lang: Lang, key: Key) -> &'static str {
    match (lang, key) {
        (Lang::En, Key::ConfigLoadError) => "Failed to load editormini.json",
        (Lang::Es, Key::ConfigLoadError) => "No se pudo cargar editormini.json",
        (Lang::En, Key::Untitled) => "[Untitled]",
        (Lang::Es, Key::Untitled) => "[Sin título]",
        (Lang::En, Key::SaveChangesPrompt) => {
            "Save changes before closing? [Y]es / [N]o / [C]ancel"
        }
        (Lang::Es, Key::SaveChangesPrompt) => {
            "¿Guardar los cambios antes de cerrar? [S]í / [N]o / [C]ancelar"
        }
        (Lang::En, Key::TerminalRequired) => {
            "editormini needs a real terminal (TTY). Run it inside Windows Terminal."
        }
        (Lang::Es, Key::TerminalRequired) => {
            "editormini necesita una terminal real (TTY). Ejecutalo dentro de Windows Terminal."
        }
        (Lang::En, Key::Saved) => "Saved",
        (Lang::Es, Key::Saved) => "Guardado",
        (Lang::En, Key::LnCol) => "Ln {ln}, Col {col}",
        (Lang::Es, Key::LnCol) => "Ln {ln}, Col {col}",
        (Lang::En, Key::StartupHints) => {
            "F2/Ctrl+S save \u{b7} F3/Ctrl+Shift+S save all \u{b7} Ctrl+O open \u{b7} Ctrl+Z undo \u{b7} Ctrl+Q quit \u{b7} Ins overwrite \u{b7} Ctrl+PgUp/PgDn switch tab"
        }
        (Lang::Es, Key::StartupHints) => {
            "F2/Ctrl+S guardar \u{b7} F3/Ctrl+Shift+S guardar todo \u{b7} Ctrl+O abrir \u{b7} Ctrl+Z deshacer \u{b7} Ctrl+Q salir \u{b7} Ins sobrescribir \u{b7} Ctrl+PgUp/PgDn cambiar pesta\u{f1}a"
        }
        (Lang::En, Key::OverwriteOn) => "Overwrite ON",
        (Lang::Es, Key::OverwriteOn) => "Sobrescribir ACTIVADO",
        (Lang::En, Key::OverwriteOff) => "Overwrite OFF",
        (Lang::Es, Key::OverwriteOff) => "Sobrescribir DESACTIVADO",
        (Lang::En, Key::OpenFile) => "Open file: ",
        (Lang::Es, Key::OpenFile) => "Abrir archivo: ",
        (Lang::En, Key::SaveAs) => "Save As: ",
        (Lang::Es, Key::SaveAs) => "Guardar como: ",
        (Lang::En, Key::SavedAll) => "All files saved",
        (Lang::Es, Key::SavedAll) => "Todos los archivos guardados",
        (Lang::En, Key::NoModifiedFiles) => "No modified files to save",
        (Lang::Es, Key::NoModifiedFiles) => "Sin archivos modificados para guardar",
        (Lang::En, Key::FileOpened) => "Opened {path}",
        (Lang::Es, Key::FileOpened) => "Abierto {path}",
        (Lang::En, Key::SavedAs) => "Saved {path}",
        (Lang::Es, Key::SavedAs) => "Guardado como {path}",
    }
}

/// Translate and substitute `{placeholder}` values, e.g.
/// `tr(lang, Key::LnCol, &[("ln", "3"), ("col", "7")])`.
pub fn tr(lang: Lang, key: Key, args: &[(&str, &str)]) -> String {
    let mut out = t(lang, key).to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_resolves_in_every_language() {
        for key in [
            Key::ConfigLoadError,
            Key::Untitled,
            Key::SaveChangesPrompt,
            Key::TerminalRequired,
            Key::Saved,
            Key::LnCol,
            Key::StartupHints,
            Key::OverwriteOn,
            Key::OverwriteOff,
            Key::OpenFile,
            Key::SaveAs,
            Key::SavedAll,
            Key::NoModifiedFiles,
            Key::FileOpened,
            Key::SavedAs,
        ] {
            for lang in [Lang::En, Lang::Es] {
                let msg = t(lang, key);
                assert!(!msg.is_empty(), "empty message for {lang:?}/{key:?}");
            }
        }
    }

    #[test]
    fn template_substitution_fills_placeholders() {
        let s = tr(Lang::En, Key::LnCol, &[("ln", "3"), ("col", "7")]);
        assert_eq!(s, "Ln 3, Col 7");
        let s = tr(Lang::Es, Key::LnCol, &[("ln", "1"), ("col", "1")]);
        assert_eq!(s, "Ln 1, Col 1");
    }

    #[test]
    fn save_prompt_offers_yes_in_both_languages() {
        // Spanish uses [S]í, English uses [Y]es; the app must accept both keys.
        assert!(t(Lang::En, Key::SaveChangesPrompt).contains("[Y]es"));
        assert!(t(Lang::Es, Key::SaveChangesPrompt).contains("[S]í"));
    }

    #[test]
    fn languages_are_distinct() {
        assert_ne!(t(Lang::En, Key::Untitled), t(Lang::Es, Key::Untitled));
    }
}
