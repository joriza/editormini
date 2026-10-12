//! editormini — a VS Code-like TUI text editor for the terminal.
//!
//! Milestone 1 scaffold: CLI parsing, config loading, and the i18n catalog.
//! The TUI lands at milestone task 4.

mod config;
mod i18n;

use clap::Parser;
use std::path::PathBuf;

/// editormini — a VS Code-like TUI text editor.
#[derive(Parser)]
#[command(name = "editormini", version, about)]
struct Cli {
    /// Files to open as tabs
    files: Vec<PathBuf>,
    /// Override the UI language from the config file
    #[arg(short, long)]
    lang: Option<i18n::Lang>,
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

    // The TUI arrives at milestone task 4; until then the binary verifies the
    // config + i18n wiring end to end.
    println!("{}", i18n::t(lang, i18n::Key::ScaffoldNotice));
}
