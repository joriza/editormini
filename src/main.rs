//! editormini — a VS Code-like TUI text editor for the terminal.
//!
//! Milestone 1 slice: document model + TUI shell (tabs bar, editing, save,
//! quit). Syntax highlighting, undo, and mouse support arrive next.

mod app;
mod command;
mod config;
mod document;
mod explorer;
mod fuzzy;
mod highlight;
mod i18n;
mod languagedetect;
mod pane;
mod theme;

use clap::Parser;
use std::io;
use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

/// editormini — a VS Code-like TUI text editor.
#[derive(Parser)]
#[command(
    name = "editormini",
    version,
    about = i18n::t(i18n::Lang::En, i18n::Key::About)
)]
struct Cli {
    /// Files to open as tabs (created on save if they do not exist)
    files: Vec<PathBuf>,
    /// Override the UI language from the config file
    #[arg(short, long)]
    lang: Option<i18n::Lang>,
}

fn load_documents(files: &[PathBuf]) -> Result<Vec<document::Document>, String> {
    if files.is_empty() {
        return Ok(vec![document::Document::new_untitled()]);
    }
    let mut docs = Vec::with_capacity(files.len());
    for file in files {
        let doc = if file.exists() {
            document::Document::from_path(file).map_err(|e| format!("{}: {e}", file.display()))?
        } else {
            document::Document::new_with_path(file)
        };
        docs.push(doc);
    }
    Ok(docs)
}

/// Open every path in `config.open_files`, falling back to a single untitled
/// tab when the list is empty. Paths that no longer exist are skipped so a
/// deleted file never blocks startup.
fn load_restored_documents(config: &config::Config) -> Vec<document::Document> {
    let mut docs = Vec::new();
    for path in &config.open_files {
        if path.exists() {
            match document::Document::from_path(path) {
                Ok(doc) => docs.push(doc),
                Err(e) => eprintln!("editormini: skipping {}: {e}", path.display()),
            }
        } else {
            eprintln!("editormini: skipping missing {}", path.display());
        }
    }
    if docs.is_empty() {
        docs.push(document::Document::new_untitled());
    }
    docs
}

/// Persist the paths of every open document so the next launch restores
/// them. Untitled tabs (no path) are skipped. When `path` is `None` the
/// in-memory `open_files` list is still updated but nothing is written to
/// disk (used by tests, where `config_path()` would point at the test
/// binary's own directory).
fn save_session(app: &app::App, config: &mut config::Config, path: Option<&PathBuf>) {
    config.open_files = app
        .docs
        .iter()
        .filter_map(|d| d.path().map(|p| p.to_path_buf()))
        .collect();
    let Some(path) = path else {
        return;
    };
    let json = match serde_json::to_string_pretty(config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("editormini: cannot serialize session: {e}");
            return;
        }
    };
    if let Err(e) = std::fs::write(path, json) {
        eprintln!("editormini: cannot write {}: {e}", path.display());
    }
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(mut terminal: Terminal<CrosstermBackend<io::Stdout>>) {
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();
}

fn main() {
    let cli = Cli::parse();
    let mut config = match config::load() {
        Ok(config) => config,
        Err(e) => {
            // The config is unusable, so its language field is unavailable;
            // the CLI override (or the English default) still applies.
            let lang = cli.lang.unwrap_or_default();
            eprintln!(
                "editormini: {} ({e})",
                i18n::t(lang, i18n::Key::ConfigLoadError)
            );
            std::process::exit(1);
        }
    };
    let lang = cli.lang.unwrap_or(config.language);

    // CLI paths win; otherwise restore whatever was open on the last run.
    let docs = if cli.files.is_empty() {
        load_restored_documents(&config)
    } else {
        match load_documents(&cli.files) {
            Ok(docs) => docs,
            Err(e) => {
                eprintln!("editormini: {e}");
                std::process::exit(1);
            }
        }
    };

    let mut terminal = match setup_terminal() {
        Ok(t) => t,
        Err(_) => {
            eprintln!("editormini: {}", i18n::t(lang, i18n::Key::TerminalRequired));
            std::process::exit(2);
        }
    };

    let mut app = app::App::new(docs, lang);
    app.set_theme(config.theme);
    let result = app::run(&mut terminal, &mut app);
    restore_terminal(terminal);
    // Persist the session even on a runtime error — the user's tabs are
    // already in memory and worth restoring on the next launch.
    save_session(&app, &mut config, config::config_path().as_ref());
    if let Err(e) = result {
        eprintln!("editormini: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::Config;
    use crate::document::Document;
    use crate::i18n::Lang;
    use std::fs;

    /// End-to-end session round-trip: write a config with open_files, save
    /// a session over it, reload, and confirm the paths survive.
    #[test]
    fn session_persists_open_paths_across_runs() {
        let dir = std::env::temp_dir().join(format!("editormini_session_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file_a = dir.join("a.txt");
        let file_b = dir.join("b.txt");
        fs::write(&file_a, "a").unwrap();
        fs::write(&file_b, "b").unwrap();

        let mut config = Config::default();
        config.open_files = vec![file_a.clone(), file_b.clone()];
        let json = serde_json::to_string_pretty(&config).unwrap();
        let cfg_path = dir.join("editormini.json");
        fs::write(&cfg_path, json).unwrap();

        // Reload from disk — the open_files key must survive.
        let raw = fs::read_to_string(&cfg_path).unwrap();
        let reloaded: Config = serde_json::from_str(&raw).unwrap();
        assert_eq!(reloaded.open_files, vec![file_a.clone(), file_b.clone()]);

        // Simulate the editor: build docs with those paths, save the
        // session, and confirm the file now records both paths.
        let docs = load_restored_documents(&reloaded);
        assert_eq!(docs.len(), 2);
        let app = app::App::new(docs, Lang::En);
        // Drop the untitled fallback doc that load_restored_documents would
        // have added when the list was empty — here we have real paths.
        save_session(&app, &mut config, None);
        assert_eq!(config.open_files, vec![file_a.clone(), file_b.clone()]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn restored_launch_skips_missing_files_and_keeps_a_tab() {
        let dir =
            std::env::temp_dir().join(format!("editormini_restore_missing_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let gone = dir.join("deleted.txt");
        let alive = dir.join("alive.txt");
        fs::write(&alive, "x").unwrap();

        let mut config = Config::default();
        config.open_files = vec![gone.clone(), alive.clone()];
        let docs = load_restored_documents(&config);
        // The missing file is skipped; the live one opens. No untitled tab
        // is added because at least one real doc survived.
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].path(), Some(alive.as_path()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_session_list_yields_one_untitled_tab() {
        let config = Config::default();
        let docs = load_restored_documents(&config);
        assert_eq!(docs.len(), 1);
        assert!(docs[0].path().is_none());
    }
}
