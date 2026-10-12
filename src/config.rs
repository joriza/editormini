//! Configuration loading.
//!
//! `editormini.json` lives beside the executable (portable-first: copy the exe,
//! copy the config). In dev builds (`cargo run`) that resolves to
//! `target/debug/`, so drop a config there to exercise non-default settings.
//! Absent file means defaults; malformed file is a hard error
//! so silent misconfiguration never happens. Unknown fields are ignored for
//! forward compatibility.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::i18n::Lang;

/// UI theme selection. `DarkPlus` is the only fully realized theme in
/// milestone 1; `LightPlus` is the stub proving the theme abstraction is
/// ready for a light/dark switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeChoice {
    #[default]
    DarkPlus,
    LightPlus,
}

/// Reserved export pipeline configuration (inert in milestone 1).
///
/// Both the PDF generator executable and the CSS template are EXTERNAL files
/// referenced here; the editor only orchestrates them (future milestone).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportConfig {
    /// External command used to render Markdown to PDF (future milestone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown_pdf_command: Option<String>,
    /// External CSS template file for the Markdown PDF export (future milestone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown_pdf_css: Option<String>,
    /// External typst compiler command (future milestone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typst_command: Option<String>,
}

/// Top-level configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Config {
    /// Interface language. Default English, switchable to Spanish.
    #[serde(default)]
    pub language: Lang,
    /// UI theme. Default VS Code Dark+.
    #[serde(default)]
    pub theme: ThemeChoice,
    /// Reserved export section (inert in milestone 1).
    #[serde(default)]
    pub export: ExportConfig,
}

/// Where the config file is expected: beside the executable.
pub fn config_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("editormini.json")))
}

/// Load the config, falling back to defaults when no file exists.
pub fn load() -> Result<Config, String> {
    let Some(path) = config_path() else {
        return Ok(Config::default());
    };
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("invalid {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_serialize_and_round_trip() {
        let json = serde_json::to_string(&Config::default()).expect("serialize defaults");
        let parsed: Config = serde_json::from_str(&json).expect("parse defaults");
        assert_eq!(parsed, Config::default());
        // Defaults are explicit in the file, not hidden.
        assert!(json.contains("dark-plus"));
        assert!(json.contains("en"));
    }

    #[test]
    fn parses_spanish_config_with_unknown_fields() {
        let raw = r#"{
            "language": "es",
            "theme": "dark-plus",
            "export": { "markdown_pdf_css": "template.css" },
            "some_future_field": true
        }"#;
        let parsed: Config = serde_json::from_str(raw).expect("parse spanish config");
        assert_eq!(parsed.language, Lang::Es);
        assert_eq!(
            parsed.export.markdown_pdf_css.as_deref(),
            Some("template.css")
        );
    }

    #[test]
    fn missing_file_means_defaults() {
        // load() only reads beside the exe; in tests there is no config file
        // next to the test binary, so defaults must come back cleanly.
        let cfg = load().expect("load falls back to defaults");
        assert_eq!(cfg, Config::default());
    }
}
