//! Terminal application shell: event loop, drawing, and key handling.
//!
//! Slice scope: one pane from the pane tree, tab bar (render + Ctrl+PgUp/PgDn
//! switching), editor viewport with cursor, save (F2/Ctrl+S), quit (Ctrl+Q)
//! with unsaved-changes prompt, Insert-key overwrite toggle, and bounded
//! per-tab undo/redo (Ctrl+Z/Ctrl+Y).
//! Syntax highlighting and mouse support arrive in later tasks.

use std::io;

use ratatui::Frame;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, read};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::document::{Document, DocumentError, EditRecord};
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

/// What a pending confirmation applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    /// Quit the whole application.
    Quit,
    /// Close the tab at this document index.
    CloseTab(usize),
}

/// Cursor and scroll state saved per tab while another tab is focused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DocView {
    pub line: usize,
    pub col: usize,
    pub scroll_top: usize,
    pub col_off: usize,
}

/// Maximum undo steps kept per tab (oldest dropped first).
pub const UNDO_LIMIT: usize = 100;

/// Bounded per-tab edit history. One undo step is a `Vec<EditRecord>` because
/// one logical edit (an overwrite keystroke, a multi-line paste) can apply
/// more than one record; typing bursts merge consecutive characters so
/// Ctrl+Z removes a whole typed run at once.
#[derive(Debug, Default)]
pub struct History {
    pub undo: Vec<Vec<EditRecord>>,
    pub redo: Vec<Vec<EditRecord>>,
    /// True while consecutive typed characters merge into the last group.
    pub burst: bool,
}

impl History {
    fn new() -> Self {
        History::default()
    }
}

pub struct App {
    pub docs: Vec<Document>,
    pub tree: PaneTree,
    /// Index of the focused document (single pane in this slice).
    pub active: usize,
    /// Saved cursor/scroll per tab, index-aligned with `docs`.
    pub views: Vec<DocView>,
    /// Bounded edit history per tab, index-aligned with `docs`.
    pub histories: Vec<History>,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub scroll_top: usize,
    pub col_off: usize,
    pub viewport_h: usize,
    pub viewport_w: usize,
    pub quit: bool,
    /// Pending confirmation: quit the app or close one tab.
    pub confirm: Option<ConfirmKind>,
    /// Overwrite mode (Insert key). Off by default: VS Code-style insertion.
    pub overwrite: bool,
    /// Transient status message. Persists until the next edit action or the
    /// next status update, so save results and errors stay readable.
    pub status: String,
    pub lang: Lang,
}

impl App {
    pub fn new(docs: Vec<Document>, lang: Lang) -> Self {
        let views = docs.iter().map(|_| DocView::default()).collect();
        let histories = (0..docs.len()).map(|_| History::new()).collect();
        App {
            docs,
            tree: PaneTree::single(0),
            active: 0,
            views,
            histories,
            cursor_line: 0,
            cursor_col: 0,
            scroll_top: 0,
            col_off: 0,
            viewport_h: 1,
            viewport_w: 1,
            quit: false,
            confirm: None,
            overwrite: false,
            status: i18n::t(lang, Key::StartupHints).to_string(),
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

    /// Push applied edits into the tab's undo history as one step.
    /// `continues_burst` merges consecutive typed characters into a single
    /// undo step; anything else (Enter, Backspace, Delete, overwrite) closes
    /// the current burst.
    fn record_edits(&mut self, mut records: Vec<EditRecord>, continues_burst: bool) {
        if records.is_empty() {
            return;
        }
        let h = &mut self.histories[self.active];
        if continues_burst && h.burst {
            if let Some(last) = h.undo.last_mut() {
                last.append(&mut records);
            }
        } else {
            h.undo.push(records);
            if h.undo.len() > UNDO_LIMIT {
                h.undo.remove(0);
            }
        }
        h.redo.clear();
        h.burst = continues_burst;
    }

    /// Undo the last step (Ctrl+Z): apply the group's inverses newest-first
    /// and leave the cursor where the undone edit started.
    fn undo(&mut self) {
        let Some(group) = self.histories[self.active].undo.pop() else {
            return;
        };
        let origin = group.first().map(|r| match r {
            EditRecord::Insert { line, col, .. }
            | EditRecord::Delete {
                start_line: line,
                start_col: col,
                ..
            } => (*line, *col),
        });
        for record in group.iter().rev() {
            if let Err(e) = self.doc_mut().apply(&record.inverse()) {
                self.status = format!("{e}");
            }
        }
        self.histories[self.active].redo.push(group);
        if let Some((line, col)) = origin {
            (self.cursor_line, self.cursor_col) = (line, col);
            self.clamp_cursor();
            self.clamp_scroll();
        }
    }

    /// Redo the last undone step (Ctrl+Y): reapply the group oldest-first and
    /// leave the cursor at the end of the last reapplied edit.
    fn redo(&mut self) {
        let Some(group) = self.histories[self.active].redo.pop() else {
            return;
        };
        let end = group.last().map(|r| match r {
            EditRecord::Insert { line, col, text } => {
                let segments: Vec<&str> = text.split('\n').collect();
                if segments.len() == 1 {
                    // Single-line insert: end sits col + length on the line.
                    (*line, col + segments[0].chars().count())
                } else {
                    (
                        line + segments.len() - 1,
                        segments.last().map(|s| s.chars().count()).unwrap_or(0),
                    )
                }
            }
            EditRecord::Delete {
                end_line,
                end_col,
                ..
            } => (*end_line, *end_col),
        });
        for record in &group {
            if let Err(e) = self.doc_mut().apply(record) {
                self.status = format!("{e}");
            }
        }
        self.histories[self.active].undo.push(group);
        if let Some((line, col)) = end {
            (self.cursor_line, self.cursor_col) = (line, col);
            self.clamp_cursor();
            self.clamp_scroll();
        }
    }

    /// Perform an insert and return its record without touching history.
    fn do_insert(&mut self, text: &str) -> Option<EditRecord> {
        self.status.clear();
        let (line, col) = (self.cursor_line, self.cursor_col);
        match self.doc_mut().insert(line, col, text) {
            Ok(record) => {
                let parts: Vec<&str> = text.split('\n').collect();
                self.cursor_line += parts.len() - 1;
                if parts.len() == 1 {
                    // Single-line insert: the cursor advances past the text.
                    self.cursor_col += parts[0].chars().count();
                } else {
                    // Multi-line insert: cursor lands at the end of the pasted
                    // text (start of any tail that was pushed down with it).
                    self.cursor_col = parts.last().map(|s| s.chars().count()).unwrap_or(0);
                }
                Some(record)
            }
            Err(DocumentError::ReadOnly) => {
                self.status = "read-only".to_string();
                None
            }
            Err(e) => {
                self.status = format!("{e}");
                None
            }
        }
    }

    fn insert_text(&mut self, text: &str, continues_burst: bool) {
        if let Some(record) = self.do_insert(text) {
            self.record_edits(vec![record], continues_burst);
        }
        self.clamp_cursor();
    }

    /// Type one printable key. In overwrite mode (Insert), the character under
    /// the cursor is replaced instead of shifting text right; the replacement
    /// (delete + insert) is ONE undo step per keystroke.
    fn type_char(&mut self, c: char) {
        self.status.clear();
        let mut records: Vec<EditRecord> = Vec::new();
        let overwrite_replaced =
            self.overwrite && self.cursor_col < self.doc().line_len(self.cursor_line);
        if overwrite_replaced {
            let (line, col) = (self.cursor_line, self.cursor_col);
            match self.doc_mut().delete_range(line, col, line, col + 1) {
                Ok(record) => records.push(record),
                Err(e) => {
                    self.status = format!("{e}");
                    return;
                }
            }
        }
        match self.do_insert(&c.to_string()) {
            Some(record) => records.push(record),
            None => {
                self.clamp_cursor();
                return;
            }
        }
        // Plain typing continues the burst; each overwrite keystroke is its
        // own undo step (delete + insert together).
        self.record_edits(records, !overwrite_replaced);
        self.clamp_cursor();
    }

    fn backspace(&mut self) {
        self.status.clear();
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
        match result {
            Ok(record) => self.record_edits(vec![record], false),
            Err(e) => self.status = format!("{e}"),
        }
        self.clamp_cursor();
    }

    fn delete_forward(&mut self) {
        self.status.clear();
        let (line, col) = (self.cursor_line, self.cursor_col);
        let result = if col < self.doc().line_len(line) {
            self.doc_mut().delete_range(line, col, line, col + 1)
        } else if line + 1 < self.doc().line_count() {
            self.doc_mut().delete_range(line, col, line + 1, 0)
        } else {
            return;
        };
        match result {
            Ok(record) => self.record_edits(vec![record], false),
            Err(e) => self.status = format!("{e}"),
        }
        self.clamp_cursor();
    }

    // -- Commands ----------------------------------------------------------

    fn save_active(&mut self) {
        match self.doc_mut().save() {
            Ok(bytes) => {
                let path = self
                    .doc()
                    .path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.status = format!("{} {path} ({bytes} B)", i18n::t(self.lang, Key::Saved));
            }
            Err(e) => self.status = format!("{e}"),
        }
    }

    fn request_quit(&mut self) {
        if self.docs.iter().any(Document::is_modified) {
            self.confirm = Some(ConfirmKind::Quit);
        } else {
            self.quit = true;
        }
    }

    /// Save the focused tab's view, then focus `index` and restore its view.
    fn switch_to(&mut self, index: usize) {
        self.views[self.active] = self.current_view();
        self.active = index;
        let view = self.views[index];
        self.cursor_line = view.line;
        self.cursor_col = view.col;
        self.scroll_top = view.scroll_top;
        self.col_off = view.col_off;
        self.clamp_cursor();
        self.clamp_scroll();
    }

    fn current_view(&self) -> DocView {
        DocView {
            line: self.cursor_line,
            col: self.cursor_col,
            scroll_top: self.scroll_top,
            col_off: self.col_off,
        }
    }

    fn next_doc(&mut self, delta: isize) {
        let n = self.docs.len() as isize;
        let target = ((self.active as isize + delta).rem_euclid(n)) as usize;
        self.switch_to(target);
    }

    /// Focus the last document of panel `n` (1-based). Panel 2+ arrive with
    /// the split-screen milestone; until then only panel 1 exists.
    fn last_doc_of_panel(&mut self, panel: usize) {
        if panel == 1 {
            let last = self.docs.len() - 1;
            self.switch_to(last);
        }
    }

    // -- Tab lifecycle -----------------------------------------------------

    fn request_close(&mut self) {
        if self.doc().is_modified() {
            self.confirm = Some(ConfirmKind::CloseTab(self.active));
        } else {
            let index = self.active;
            self.close_tab(index);
        }
    }

    /// Remove the tab (after explicit confirmation when modified), keeping at
    /// least one empty document open, VS Code-style.
    fn close_tab(&mut self, index: usize) {
        self.docs.remove(index);
        self.views.remove(index);
        self.histories.remove(index);
        if self.docs.is_empty() {
            self.docs.push(Document::new_untitled());
            self.views.push(DocView::default());
            self.histories.push(History::new());
        }
        let target = index.min(self.docs.len() - 1);
        // The closed tab's view is gone; do NOT save the stale cursor into
        // the survivor's slot — load the target's view directly.
        self.active = target;
        let view = self.views[target];
        self.cursor_line = view.line;
        self.cursor_col = view.col;
        self.scroll_top = view.scroll_top;
        self.col_off = view.col_off;
        self.clamp_cursor();
        self.clamp_scroll();
    }

    // -- Events ------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        if self.confirm.is_some() {
            self.on_confirm_key(key.code);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            (KeyCode::Char('s' | 'S'), true) => self.save_active(),
            (KeyCode::F(2), _) => self.save_active(),
            (KeyCode::Char('q'), true) => self.request_quit(),
            (KeyCode::Char('w'), true) => self.request_close(),
            (KeyCode::Char('z'), true) => self.undo(),
            (KeyCode::Char('Z'), true) => self.redo(),
            (KeyCode::Char('y'), true) => self.redo(),
            (KeyCode::Char('1'), true) => self.last_doc_of_panel(1),
            (KeyCode::Char('2'), true) => self.last_doc_of_panel(2),
            (KeyCode::PageDown, true) => self.next_doc(1),
            (KeyCode::PageUp, true) => self.next_doc(-1),
            (KeyCode::Insert, _) => {
                self.overwrite = !self.overwrite;
                let key = if self.overwrite {
                    Key::OverwriteOn
                } else {
                    Key::OverwriteOff
                };
                self.status = i18n::t(self.lang, key).to_string();
            }
            (KeyCode::Char(c), false) => self.type_char(c),
            (KeyCode::Enter, _) => self.insert_text("\n", false),
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
        let Some(kind) = self.confirm else {
            return;
        };
        match (kind, code) {
            // [Y]es / [S]í — both keys accepted in both languages.
            (ConfirmKind::Quit, KeyCode::Char('y' | 'Y' | 's' | 'S')) => {
                // Save every modified document with a path; a failure keeps
                // the editor open and shows the error instead of losing work.
                let mut failure: Option<String> = None;
                for doc in &mut self.docs {
                    if doc.is_modified()
                        && doc.path().is_some()
                        && let Err(e) = doc.save()
                    {
                        failure = Some(e.to_string());
                        break;
                    }
                }
                match failure {
                    Some(e) => {
                        self.status = e;
                        self.confirm = None;
                    }
                    None => self.quit = true,
                }
            }
            (ConfirmKind::CloseTab(index), KeyCode::Char('y' | 'Y' | 's' | 'S')) => {
                let mut failure: Option<String> = None;
                if self.docs[index].path().is_some()
                    && let Err(e) = self.docs[index].save()
                {
                    failure = Some(e.to_string());
                }
                match failure {
                    Some(e) => {
                        self.status = e;
                        self.confirm = None;
                    }
                    None => {
                        self.confirm = None;
                        self.close_tab(index);
                    }
                }
            }
            (ConfirmKind::Quit, KeyCode::Char('n' | 'N')) => self.quit = true,
            (ConfirmKind::CloseTab(index), KeyCode::Char('n' | 'N')) => {
                self.confirm = None;
                self.close_tab(index);
            }
            (_, KeyCode::Char('c' | 'C')) | (_, KeyCode::Esc) => self.confirm = None,
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
        if self.confirm.is_some() {
            let prompt = i18n::t(self.lang, Key::SaveChangesPrompt);
            let widget =
                Paragraph::new(prompt).style(Style::new().fg(Color::Black).bg(colors::MODIFIED));
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
        let mode = if self.overwrite { "OVR " } else { "" };
        let left = format!("{}{mode}{}", self.doc_title(self.active).trim_end(), pos);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        // KeyEvent::new sets kind = Press, which is what handle_event acts on.
        KeyEvent::new(code, modifiers)
    }

    fn plain(code: KeyCode) -> KeyEvent {
        press(code, KeyModifiers::NONE)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        press(code, KeyModifiers::CONTROL)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("editormini_app_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn typed(app: &mut App, text: &str) {
        for c in text.chars() {
            app.on_key(plain(KeyCode::Char(c)));
        }
    }

    #[test]
    fn typing_inserts_and_advances_cursor() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hola");
        assert_eq!(app.doc().line(0), "hola");
        assert_eq!(app.cursor_col, 4);
        assert!(app.doc().is_modified());
    }

    #[test]
    fn ctrl_s_saves_pathed_document_and_reports_bytes() {
        let dir = temp_dir("save");
        let path = dir.join("save.txt");
        let mut app = App::new(vec![Document::new_with_path(&path)], Lang::En);
        typed(&mut app, "contenido");
        app.on_key(ctrl(KeyCode::Char('s')));
        assert!(
            !app.doc().is_modified(),
            "save must clear the modified flag"
        );
        let on_disk = std::fs::read_to_string(&path).expect("file saved");
        assert_eq!(on_disk, "contenido\r\n");
        assert!(app.status.contains("Saved"), "status: {}", app.status);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn f2_saves_too() {
        let dir = temp_dir("f2");
        let path = dir.join("f2.txt");
        let mut app = App::new(vec![Document::new_with_path(&path)], Lang::En);
        typed(&mut app, "x");
        app.on_key(plain(KeyCode::F(2)));
        assert!(!app.doc().is_modified());
        assert_eq!(std::fs::read_to_string(&path).expect("saved"), "x\r\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ctrl_s_on_untitled_shows_persistent_error() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "draft");
        app.on_key(ctrl(KeyCode::Char('s')));
        assert!(
            app.status.contains("file path"),
            "error must be visible, got: {}",
            app.status
        );
        assert!(
            app.doc().is_modified(),
            "untitled save must not lose changes"
        );
    }

    #[test]
    fn status_persists_until_next_edit_action() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "abc");
        app.on_key(ctrl(KeyCode::Char('s')));
        let error = app.status.clone();
        assert!(!error.is_empty());
        // Cursor movement keeps the message visible...
        app.on_key(plain(KeyCode::Left));
        assert_eq!(app.status, error);
        // ...but the next edit action clears it.
        app.on_key(plain(KeyCode::Char('x')));
        assert!(app.status.is_empty());
    }

    #[test]
    fn insert_toggles_overwrite_replacing_char_under_cursor() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "abc");
        app.on_key(plain(KeyCode::Home));
        app.on_key(plain(KeyCode::Right));
        app.on_key(plain(KeyCode::Insert));
        assert!(app.overwrite);
        app.on_key(plain(KeyCode::Char('X')));
        assert_eq!(app.doc().line(0), "aXc");
        assert_eq!(app.cursor_col, 2);
        // Typing past end of line still appends in overwrite mode.
        app.on_key(plain(KeyCode::End));
        app.on_key(plain(KeyCode::Char('Z')));
        assert_eq!(app.doc().line(0), "aXcZ");
        // Toggle back to insert mode.
        app.on_key(plain(KeyCode::Insert));
        assert!(!app.overwrite);
        app.on_key(plain(KeyCode::Home));
        app.on_key(plain(KeyCode::Char('-')));
        assert_eq!(app.doc().line(0), "-aXcZ");
    }

    #[test]
    fn ctrl_q_yes_saves_modified_docs_and_quits() {
        let dir = temp_dir("quityes");
        let path = dir.join("q.txt");
        let mut app = App::new(vec![Document::new_with_path(&path)], Lang::En);
        typed(&mut app, "work");
        app.on_key(ctrl(KeyCode::Char('q')));
        assert_eq!(app.confirm, Some(ConfirmKind::Quit));
        assert!(!app.quit);
        app.on_key(plain(KeyCode::Char('y')));
        assert!(app.quit);
        assert_eq!(std::fs::read_to_string(&path).expect("saved"), "work\r\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ctrl_q_n_quits_without_saving() {
        let dir = temp_dir("quitno");
        let path = dir.join("q.txt");
        std::fs::write(&path, "").expect("pre-create empty file");
        let mut app = App::new(vec![Document::new_with_path(&path)], Lang::En);
        typed(&mut app, "work");
        app.on_key(ctrl(KeyCode::Char('q')));
        app.on_key(plain(KeyCode::Char('n')));
        assert!(app.quit);
        assert_eq!(
            std::fs::read_to_string(&path).expect("untouched"),
            "",
            "quit without saving must not write"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ctrl_q_c_cancels_and_keeps_changes() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "keep me");
        app.on_key(ctrl(KeyCode::Char('q')));
        assert_eq!(app.confirm, Some(ConfirmKind::Quit));
        app.on_key(plain(KeyCode::Char('c')));
        assert_eq!(app.confirm, None);
        assert!(!app.quit);
        assert_eq!(app.doc().line(0), "keep me");
    }

    #[test]
    fn ctrl_pgdn_cycles_documents() {
        let mut app = App::new(
            vec![Document::new_untitled(), Document::new_untitled()],
            Lang::En,
        );
        assert_eq!(app.active, 0);
        app.on_key(ctrl(KeyCode::PageDown));
        assert_eq!(app.active, 1);
        app.on_key(ctrl(KeyCode::PageDown));
        assert_eq!(app.active, 0, "wraps around");
        app.on_key(ctrl(KeyCode::PageUp));
        assert_eq!(app.active, 1);
    }

    #[test]
    fn startup_hints_are_the_first_status() {
        let app = App::new(vec![Document::new_untitled()], Lang::Es);
        assert!(app.status.contains("guardar"), "status: {}", app.status);
    }

    #[test]
    fn spanish_yes_key_saves_and_quits() {
        let dir = temp_dir("quitsi");
        let path = dir.join("q.txt");
        let mut app = App::new(vec![Document::new_with_path(&path)], Lang::Es);
        typed(&mut app, "datos");
        app.on_key(ctrl(KeyCode::Char('q')));
        app.on_key(plain(KeyCode::Char('s'))); // [S]í
        assert!(app.quit);
        assert_eq!(std::fs::read_to_string(&path).expect("saved"), "datos\r\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ctrl_1_focuses_last_document_of_panel_1() {
        let mut app = App::new(
            vec![
                Document::new_untitled(),
                Document::new_untitled(),
                Document::new_untitled(),
            ],
            Lang::En,
        );
        app.on_key(ctrl(KeyCode::Char('1')));
        assert_eq!(app.active, 2, "Ctrl+1 jumps to the last doc of panel 1");
    }

    #[test]
    fn ctrl_2_is_reserved_until_panels_exist() {
        let mut app = App::new(
            vec![Document::new_untitled(), Document::new_untitled()],
            Lang::En,
        );
        app.on_key(ctrl(KeyCode::Char('2')));
        assert_eq!(app.active, 0, "no panel 2 yet: reserved no-op");
    }

    #[test]
    fn ctrl_w_closes_unmodified_tab_immediately() {
        let mut app = App::new(
            vec![Document::new_untitled(), Document::new_untitled()],
            Lang::En,
        );
        app.on_key(ctrl(KeyCode::Char('w')));
        assert_eq!(app.docs.len(), 1);
        assert_eq!(app.active, 0);
        assert_eq!(app.confirm, None);
    }

    #[test]
    fn ctrl_w_modified_prompts_then_y_saves_and_closes() {
        let dir = temp_dir("closeyes");
        let path_a = dir.join("a.txt");
        let path_b = dir.join("b.txt");
        let mut app = App::new(
            vec![
                Document::new_with_path(&path_a),
                Document::new_with_path(&path_b),
            ],
            Lang::En,
        );
        typed(&mut app, "content A");
        app.on_key(ctrl(KeyCode::Char('w')));
        assert_eq!(app.confirm, Some(ConfirmKind::CloseTab(0)));
        app.on_key(plain(KeyCode::Char('y')));
        assert_eq!(app.docs.len(), 1, "tab A closed");
        assert_eq!(app.doc().path(), Some(path_b.as_path()));
        assert_eq!(
            std::fs::read_to_string(&path_a).expect("saved before close"),
            "content A\r\n"
        );
        let _ = std::fs::remove_file(&path_a);
        let _ = std::fs::remove_file(&path_b);
    }

    #[test]
    fn ctrl_w_prompt_n_closes_without_saving() {
        let dir = temp_dir("closeno");
        let path = dir.join("a.txt");
        std::fs::write(&path, "").expect("pre-create empty file");
        let mut app = App::new(vec![Document::new_with_path(&path)], Lang::En);
        typed(&mut app, "discard me");
        app.on_key(ctrl(KeyCode::Char('w')));
        assert_eq!(app.confirm, Some(ConfirmKind::CloseTab(0)));
        app.on_key(plain(KeyCode::Char('n')));
        assert_eq!(
            app.docs.len(),
            1,
            "closed last tab replaced by fresh untitled"
        );
        assert!(app.doc().path().is_none());
        assert_eq!(
            std::fs::read_to_string(&path).expect("untouched"),
            "",
            "discard must not write"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ctrl_w_prompt_c_cancels_close() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "keep");
        app.on_key(ctrl(KeyCode::Char('w')));
        assert_eq!(app.confirm, Some(ConfirmKind::CloseTab(0)));
        app.on_key(plain(KeyCode::Char('c')));
        assert_eq!(app.confirm, None);
        assert_eq!(app.docs.len(), 1);
        assert_eq!(app.doc().line(0), "keep");
    }

    #[test]
    fn cursor_and_scroll_restored_on_tab_switch() {
        let mut app = App::new(
            vec![Document::new_untitled(), Document::new_untitled()],
            Lang::En,
        );
        typed(&mut app, "hola");
        app.on_key(ctrl(KeyCode::PageDown));
        typed(&mut app, "xy");
        app.on_key(ctrl(KeyCode::PageUp));
        assert_eq!(app.doc().line(0), "hola");
        assert_eq!((app.cursor_line, app.cursor_col), (0, 4));
        app.on_key(ctrl(KeyCode::PageDown));
        assert_eq!(app.doc().line(0), "xy");
        assert_eq!((app.cursor_line, app.cursor_col), (0, 2));
    }

    #[test]
    fn typing_burst_undone_in_one_step() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hola");
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line(0), "", "whole typed run goes in one Ctrl+Z");
        assert_eq!((app.cursor_line, app.cursor_col), (0, 0));
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hola");
        app.on_key(ctrl(KeyCode::Char('z')));
        app.on_key(ctrl(KeyCode::Char('y')));
        assert_eq!(app.doc().line(0), "hola");
        assert_eq!(app.cursor_col, 4, "redo leaves the cursor after the text");
    }

    #[test]
    fn enter_and_type_break_into_separate_steps() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "ab");
        app.on_key(plain(KeyCode::Enter));
        typed(&mut app, "cd");
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line_count(), 2);
        assert_eq!(app.doc().line(0), "ab");
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line_count(), 1);
        assert_eq!(app.doc().line(0), "ab");
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line(0), "");
    }

    #[test]
    fn backspace_closes_the_typing_burst() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "abc");
        app.on_key(plain(KeyCode::Backspace));
        typed(&mut app, "de");
        assert_eq!(app.doc().line(0), "abde");
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line(0), "ab", "burst closed by backspace");
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line(0), "abc", "backspace itself is undoable");
    }

    #[test]
    fn redo_is_cleared_by_a_new_edit() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "ab");
        app.on_key(ctrl(KeyCode::Char('z')));
        typed(&mut app, "x");
        app.on_key(ctrl(KeyCode::Char('y')));
        assert_eq!(app.doc().line(0), "x", "redo stack cleared by new edit");
    }

    #[test]
    fn undo_is_bounded() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        for _ in 0..150 {
            app.on_key(plain(KeyCode::Char('a')));
            app.on_key(plain(KeyCode::Backspace));
        }
        assert!(
            app.histories[0].undo.len() <= UNDO_LIMIT,
            "history must stay bounded"
        );
    }

    #[test]
    fn histories_are_per_tab() {
        let mut app = App::new(
            vec![Document::new_untitled(), Document::new_untitled()],
            Lang::En,
        );
        typed(&mut app, "aaa");
        app.on_key(ctrl(KeyCode::PageDown));
        typed(&mut app, "bbb");
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line(0), "", "own history: whole burst undone");
        app.on_key(ctrl(KeyCode::PageUp));
        assert_eq!(app.doc().line(0), "aaa", "other tab history untouched");
    }

    #[test]
    fn overwrite_typing_undone_in_one_step() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "abc");
        app.on_key(plain(KeyCode::Home));
        app.on_key(plain(KeyCode::Insert));
        app.on_key(plain(KeyCode::Char('X')));
        assert_eq!(app.doc().line(0), "Xbc");
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line(0), "abc", "replace + insert undo together");
    }
}
