//! UI message catalog.
//!
//! Every user-visible string routes through [`t`] so the interface language can
//! switch between English (default) and Spanish, mirroring VS Code's
//! "Configure Display Language" behavior. The catalog stays table-driven and
//! dependency-free on purpose.
//!
//! ## Missing-key visibility in dev builds
//!
//! `t(lang, key)` asserts in `debug_assert!` that the returned string is
//! non-empty.  In debug builds a missing or empty translation panics with a
//! clear message; in release builds the assertion is compiled out (zero cost).
//!
//! The exhaustive `match` in `t` already guarantees a **compile error** if a
//! `Key` variant is missing from any language arm — that is the primary
//! compile-time safety net.

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
    /// Read-only document indicator in the status bar.
    ReadOnly,
    /// I/O error prefix shown in the status bar.
    IoError,
    /// File is not valid UTF-8.
    NotUtf8,
    /// Position out of bounds.
    OutOfBounds,
    /// CLI `--help` about text.
    About,
    /// Overwrite-mode indicator in the status bar.
    OverwriteIndicator,
    /// Fallback language name for files with no recognized syntax.
    PlainText,
    /// No matches result.
    NoMatches,
    /// Match counter template: "{current}/{total}".
    MatchCounter,
    /// Key hint: Enter to find next match.
    FindNext,
    /// Key hint: Shift+Enter to find previous match.
    FindPrev,
    /// Key hint: Esc to close the search dialog.
    FindClose,
    /// Search dialog title.
    FindDialogTitle,
    /// Search options placeholder (future case/regex toggles).
    FindOptions,
    /// Replace dialog title.
    ReplaceDialogTitle,
    /// Key hint: Ctrl+Enter to replace current match.
    ReplaceNext,
    /// Key hint: F5 to replace (fallback).
    ReplaceFallback,
    /// Key hint: F6 to replace all.
    ReplaceAll,
    /// Key hint: Tab to switch field.
    ReplaceSwitchField,
    /// Key hint: Esc to close the replace dialog.
    ReplaceClose,
    /// Label for the query field in the replace dialog.
    ReplaceFieldQuery,
    /// Label for the replacement field in the replace dialog.
    ReplaceFieldReplacement,
    /// Replace-all confirmation title.
    ReplaceConfirmTitle,
    /// Replace-all confirmation message template: "{count}".
    ReplaceConfirmMessage,
    /// Status message after replace-all: "Replaced {count}".
    ReplacedCount,
    /// Status when no matches exist for replace.
    NoMatchesReplace,
    /// Replace-all confirmation for cross-doc: "Replace {count} occurrence(s) in {docs} documents?"
    ReplaceConfirmMessageCross,
    /// Match counter for cross-doc: "{current}/{total} ({doc})".
    MatchCounterCross,
    /// Status suffix for skipped read-only docs in cross-doc replace-all.
    SkippedReadOnly,
}

/// Translate `key` for `lang`.
pub fn t(lang: Lang, key: Key) -> &'static str {
    let msg = match (lang, key) {
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
            "F2/Ctrl+S save \u{b7} F3/Ctrl+Shift+S save all \u{b7} Ctrl+O open \u{b7} Ctrl+Q quit \u{b7} Ins overwrite \u{b7} Ctrl+PgUp/PgDn switch tab"
        }
        (Lang::Es, Key::StartupHints) => {
            "F2/Ctrl+S guardar \u{b7} F3/Ctrl+Shift+S guardar todo \u{b7} Ctrl+O abrir \u{b7} Ctrl+Q salir \u{b7} Ins sobrescribir \u{b7} Ctrl+PgUp/PgDn cambiar pesta\u{f1}a"
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
        // Error kinds mapped from DocumentError for status-bar display.
        (Lang::En, Key::ReadOnly) => "read-only",
        (Lang::Es, Key::ReadOnly) => "solo lectura",
        (Lang::En, Key::IoError) => "I/O error",
        (Lang::Es, Key::IoError) => "error de E/S",
        (Lang::En, Key::NotUtf8) => "file is not valid UTF-8",
        (Lang::Es, Key::NotUtf8) => "archivo no es UTF-8 válido",
        (Lang::En, Key::OutOfBounds) => "position {line}:{col} out of bounds",
        (Lang::Es, Key::OutOfBounds) => "posición {line}:{col} fuera de rango",
        // CLI help / about text.
        (Lang::En, Key::About) => {
            "A VS Code-like TUI text editor\n\nKeys: F2 or Ctrl+S save \u{b7} Ctrl+Z undo \u{b7} Ctrl+Q quit \u{b7} Ins overwrite \u{b7} Ctrl+PgUp/PgDn switch tab"
        }
        (Lang::Es, Key::About) => {
            "Un editor de texto TUI estilo VS Code\n\nTeclas: F2 o Ctrl+S guardar \u{b7} Ctrl+Z deshacer \u{b7} Ctrl+Q salir \u{b7} Ins sobrescribir \u{b7} Ctrl+PgUp/PgDn cambiar pesta\u{f1}a"
        }
        // Overwrite-mode compact indicator in the status bar.
        (Lang::En, Key::OverwriteIndicator) => "OVR ",
        (Lang::Es, Key::OverwriteIndicator) => "SOB ",
        // Fallback language name for files with no recognized syntax.
        (Lang::En, Key::PlainText) => "Plain text",
        (Lang::Es, Key::PlainText) => "Texto sin formato",
        // Search dialog.
        (Lang::En, Key::NoMatches) => "No matches",
        (Lang::Es, Key::NoMatches) => "Sin coincidencias",
        (Lang::En, Key::MatchCounter) => "{current}/{total}",
        (Lang::Es, Key::MatchCounter) => "{current}/{total}",
        (Lang::En, Key::FindNext) => "Enter: next",
        (Lang::Es, Key::FindNext) => "Enter: siguiente",
        (Lang::En, Key::FindPrev) => "Shift+Enter: prev",
        (Lang::Es, Key::FindPrev) => "Shift+Enter: anterior",
        (Lang::En, Key::FindClose) => "Esc: close",
        (Lang::Es, Key::FindClose) => "Esc: cerrar",
        // Search dialog title and options placeholder.
        (Lang::En, Key::FindDialogTitle) => "Find",
        (Lang::Es, Key::FindDialogTitle) => "Buscar",
        (Lang::En, Key::FindOptions) => "options: case/regex",
        (Lang::Es, Key::FindOptions) => "opciones: mayúsculas/regex",
        // Replace dialog.
        (Lang::En, Key::ReplaceDialogTitle) => "Replace",
        (Lang::Es, Key::ReplaceDialogTitle) => "Reemplazar",
        (Lang::En, Key::ReplaceNext) => "Ctrl+Enter: replace",
        (Lang::Es, Key::ReplaceNext) => "Ctrl+Enter: reemplazar",
        (Lang::En, Key::ReplaceFallback) => "F5: replace",
        (Lang::Es, Key::ReplaceFallback) => "F5: reemplazar",
        (Lang::En, Key::ReplaceAll) => "F6: replace all",
        (Lang::Es, Key::ReplaceAll) => "F6: reemplazar todo",
        (Lang::En, Key::ReplaceSwitchField) => "Tab: field",
        (Lang::Es, Key::ReplaceSwitchField) => "Tab: campo",
        (Lang::En, Key::ReplaceClose) => "Esc: close",
        (Lang::Es, Key::ReplaceClose) => "Esc: cerrar",
        (Lang::En, Key::ReplaceFieldQuery) => "Query:",
        (Lang::Es, Key::ReplaceFieldQuery) => "Consulta:",
        (Lang::En, Key::ReplaceFieldReplacement) => "Replace with:",
        (Lang::Es, Key::ReplaceFieldReplacement) => "Reemplazar con:",
        (Lang::En, Key::ReplaceConfirmTitle) => "Replace all?",
        (Lang::Es, Key::ReplaceConfirmTitle) => "¿Reemplazar todo?",
        (Lang::En, Key::ReplaceConfirmMessage) => "Replace {count} occurrence(s)?",
        (Lang::Es, Key::ReplaceConfirmMessage) => "¿Reemplazar {count} ocurrencia(s)?",
        (Lang::En, Key::ReplacedCount) => "Replaced {count}",
        (Lang::Es, Key::ReplacedCount) => "Reemplazado(s) {count}",
        (Lang::En, Key::NoMatchesReplace) => "No matches to replace",
        (Lang::Es, Key::NoMatchesReplace) => "Sin coincidencias para reemplazar",
        // Cross-doc replace-all confirm message.
        (Lang::En, Key::ReplaceConfirmMessageCross) => {
            "Replace {count} occurrence(s) in {docs} documents?"
        }
        (Lang::Es, Key::ReplaceConfirmMessageCross) => {
            "¿Reemplazar {count} ocurrencia(s) en {docs} documentos?"
        }
        // Cross-doc match counter showing doc title.
        (Lang::En, Key::MatchCounterCross) => "{current}/{total} ({doc})",
        (Lang::Es, Key::MatchCounterCross) => "{current}/{total} ({doc})",
        // Status suffix for read-only docs skipped in cross-doc replace-all.
        (Lang::En, Key::SkippedReadOnly) => "read-only doc(s) skipped",
        (Lang::Es, Key::SkippedReadOnly) => "documento(s) solo lectura omitido(s)",
    };
    debug_assert!(!msg.is_empty(), "missing translation for {lang:?}/{key:?}");
    msg
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

/// Return an i18n'd status-bar message for a [`DocumentError`].
///
/// Maps each error kind to a catalog key so the user sees a
/// translated message instead of raw English `DocumentError` text.
pub fn error_message(lang: Lang, error: &crate::document::DocumentError) -> String {
    match error {
        crate::document::DocumentError::Io(e) => {
            format!("{}: {e}", t(lang, Key::IoError))
        }
        crate::document::DocumentError::NotUtf8 => t(lang, Key::NotUtf8).to_string(),
        crate::document::DocumentError::Untitled => t(lang, Key::Untitled).to_string(),
        crate::document::DocumentError::ReadOnly => t(lang, Key::ReadOnly).to_string(),
        crate::document::DocumentError::OutOfBounds { line, col } => tr(
            lang,
            Key::OutOfBounds,
            &[("line", &line.to_string()), ("col", &col.to_string())],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_KEYS: &[Key] = &[
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
        Key::ReadOnly,
        Key::IoError,
        Key::NotUtf8,
        Key::OutOfBounds,
        Key::About,
        Key::OverwriteIndicator,
        Key::PlainText,
        Key::ReplaceFieldQuery,
        Key::ReplaceFieldReplacement,
        Key::ReplaceConfirmTitle,
        Key::ReplaceConfirmMessage,
        Key::ReplaceConfirmMessageCross,
        Key::MatchCounterCross,
        Key::SkippedReadOnly,
    ];

    #[test]
    fn every_key_resolves_in_every_language() {
        for &key in ALL_KEYS {
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

    #[test]
    fn startup_hints_omit_obvious_keys() {
        // Owner: undo is as obvious as find, which was never listed — both
        // stay out of the always-on hint bar to keep it readable.
        assert!(
            !t(Lang::En, Key::StartupHints).contains("undo"),
            "startup hints must not list undo"
        );
        assert!(
            !t(Lang::Es, Key::StartupHints).contains("deshacer"),
            "startup hints must not list deshacer"
        );
    }
}
