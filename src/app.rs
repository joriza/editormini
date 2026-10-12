//! Terminal application shell: event loop, drawing, and key handling.
//!
//! Slice scope (milestone task "TUI shell"): one pane from the pane tree,
//! tab bar (render + Ctrl+PgUp/PgDn switching), editor viewport with cursor,
//! Ctrl+S save, Ctrl+Q quit with unsaved-changes prompt. Syntax highlighting,
//! undo, mouse, and per-pane cursor state arrive in later tasks.

use std::io;

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, read};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::document::{Document, DocumentError};
use crate::i18n::{self, Key, Lang};
use crate::pane::PaneTree;

/// Dark+ palette subset used before the theme milestone formalizes it.
mod colors {
    use ratatui::style::Color;
    pub const BG: Color = Color::Rgb(0x1E, 0x1E, 0x1E);
    pub const FG: Color = Color::Rgb(0xD4, 0xD4, 0xD4);
    pub const TAB_INACTIVE_BG: Color = Color::Rgb(0x2D, 0x2D, 0x2D);
    pub const TAB_ACTIVE_BG: Color = Color::Rgb(0x09, 0x47, 0x7B);
    pub const ACCENT: Color = Color::Rgb(0x00, 0x7A, 0xCC);
    pub const MODIFIED: Color = Color::Rgb(0xE2, 0xC0, 0x89);
}

pub struct App {
    pub docs: Vec<Document>,
    pub tree: PaneTree,
    /// Index of the focused document (single pane in this slice).
    pub active: usize,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub scroll_top: usize,
    pub col_off: usize,
    pub viewport_h: usize,
    pub viewport_w: usize,
    pub quit: bool,
    pub confirm_quit: bool,
    pub status: String,
    pub lang: Lang,
}

impl App {
    pub fn new(docs: Vec<Document>, lang: Lang) -> Self {
        let tree = PaneTree::single(0);
        App {
            docs,
            tree,
            active: 0,
            cursor_line: 0,
            cursor_col: 0,
            scroll_top: 0,
            col_off: 0,
            viewport_h: 1,
            viewport_w: 1,
            quit: false,
            confirm_quit: false,
            status: String::new(),
            lang,
        }
    }

    pub fn doc(&self) -> &Document {
        &self.docs[self.active]
    }

    fn doc_mut(&mut self) -> &mut Document {
        &mut self.docs[self.active]
    }

    // -- Cursor / scroll invariants ---------------------------------------

    fn clamp_cursor(&mut self) {
        let last = self.doc().line_count().saturating_sub(1);
        self.cursor_line = self.cursor_line.min(last);
        self.cursor_col = self.cursor_col.min(self.doc().line_len(self.cursor_line));
    }

    fn clamp_scroll(&mut self) {
        let h = self.viewport_h.max(1);
        let w = self.viewport_w.max(1);
        if self.cursor_line < self.scroll_top {
            self.scroll_top = self.cursor_line;
        }
        if self.cursor_line >= self.scroll_top + h {
            self.scroll_top = self.cursor_line + 1 - h;
        }
        if self.cursor_col < self.col_off {
            self.col_off = self.cursor_col;
        }
        if self.cursor_col >= self.col_off + w {
            self.col_off = self.cursor_col + 1 - w;
        }
    }

    // -- Editing -----------------------------------------------------------

    fn insert_text(&mut self, text: &str) {
        let (line, col) = (self.cursor_line, self.cursor_col);
        match self.doc_mut().insert(line, col, text) {
            Ok(_) => {
                let parts: Vec<&str> = text.split('\n').collect();
                self.cursor_line += parts.len() - 1;
                self.cursor_col = parts.last().map(|s| s.chars().count()).unwrap_or(0);
            }
            Err(DocumentError::ReadOnly) => self.status = "read-only".to_string(),
            Err(e) => self.status = format!("{e}"),
        }
        self.clamp_cursor();
    }

    fn backspace(&mut self) {
        let (line, col) = (self.cursor_line, self.cursor_col);
        let result = if col > 0 {
            self.doc_mut().delete_range(line, col - 1, line, col)
        } else if line > 0 {
            let prev_len = self.doc().line_len(line - 1);
            let r = self.doc_mut().delete_range(line - 1, prev_len, line, 0);
            self.cursor_line -= 1;
            self.cursor_col = prev_len;
            r
        } else {
            return;
        };
        if let Err(e) = result {
            self.status = format!("{e}");
        }
        self.clamp_cursor();
    }

    fn delete_forward(&mut self) {
        let (line, col) = (self.cursor_line, self.cursor_col);
        let result = if col < self.doc().line_len(line) {
            self.doc_mut().delete_range(line, col, line, col + 1)
        } else if line + 1 < self.doc().line_count() {
            self.doc_mut().delete_range(line, col, line + 1, 0)
        } else {
            return;
        };
        if let Err(e) = result {
            self.status = format!("{e}");
        }
        self.clamp_cursor();
    }

    // -- Commands ----------------------------------------------------------

    fn save_active(&mut self) {
        match self.doc_mut().save() {
            Ok(bytes) => {
                self.status = format!("{} ({bytes} B)", i18n::t(self.lang, Key::Saved));
            }
            Err(e) => self.status = format!("{e}"),
        }
    }

    fn request_quit(&mut self) {
        if self.docs.iter().any(Document::is_modified) {
            self.confirm_quit = true;
        } else {
            self.quit = true;
        }
    }

    fn next_doc(&mut self, delta: isize) {
        let n = self.docs.len() as isize;
        self.active = ((self.active as isize + delta).rem_euclid(n)) as usize;
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.scroll_top = 0;
        self.col_off = 0;
        self.clamp_cursor();
        if let PaneTree::Single(pane) = &mut self.tree {
            pane.doc = self.active;
        }
    }

    // -- Events ------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        if self.confirm_quit {
            self.on_confirm_key(key.code);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            (KeyCode::Char('s'), true) => self.save_active(),
            (KeyCode::Char('q'), true) => self.request_quit(),
            (KeyCode::PageDown, true) => self.next_doc(1),
            (KeyCode::PageUp, true) => self.next_doc(-1),
            (KeyCode::Char(c), false) => self.insert_text(&c.to_string()),
            (KeyCode::Enter, _) => self.insert_text("\n"),
            (KeyCode::Backspace, _) => self.backspace(),
            (KeyCode::Delete, _) => self.delete_forward(),
            (KeyCode::Left, _) => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                } else if self.cursor_line > 0 {
                    self.cursor_line -= 1;
                    self.cursor_col = self.doc().line_len(self.cursor_line);
                }
                self.clamp_scroll();
            }
            (KeyCode::Right, _) => {
                if self.cursor_col < self.doc().line_len(self.cursor_line) {
                    self.cursor_col += 1;
                } else if self.cursor_line + 1 < self.doc().line_count() {
                    self.cursor_line += 1;
                    self.cursor_col = 0;
                }
                self.clamp_scroll();
            }
            (KeyCode::Up, _) => {
                self.cursor_line = self.cursor_line.saturating_sub(1);
                self.clamp_cursor();
                self.clamp_scroll();
            }
            (KeyCode::Down, _) => {
                self.cursor_line += 1;
                self.clamp_cursor();
                self.clamp_scroll();
            }
            (KeyCode::Home, _) => {
                self.cursor_col = 0;
                self.clamp_scroll();
            }
            (KeyCode::End, _) => {
                self.cursor_col = self.doc().line_len(self.cursor_line);
                self.clamp_scroll();
            }
            (KeyCode::PageUp, _) => {
                self.cursor_line = self
                    .cursor_line
                    .saturating_sub(self.viewport_h.saturating_sub(1));
                self.clamp_cursor();
                self.clamp_scroll();
            }
            (KeyCode::PageDown, _) => {
                self.cursor_line += self.viewport_h.saturating_sub(1);
                self.clamp_cursor();
                self.clamp_scroll();
            }
            _ => {}
        }
    }

    fn on_confirm_key(&mut self, code: KeyCode) {
        match code {
            // [Y]es / [S]í — both keys accepted in both languages.
            KeyCode::Char('y' | 'Y' | 's' | 'S') => {
                for doc in &mut self.docs {
                    if doc.is_modified() && doc.path().is_some() {
                        let _ = doc.save();
                    }
                }
                self.quit = true;
            }
            KeyCode::Char('n' | 'N') => self.quit = true,
            KeyCode::Char('c' | 'C') | KeyCode::Esc => self.confirm_quit = false,
            _ => {}
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        if let Event::Key(key) = event {
            // Windows terminals deliver Press and Release; act on Press only.
            if key.kind == KeyEventKind::Press {
                self.on_key(key);
            }
        }
    }

    // -- Drawing -----------------------------------------------------------

    pub fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        let [tabs_area, editor_area, status_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(area);

        self.viewport_h = editor_area.height as usize;
        self.viewport_w = editor_area.width as usize;
        self.clamp_scroll();

        self.draw_tabs(f, tabs_area);
        self.draw_editor(f, editor_area);
        self.draw_status(f, status_area);
    }

    fn doc_title(&self, index: usize) -> String {
        let doc = &self.docs[index];
        let name = doc
            .path()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| i18n::t(self.lang, Key::Untitled).to_string());
        if doc.is_modified() {
            format!(" {name} ● ")
        } else {
            format!(" {name} ")
        }
    }

    fn draw_tabs(&self, f: &mut Frame, area: ratatui::prelude::Rect) {
        let mut spans: Vec<Span> = Vec::new();
        for i in 0..self.docs.len() {
            let active = i == self.active;
            let style = if active {
                Style::new()
                    .fg(Color::White)
                    .bg(colors::TAB_ACTIVE_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(colors::FG).bg(colors::TAB_INACTIVE_BG)
            };
            let title = self.doc_title(i);
            let title = if active {
                title
            } else {
                title.trim_end().to_string() + " │"
            };
            spans.push(Span::styled(title, style));
        }
        let fill = Span::styled(
            " ".repeat(area.width as usize),
            Style::new().bg(colors::TAB_INACTIVE_BG),
        );
        spans.push(fill);
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn draw_editor(&mut self, f: &mut Frame, area: ratatui::prelude::Rect) {
        let first = self.scroll_top;
        let last = (self.scroll_top + self.viewport_h).min(self.doc().line_count());
        let visible: String = (first..last)
            .map(|i| self.doc().line(i))
            .collect::<Vec<_>>()
            .join("\n");
        let widget = Paragraph::new(visible)
            .style(Style::new().fg(colors::FG).bg(colors::BG))
            .scroll((0, self.col_off as u16));
        f.render_widget(widget, area);

        let x = area.x + (self.cursor_col - self.col_off) as u16;
        let y = area.y + (self.cursor_line - self.scroll_top) as u16;
        f.set_cursor_position((x, y));
    }

    fn draw_status(&mut self, f: &mut Frame, area: ratatui::prelude::Rect) {
        if self.confirm_quit {
            let prompt = i18n::t(self.lang, Key::SaveChangesPrompt);
            let widget = Paragraph::new(prompt)
                .style(Style::new().fg(Color::Black).bg(colors::MODIFIED));
            f.render_widget(widget, area);
            return;
        }
        let pos = i18n::tr(
            self.lang,
            Key::LnCol,
            &[
                ("ln", &(self.cursor_line + 1).to_string()),
                ("col", &(self.cursor_col + 1).to_string()),
            ],
        );
        let left = format!("{} {}", self.doc_title(self.active).trim_end(), pos);
        let right = self.status.clone();
        let width = area.width as usize;
        let mut left = left;
        if left.chars().count() > width {
            left = left.chars().take(width).collect();
        }
        let pad = width.saturating_sub(left.chars().count() + right.chars().count());
        let line = Line::from(vec![
            Span::styled(format!("{left}{}", " ".repeat(pad)), Style::default()),
            Span::styled(right, Style::new().add_modifier(Modifier::BOLD)),
        ]);
        let widget = Paragraph::new(line).style(Style::new().fg(Color::White).bg(colors::ACCENT));
        f.render_widget(widget, area);
        self.status.clear();
    }

    /// Sync the pane tree after focus changes (single pane in this slice).
    pub fn sync_tree(&mut self) {
        if let PaneTree::Single(pane) = &mut self.tree {
            pane.doc = self.active;
        }
    }
}

/// Run the event loop until the app quits.
pub fn run(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        app.sync_tree();
        terminal.draw(|f| app.draw(f))?;
        if app.quit {
            return Ok(());
        }
        app.handle_event(read()?);
    }
}
