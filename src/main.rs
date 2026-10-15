//! editormini — a VS Code-like TUI text editor for the terminal.
//!
//! Milestone 1 slice: document model + TUI shell (tabs bar, editing, save,
//! quit). Syntax highlighting, undo, and mouse support arrive next.

mod app;
mod command;
mod config;
mod document;
mod fuzzy;
mod highlight;
mod i18n;
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
    let config = match config::load() {
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

    let docs = match load_documents(&cli.files) {
        Ok(docs) => docs,
        Err(e) => {
            eprintln!("editormini: {e}");
            std::process::exit(1);
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
    if let Err(e) = result {
        eprintln!("editormini: {e}");
        std::process::exit(1);
    }
}
