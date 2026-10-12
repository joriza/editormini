//! Terminal application shell: event loop, drawing, and key handling.
//!
//! Slice scope: one pane from the pane tree, tab bar (render + Ctrl+PgUp/PgDn
//! switching), editor viewport with cursor, save (F2/Ctrl+S), quit (Ctrl+Q)
//! with unsaved-changes prompt, Insert-key overwrite toggle, bounded
//! per-tab undo/redo (Ctrl+Z/Ctrl+Y), syntax highlighting by extension
//! (highlight.rs + theme.rs). Mouse support arrives in a later task.

use std::io;
use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind, read,
};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::config::ThemeChoice;
use crate::document::{Document, DocumentError, EditRecord};
use crate::highlight;
use crate::i18n::{self, Key, Lang};
use crate::pane::PaneTree;
use crate::theme::Theme;

/// Dark+ palette subset used before the theme milestone formalizes it.
/// What a pending confirmation applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    /// Quit the whole application.
    Quit,
    /// Close the tab at this document index.
    CloseTab(usize),
}

/// What a pending text-input prompt is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// Open a file from disk.
    OpenFile,
    /// Save an untitled document to a new path.
    SaveAs,
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
    /// Pending text-input prompt: open file or save-as destination.
    pub prompt: Option<(PromptKind, String)>,
    /// Overwrite mode (Insert key). Off by default: VS Code-style insertion.
    pub overwrite: bool,
    /// Transient status message. Persists until the next edit action or the
    /// next status update, so save results and errors stay readable.
    pub status: String,
    pub lang: Lang,
    /// Active chrome theme (tabs, status bar, editor background).
    pub theme: Theme,
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
            prompt: None,
            overwrite: false,
            status: i18n::t(lang, Key::StartupHints).to_string(),
            lang,
            theme: Theme::for_choice(ThemeChoice::DarkPlus),
        }
    }

    /// Apply a configured theme to the running app.
    pub fn set_theme(&mut self, choice: ThemeChoice) {
        self.theme = Theme::for_choice(choice);
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
                self.status = i18n::error_message(self.lang, &e);
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
                end_line, end_col, ..
            } => (*end_line, *end_col),
        });
        for record in &group {
            if let Err(e) = self.doc_mut().apply(record) {
                self.status = i18n::error_message(self.lang, &e);
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
                self.status = i18n::t(self.lang, Key::ReadOnly).to_string();
                None
            }
            Err(e) => {
                self.status = i18n::error_message(self.lang, &e);
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
                    self.status = i18n::error_message(self.lang, &e);
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
            Err(e) => self.status = i18n::error_message(self.lang, &e),
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
            Err(e) => self.status = i18n::error_message(self.lang, &e),
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
            Err(DocumentError::Untitled) if self.doc().is_modified() => {
                self.prompt = Some((PromptKind::SaveAs, String::new()));
            }
            Err(e) => self.status = i18n::error_message(self.lang, &e),
        }
    }

    /// Save every modified tab that has a path. Modified untitled tabs
    /// are left untouched. Simplest correct status feedback.
    fn save_all(&mut self) {
        let mut saved = 0;
        let mut failure: Option<String> = None;
        for i in 0..self.docs.len() {
            if self.docs[i].is_modified() && self.docs[i].path().is_some() {
                if let Err(e) = self.docs[i].save() {
                    failure = Some(i18n::error_message(self.lang, &e));
                    break;
                }
                saved += 1;
            }
        }
        if let Some(e) = failure {
            self.status = e;
        } else if saved > 0 {
            self.status = i18n::t(self.lang, Key::SavedAll).to_string();
        } else {
            self.status = i18n::t(self.lang, Key::NoModifiedFiles).to_string();
        }
    }

    /// Open the text-input prompt for a file path.
    fn open_file_prompt(&mut self) {
        self.prompt = Some((PromptKind::OpenFile, String::new()));
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
        if self.prompt.is_some() {
            self.on_prompt_key(key);
            return;
        }
        if self.confirm.is_some() {
            self.on_confirm_key(key.code);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match (key.code, ctrl, shift) {
            // Ctrl+Shift+S → save all (must precede plain Ctrl+S)
            (KeyCode::Char('s' | 'S'), true, true) => self.save_all(),
            // Ctrl+S → save active
            (KeyCode::Char('s' | 'S'), true, _) => self.save_active(),
            // F2 → save active
            (KeyCode::F(2), _, _) => self.save_active(),
            // F3 → save all (fallback for Ctrl+Shift+S on Windows)
            (KeyCode::F(3), _, _) => self.save_all(),
            // Ctrl+O → open file prompt
            (KeyCode::Char('o' | 'O'), true, _) => self.open_file_prompt(),
            (KeyCode::Char('q'), true, _) => self.request_quit(),
            (KeyCode::Char('w'), true, _) => self.request_close(),
            (KeyCode::Char('z'), true, _) => self.undo(),
            (KeyCode::Char('Z'), true, _) => self.redo(),
            (KeyCode::Char('y'), true, _) => self.redo(),
            (KeyCode::Char('1'), true, _) => self.last_doc_of_panel(1),
            (KeyCode::Char('2'), true, _) => self.last_doc_of_panel(2),
            (KeyCode::PageDown, true, _) => self.next_doc(1),
            (KeyCode::PageUp, true, _) => self.next_doc(-1),
            (KeyCode::Insert, _, _) => {
                self.overwrite = !self.overwrite;
                let key = if self.overwrite {
                    Key::OverwriteOn
                } else {
                    Key::OverwriteOff
                };
                self.status = i18n::t(self.lang, key).to_string();
            }
            (KeyCode::Char(c), false, _) => self.type_char(c),
            (KeyCode::Enter, _, _) => self.insert_text("\n", false),
            (KeyCode::Backspace, _, _) => self.backspace(),
            (KeyCode::Delete, _, _) => self.delete_forward(),
            (KeyCode::Left, _, _) => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                } else if self.cursor_line > 0 {
                    self.cursor_line -= 1;
                    self.cursor_col = self.doc().line_len(self.cursor_line);
                }
                self.clamp_scroll();
            }
            (KeyCode::Right, _, _) => {
                if self.cursor_col < self.doc().line_len(self.cursor_line) {
                    self.cursor_col += 1;
                } else if self.cursor_line + 1 < self.doc().line_count() {
                    self.cursor_line += 1;
                    self.cursor_col = 0;
                }
                self.clamp_scroll();
            }
            (KeyCode::Up, _, _) => {
                self.cursor_line = self.cursor_line.saturating_sub(1);
                self.clamp_cursor();
                self.clamp_scroll();
            }
            (KeyCode::Down, _, _) => {
                self.cursor_line += 1;
                self.clamp_cursor();
                self.clamp_scroll();
            }
            (KeyCode::Home, _, _) => {
                self.cursor_col = 0;
                self.clamp_scroll();
            }
            (KeyCode::End, _, _) => {
                self.cursor_col = self.doc().line_len(self.cursor_line);
                self.clamp_scroll();
            }
            (KeyCode::PageUp, _, _) => {
                self.cursor_line = self
                    .cursor_line
                    .saturating_sub(self.viewport_h.saturating_sub(1));
                self.clamp_cursor();
                self.clamp_scroll();
            }
            (KeyCode::PageDown, _, _) => {
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
                        failure = Some(i18n::error_message(self.lang, &e));
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
                    failure = Some(i18n::error_message(self.lang, &e));
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

    /// Handle keys while a text-input prompt is active.
    fn on_prompt_key(&mut self, key: KeyEvent) {
        let Some((kind, input)) = &mut self.prompt else {
            return;
        };
        match key.code {
            KeyCode::Enter => {
                let path = input.clone();
                let kind = *kind;
                self.prompt = None;
                self.handle_prompt_confirm(&kind, &path);
            }
            KeyCode::Esc => {
                self.prompt = None;
            }
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                input.push(c);
            }
            _ => {}
        }
    }

    /// Act on the confirmed path in a prompt.
    fn handle_prompt_confirm(&mut self, kind: &PromptKind, path: &str) {
        match kind {
            PromptKind::OpenFile => {
                let pathbuf = PathBuf::from(path);
                if pathbuf.exists() {
                    match Document::from_path(&pathbuf) {
                        Ok(doc) => {
                            self.docs.push(doc);
                            self.views.push(DocView::default());
                            self.histories.push(History::new());
                            self.status = i18n::tr(self.lang, Key::FileOpened, &[("path", path)]);
                        }
                        Err(e) => {
                            self.status = i18n::error_message(self.lang, &e);
                        }
                    }
                } else {
                    // New file: create untitled with path.
                    self.docs.push(Document::new_with_path(&pathbuf));
                    self.views.push(DocView::default());
                    self.histories.push(History::new());
                    self.status = i18n::tr(self.lang, Key::FileOpened, &[("path", path)]);
                }
            }
            PromptKind::SaveAs => {
                match self.doc_mut().save_as(path) {
                    Ok(bytes) => {
                        self.status = i18n::tr(self.lang, Key::SavedAs, &[("path", path)]);
                        // Append byte count like save_active does.
                        self.status = format!("{} {bytes} B", self.status);
                    }
                    Err(e) => {
                        self.status = i18n::error_message(self.lang, &e);
                    }
                }
            }
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) => {
                // Windows terminals deliver Press and Release; act on Press only.
                if key.kind == KeyEventKind::Press {
                    self.on_key(key);
                }
            }
            Event::Mouse(mouse) => {
                self.on_mouse(mouse.kind, mouse.column, mouse.row, mouse.modifiers);
            }
            _ => {}
        }
    }

    // -- Mouse ------------------------------------------------------------

    /// Handle a mouse event: left-click places the cursor (or switches
    /// tabs), wheel scrolls the viewport, right/middle clicks are ignored.
    pub fn on_mouse(
        &mut self,
        kind: MouseEventKind,
        column: u16,
        row: u16,
        _modifiers: KeyModifiers,
    ) {
        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if row == 0 {
                    // Click on the tab bar: switch to the clicked tab.
                    if let Some(index) = self.tab_at_column(column) {
                        self.switch_to(index);
                    }
                    return;
                }
                // Click inside the editor viewport: place the cursor.
                let editor_row = row.saturating_sub(1);
                if editor_row >= self.viewport_h as u16 {
                    return;
                }
                let buf_line = self.scroll_top + editor_row as usize;
                let buf_col = self.col_off + column as usize;
                self.cursor_line = buf_line;
                self.cursor_col = buf_col;
                self.clamp_cursor();
                self.clamp_scroll();
            }
            MouseEventKind::ScrollUp => {
                let n = self.viewport_h.saturating_sub(1);
                self.scroll_top = self.scroll_top.saturating_sub(n);
                self.clamp_scroll();
            }
            MouseEventKind::ScrollDown => {
                let n = self.viewport_h.saturating_sub(1);
                self.scroll_top += n;
                let max_scroll = self.doc().line_count().saturating_sub(self.viewport_h);
                self.scroll_top = self.scroll_top.min(max_scroll);
                self.clamp_scroll();
            }
            _ => {}
        }
    }

    /// Shared layout helper used by both the tab-bar render and
    /// tab_at_column hit-testing. A render change automatically updates
    /// hit-testing because both consume the same ranges.
    /// Returns (start_col, title, tab_index) for each tab.
    fn tab_layout(&self) -> Vec<(u16, String, usize)> {
        let mut col: u16 = 0;
        let mut result = Vec::new();
        for i in 0..self.docs.len() {
            let title = self.doc_title(i);
            let title = if i == self.active {
                title
            } else {
                title.trim_end().to_string() + " │"
            };
            let width = title.chars().count() as u16;
            result.push((col, title, i));
            col += width;
        }
        result
    }

    /// Return the index of the tab at `column`, or `None` if the click
    /// landed on empty tab-bar space (the fill span).
    fn tab_at_column(&self, column: u16) -> Option<usize> {
        self.tab_layout()
            .iter()
            .find(|(start, title, _)| {
                column >= *start && column < start + title.chars().count() as u16
            })
            .map(|(_, _, i)| *i)
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
        for (_, title, i) in self.tab_layout() {
            let active = i == self.active;
            let style = if active {
                Style::new()
                    .fg(Color::White)
                    .bg(self.theme.tab_active_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new()
                    .fg(self.theme.fg)
                    .bg(self.theme.tab_inactive_bg)
            };
            spans.push(Span::styled(title, style));
        }
        let fill = Span::styled(
            " ".repeat(area.width as usize),
            Style::new().bg(self.theme.tab_inactive_bg),
        );
        spans.push(fill);
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn draw_editor(&mut self, f: &mut Frame, area: ratatui::prelude::Rect) {
        let first = self.scroll_top;
        let last = (self.scroll_top + self.viewport_h).min(self.doc().line_count());
        let lines: Vec<&str> = (first..last).map(|i| self.doc().line(i)).collect();
        let extension = highlight::extension_of(self.doc().path());
        let styled =
            highlight::styled_lines(&lines, extension.as_deref(), self.theme.fg, self.theme.bg);
        let widget = Paragraph::new(styled);
        f.render_widget(widget, area);

        let x = area.x + (self.cursor_col - self.col_off) as u16;
        let y = area.y + (self.cursor_line - self.scroll_top) as u16;
        f.set_cursor_position((x, y));
    }

    /// Compose the status-bar left and right segments for the active doc
    /// (no modal open), truncating the left side with ellipsis when the
    /// combined content exceeds `width` terminal cells.
    ///
    /// Left:  doc identity + cursor position + mode hints.
    /// Right: language indicator + transient status message.
    fn status_line(&self, width: usize) -> (String, String) {
        let doc = self.doc();
        let pos = i18n::tr(
            self.lang,
            Key::LnCol,
            &[
                ("ln", &(self.cursor_line + 1).to_string()),
                ("col", &(self.cursor_col + 1).to_string()),
            ],
        );
        let mode = if self.overwrite {
            i18n::t(self.lang, Key::OverwriteIndicator)
        } else {
            ""
        };
        let read_only = if doc.is_read_only() {
            format!(" {}", i18n::t(self.lang, Key::ReadOnly))
        } else {
            String::new()
        };
        let mut left = format!(
            "{}{mode}{pos}{read_only}",
            self.doc_title(self.active).trim_end()
        );
        let ext = highlight::extension_of(doc.path());
        let language = highlight::language_name(ext.as_deref())
            .unwrap_or_else(|| i18n::t(self.lang, Key::PlainText));
        let right = format!(" {language} {}", self.status);
        // Truncate left with ellipsis when total content exceeds width.
        let right_len = right.chars().count();
        let available = width.saturating_sub(right_len);
        if left.chars().count() > available {
            let truncate_to = available.saturating_sub(1);
            left = if truncate_to > 0 {
                format!("{}…", left.chars().take(truncate_to).collect::<String>())
            } else {
                "…".to_string()
            };
        }
        (left, right)
    }

    fn draw_status(&mut self, f: &mut Frame, area: ratatui::prelude::Rect) {
        if let Some((kind, input)) = &self.prompt {
            let title = match kind {
                PromptKind::OpenFile => i18n::t(self.lang, Key::OpenFile),
                PromptKind::SaveAs => i18n::t(self.lang, Key::SaveAs),
            };
            let text = format!("{}{input}", title);
            let widget = Paragraph::new(text).style(
                Style::new()
                    .fg(self.theme.prompt_fg)
                    .bg(self.theme.prompt_bg),
            );
            f.render_widget(widget, area);
            return;
        }
        if self.confirm.is_some() {
            let prompt = i18n::t(self.lang, Key::SaveChangesPrompt);
            let widget = Paragraph::new(prompt).style(
                Style::new()
                    .fg(self.theme.prompt_fg)
                    .bg(self.theme.prompt_bg),
            );
            f.render_widget(widget, area);
            return;
        }
        let width = area.width as usize;
        let (left, right) = self.status_line(width);
        let pad = width.saturating_sub(left.chars().count() + right.chars().count());
        let line = Line::from(vec![
            Span::styled(format!("{left}{}", " ".repeat(pad)), Style::default()),
            Span::styled(right, Style::new().add_modifier(Modifier::BOLD)),
        ]);
        let widget =
            Paragraph::new(line).style(Style::new().fg(Color::White).bg(self.theme.accent));
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
        assert!(
            app.status.contains(i18n::t(Lang::En, Key::Saved)),
            "status: {}",
            app.status
        );
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
    fn ctrl_s_on_modified_untitled_opens_save_as_prompt() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "draft");
        app.on_key(ctrl(KeyCode::Char('s')));
        assert_eq!(
            app.prompt,
            Some((PromptKind::SaveAs, String::new())),
            "Ctrl+S on modified untitled must open SaveAs prompt"
        );
        assert!(
            app.doc().is_modified(),
            "untitled save must not lose changes"
        );
        assert!(
            app.status.is_empty(),
            "prompt replaces the status, got: {}",
            app.status
        );
    }

    #[test]
    fn status_persists_until_next_edit_action() {
        let dir = temp_dir("status");
        let path = dir.join("status.txt");
        let mut app = App::new(vec![Document::new_with_path(&path)], Lang::En);
        typed(&mut app, "abc");
        app.on_key(ctrl(KeyCode::Char('s')));
        let saved = app.status.clone();
        assert!(!saved.is_empty());
        // Cursor movement keeps the message visible...
        app.on_key(plain(KeyCode::Left));
        assert_eq!(app.status, saved);
        // ...but the next edit action clears it.
        app.on_key(plain(KeyCode::Char('x')));
        assert!(app.status.is_empty());
        let _ = std::fs::remove_file(&path);
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

    // -- Task 8: save_all / Ctrl+Shift+S / F3 -------------------------

    #[test]
    fn ctrl_shift_s_saves_all_pathed_modified_tabs() {
        let dir = temp_dir("saveall");
        let path_a = dir.join("a.txt");
        let path_b = dir.join("b.txt");
        std::fs::write(&path_a, "").expect("pre-create");
        std::fs::write(&path_b, "").expect("pre-create");
        let mut app = App::new(
            vec![
                Document::new_with_path(&path_a),
                Document::new_with_path(&path_b),
                Document::new_untitled(),
            ],
            Lang::En,
        );
        // Modify all three tabs
        typed(&mut app, "content A");
        app.on_key(ctrl(KeyCode::PageDown));
        typed(&mut app, "content B");
        app.on_key(ctrl(KeyCode::PageDown));
        typed(&mut app, "untitled content");
        // Ctrl+Shift+S saves pathed modified tabs, skips untitled
        app.on_key(press(
            KeyCode::Char('s'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        assert!(
            app.status.contains("All files saved"),
            "status: {}",
            app.status
        );
        // Pathed tabs should be clean
        app.on_key(ctrl(KeyCode::PageUp));
        app.on_key(ctrl(KeyCode::PageUp));
        assert!(!app.doc().is_modified(), "tab A should be saved and clean");
        app.on_key(ctrl(KeyCode::PageDown));
        assert!(!app.doc().is_modified(), "tab B should be saved and clean");
        // Untitled tab is still modified (skipped by save_all)
        app.on_key(ctrl(KeyCode::PageDown));
        assert!(
            app.doc().is_modified(),
            "untitled tab should still be modified (skipped)"
        );
        let _ = std::fs::remove_file(&path_a);
        let _ = std::fs::remove_file(&path_b);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn f3_saves_all_too() {
        let dir = temp_dir("f3saveall");
        let path = dir.join("f3.txt");
        std::fs::write(&path, "").expect("pre-create");
        let mut app = App::new(vec![Document::new_with_path(&path)], Lang::En);
        typed(&mut app, "f3 content");
        app.on_key(plain(KeyCode::F(3)));
        assert!(
            app.status.contains("All files saved"),
            "status: {}",
            app.status
        );
        assert!(!app.doc().is_modified());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ctrl_shift_s_before_ctrl_s_no_collision() {
        let dir = temp_dir("collision");
        let path = dir.join("collision.txt");
        std::fs::write(&path, "").expect("pre-create");
        let mut app = App::new(vec![Document::new_with_path(&path)], Lang::En);
        typed(&mut app, "first");
        // Ctrl+Shift+S saves all (this one pathed tab)
        app.on_key(press(
            KeyCode::Char('s'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        assert!(
            app.status.contains("All files saved"),
            "status: {}",
            app.status
        );
        assert!(!app.doc().is_modified());
        // Modify again
        typed(&mut app, " second");
        // Ctrl+S saves active only
        app.on_key(ctrl(KeyCode::Char('s')));
        assert!(app.status.contains("Saved"), "status: {}", app.status);
        assert!(!app.doc().is_modified());
        let content = std::fs::read_to_string(&path).expect("reread");
        assert_eq!(content, "first second\r\n");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- Task 8: Ctrl+O open prompt -----------------------------------

    #[test]
    fn ctrl_o_opens_open_file_prompt() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl(KeyCode::Char('o')));
        assert_eq!(
            app.prompt,
            Some((PromptKind::OpenFile, String::new())),
            "Ctrl+O must open OpenFile prompt"
        );
    }

    #[test]
    fn open_file_prompt_existing_file_opens_as_tab() {
        let dir = temp_dir("openfile");
        let path = dir.join("existing.txt");
        std::fs::write(&path, "hello world").expect("pre-create");
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        // Open the prompt
        app.on_key(ctrl(KeyCode::Char('o')));
        assert_eq!(app.prompt, Some((PromptKind::OpenFile, String::new())));
        // Type the path
        let path_str = path.to_string_lossy().to_string();
        typed(&mut app, &path_str);
        // Confirm
        app.on_key(plain(KeyCode::Enter));
        // The file should be opened as a new tab (appends, doesn't switch focus)
        assert_eq!(app.docs.len(), 2, "new tab opened");
        assert_eq!(
            app.docs.last().expect("new tab exists").line(0),
            "hello world",
            "content loaded"
        );
        assert!(app.prompt.is_none(), "prompt should be closed");
        assert!(
            app.status
                .starts_with(&i18n::t(Lang::En, Key::FileOpened).replace("{path}", "")),
            "status: {}",
            app.status
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_file_prompt_new_path_creates_untitled_with_path() {
        let dir = temp_dir("opennew");
        let path = dir.join("newfile.txt");
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        // Open the prompt
        app.on_key(ctrl(KeyCode::Char('o')));
        // Type a new path (file doesn't exist yet)
        let path_str = path.to_string_lossy().to_string();
        typed(&mut app, &path_str);
        // Confirm
        app.on_key(plain(KeyCode::Enter));
        // Should create an untitled-with-path tab (appends, doesn't switch focus)
        assert_eq!(app.docs.len(), 2, "new tab opened");
        let new_doc = app.docs.last().expect("new tab exists");
        assert_eq!(
            new_doc.path(),
            Some(path.as_path()),
            "new tab has the new path"
        );
        assert_eq!(new_doc.line_count(), 1, "new file has one empty line");
        assert!(app.prompt.is_none(), "prompt should be closed");
        assert!(
            app.status
                .starts_with(&i18n::t(Lang::En, Key::FileOpened).replace("{path}", "")),
            "status: {}",
            app.status
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_file_prompt_esc_cancels() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl(KeyCode::Char('o')));
        assert!(app.prompt.is_some());
        typed(&mut app, "/some/path");
        app.on_key(plain(KeyCode::Esc));
        assert!(app.prompt.is_none(), "Esc must close the prompt");
        assert_eq!(app.docs.len(), 1, "no new tab opened");
    }

    // -- Task 8: SaveAs prompt flow -----------------------------------

    #[test]
    fn save_as_flow_writes_file_and_adopts_path() {
        let dir = temp_dir("saveas");
        let path = dir.join("saveas.txt");
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "save as content");
        // Ctrl+S on modified untitled opens SaveAs prompt
        app.on_key(ctrl(KeyCode::Char('s')));
        assert_eq!(app.prompt, Some((PromptKind::SaveAs, String::new())));
        // Type the path
        let path_str = path.to_string_lossy().to_string();
        typed(&mut app, &path_str);
        // Confirm
        app.on_key(plain(KeyCode::Enter));
        // File should be written and tab should adopt the path
        assert!(app.doc().path().is_some(), "tab should have a path now");
        assert!(
            !app.doc().is_modified(),
            "save-as should clear modified flag"
        );
        let content = std::fs::read_to_string(&path).expect("file written");
        assert_eq!(content, "save as content\r\n");
        assert!(
            app.status
                .starts_with(&i18n::t(Lang::En, Key::SavedAs).replace("{path}", "")),
            "status: {}",
            app.status
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_as_prompt_esc_cancels() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "unsaved");
        // Ctrl+S on modified untitled opens SaveAs prompt
        app.on_key(ctrl(KeyCode::Char('s')));
        assert_eq!(app.prompt, Some((PromptKind::SaveAs, String::new())));
        typed(&mut app, "/some/path");
        app.on_key(plain(KeyCode::Esc));
        assert!(app.prompt.is_none(), "Esc must close the SaveAs prompt");
        assert!(app.doc().is_modified(), "content must be preserved");
        assert!(app.doc().path().is_none(), "path must not be adopted");
    }

    // -- Task 8: on_prompt_key state machine --------------------------

    #[test]
    fn on_prompt_key_chars_append() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.prompt = Some((PromptKind::OpenFile, String::new()));
        typed(&mut app, "/home/user/file.txt");
        assert_eq!(
            app.prompt,
            Some((PromptKind::OpenFile, "/home/user/file.txt".to_string()))
        );
    }

    #[test]
    fn on_prompt_key_backspace_deletes() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.prompt = Some((PromptKind::OpenFile, "hello".to_string()));
        app.on_key(plain(KeyCode::Backspace));
        assert_eq!(app.prompt, Some((PromptKind::OpenFile, "hell".to_string())));
    }

    #[test]
    fn on_prompt_key_enter_confirms() {
        let dir = temp_dir("promptenter");
        let path = dir.join("enter.txt");
        std::fs::write(&path, "loaded content").expect("pre-create");
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.prompt = Some((PromptKind::OpenFile, path.to_string_lossy().to_string()));
        app.on_key(plain(KeyCode::Enter));
        assert!(app.prompt.is_none(), "prompt should close on Enter");
        assert_eq!(app.docs.len(), 2, "file opened as new tab");
        assert_eq!(
            app.docs.last().expect("new tab exists").line(0),
            "loaded content"
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn on_prompt_key_esc_cancels() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.prompt = Some((PromptKind::SaveAs, "/some/path".to_string()));
        app.on_key(plain(KeyCode::Esc));
        assert!(app.prompt.is_none(), "Esc must cancel the prompt");
    }

    // -- Mouse tests ----------------------------------------------------

    fn setup_viewport(app: &mut App, h: usize, w: usize) {
        app.viewport_h = h;
        app.viewport_w = w;
    }

    #[test]
    fn left_click_mid_line_places_cursor() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        // Type "hello world" on line 0, cursor at col 11
        typed(&mut app, "hello world");
        // Click at column 6 (0-indexed), row 1 (first editor row) → buffer col 6
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            6,
            1,
            KeyModifiers::NONE,
        );
        assert_eq!(app.cursor_line, 0);
        assert_eq!(app.cursor_col, 6);
    }

    #[test]
    fn left_click_past_end_of_line_clamps_to_line_end() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        typed(&mut app, "hi");
        // Click far past end of line 0 → clamps to col 2
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            99,
            1,
            KeyModifiers::NONE,
        );
        assert_eq!(app.cursor_line, 0);
        assert_eq!(app.cursor_col, 2);
    }

    #[test]
    fn left_click_past_last_line_clamps_to_last_line_end() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        typed(&mut app, "hello");
        // Click row 50 (far past last line) → clamps to line 0 end
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            0,
            50,
            KeyModifiers::NONE,
        );
        assert_eq!(app.cursor_line, 0);
        assert_eq!(app.cursor_col, 5);
    }

    #[test]
    fn left_click_with_scroll_offset_maps_to_buffer_position() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 5, 40);
        // Type 10 lines of content
        for i in 0..10 {
            typed(&mut app, &format!("line{i}"));
            if i < 9 {
                app.on_key(plain(KeyCode::Enter));
            }
        }
        // Scroll down by 3 lines
        app.scroll_top = 3;
        // Click at editor row 2 (screen row 2, editor starts at row 1) → buffer line 3+2=5
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            0,
            3,
            KeyModifiers::NONE,
        );
        assert_eq!(app.cursor_line, 5);
    }

    #[test]
    fn click_tab_switches_active_document() {
        let mut app = App::new(
            vec![Document::new_untitled(), Document::new_untitled()],
            Lang::En,
        );
        // Tab bar is row 0. First tab occupies columns 0..len(" [Untitled] ").
        let first_tab_width = app.doc_title(0).chars().count();
        // Click in the second tab's column range
        let second_tab_col = first_tab_width as u16;
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            second_tab_col,
            0,
            KeyModifiers::NONE,
        );
        assert_eq!(app.active, 1);
    }

    #[test]
    fn tab_layout_ranges_match_rendered_title_spans() {
        let mut app = App::new(
            vec![Document::new_untitled(), Document::new_untitled()],
            Lang::En,
        );
        let layout = app.tab_layout();
        // The helper is consumed by draw_tabs (render) and tab_at_column
        // (hit-testing); ranges must match what the renderer draws.
        let mut col: u16 = 0;
        for (start, title, idx) in &layout {
            assert_eq!(*start, col, "tab {} start col", idx);
            assert_eq!(
                title.chars().count() as u16,
                col + title.chars().count() as u16 - *start,
                "tab {} width",
                idx
            );
            col += title.chars().count() as u16;
        }
    }

    #[test]
    fn left_click_with_horizontal_scroll_maps_buffer_col() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        // Type a long line so horizontal scroll is needed
        typed(&mut app, "abcdefghijklmnopqrstuvwxyz");
        // Set a non-zero horizontal scroll offset
        app.col_off = 5;
        // Click at screen column 3 → buffer col = 3 + 5 = 8
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            3,
            1,
            KeyModifiers::NONE,
        );
        assert_eq!(app.cursor_col, 8, "buffer col = screen col + col_off");
    }

    #[test]
    fn click_empty_tab_bar_space_does_nothing() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        let original_active = app.active;
        // Click far to the right of the tab bar (on the fill span)
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            99,
            0,
            KeyModifiers::NONE,
        );
        assert_eq!(
            app.active, original_active,
            "click on fill must not switch tabs"
        );
    }

    #[test]
    fn wheel_scroll_up_moves_exact_step() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        // Step is viewport_h - 1 = 9, same as PgUp/PgDn
        for i in 0..20 {
            typed(&mut app, &format!("line{i}"));
            if i < 19 {
                app.on_key(plain(KeyCode::Enter));
            }
        }
        // cursor_line must stay within viewport after scroll: set it
        // at the bottom edge of the resulting viewport (scroll_top=10).
        app.cursor_line = 19;
        app.scroll_top = 19;
        app.on_mouse(MouseEventKind::ScrollUp, 0, 5, KeyModifiers::NONE);
        assert_eq!(app.scroll_top, 10, "scroll_up must step by viewport_h - 1");
    }

    #[test]
    fn wheel_scroll_down_moves_exact_step() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        for i in 0..20 {
            typed(&mut app, &format!("line{i}"));
            if i < 19 {
                app.on_key(plain(KeyCode::Enter));
            }
        }
        // scroll_top=1 + step=9 = 10 = max_scroll (no clamp needed).
        // cursor_line=10 is within resulting viewport [10..20).
        app.cursor_line = 10;
        app.scroll_top = 1;
        app.on_mouse(MouseEventKind::ScrollDown, 0, 5, KeyModifiers::NONE);
        assert_eq!(
            app.scroll_top, 10,
            "scroll_down must step by viewport_h - 1"
        );
    }

    #[test]
    fn wheel_scroll_up_partial_step_clamped_at_zero() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        // Near top: scroll_top (3) < step (9), so clamps to 0 — partial step
        app.scroll_top = 3;
        app.on_mouse(MouseEventKind::ScrollUp, 0, 5, KeyModifiers::NONE);
        assert_eq!(app.scroll_top, 0, "scroll_up near top clamps at zero");
    }

    #[test]
    fn wheel_scroll_down_clamped_at_max() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        for i in 0..20 {
            typed(&mut app, &format!("line{i}"));
            if i < 19 {
                app.on_key(plain(KeyCode::Enter));
            }
        }
        // Step past max, then verify clamp
        let max_scroll = app.doc().line_count().saturating_sub(app.viewport_h);
        app.scroll_top = max_scroll + 5;
        app.on_mouse(MouseEventKind::ScrollDown, 0, 5, KeyModifiers::NONE);
        assert_eq!(app.scroll_top, max_scroll, "scroll_down must clamp at max");
    }

    #[test]
    fn click_below_viewport_ignored() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        typed(&mut app, "hello");
        let original_line = app.cursor_line;
        // Click below the editor viewport (row >= 1 + viewport_h)
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            0,
            15,
            KeyModifiers::NONE,
        );
        assert_eq!(
            app.cursor_line, original_line,
            "click below viewport must be ignored"
        );
    }

    #[test]
    fn right_click_ignored() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        typed(&mut app, "hello");
        let original_line = app.cursor_line;
        let original_col = app.cursor_col;
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Right),
            5,
            1,
            KeyModifiers::NONE,
        );
        assert_eq!(
            app.cursor_line, original_line,
            "right click must not move cursor"
        );
        assert_eq!(
            app.cursor_col, original_col,
            "right click must not move cursor"
        );
    }

    #[test]
    fn click_brings_offscreen_cursor_onscreen() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 5, 40);
        // Type 10 lines
        for i in 0..10 {
            typed(&mut app, &format!("line{i}"));
            if i < 9 {
                app.on_key(plain(KeyCode::Enter));
            }
        }
        // Cursor is at line 0, scroll_top is 0 (showing lines 0-4)
        // Scroll down manually to 8 so lines 8-9 are visible but line 0 is off-screen
        app.scroll_top = 8;
        // Click at the top of the viewport (row 1, editor_row 0) → buffer line 8
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            0,
            1,
            KeyModifiers::NONE,
        );
        // Cursor should now be at line 8
        assert_eq!(app.cursor_line, 8);
        // Now click above the viewport at row 1 again but with scroll_top=8
        // This maps to buffer line 8, which is on-screen, so no adjustment needed
        // To test "bringing on-screen": set cursor far away, then click within viewport
        app.cursor_line = 15;
        app.scroll_top = 8;
        // Click at editor row 0 → buffer line 8, cursor moves to 8
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            0,
            1,
            KeyModifiers::NONE,
        );
        assert_eq!(app.cursor_line, 8);
        // clamp_scroll should have adjusted scroll_top since cursor (8) is now
        // within the viewport (8..13), so scroll_top stays 8
        assert_eq!(app.scroll_top, 8);
    }

    // -- Status bar content tests ----------------------------------

    #[test]
    fn status_line_shows_untitled_for_pathless_doc() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.status.clear();
        let (left, _) = app.status_line(80);
        assert!(left.contains("[Untitled]"), "left: {left}");
    }

    #[test]
    fn status_line_shows_filename_for_pathed_doc() {
        let dir = temp_dir("statusbar");
        let path = dir.join("main.rs");
        let mut app = App::new(vec![Document::new_with_path(&path)], Lang::En);
        app.status.clear();
        let (left, _) = app.status_line(80);
        assert!(left.contains("main.rs"), "left: {left}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_line_includes_lncol() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.status.clear();
        app.cursor_line = 2;
        app.cursor_col = 3;
        let (left, _) = app.status_line(80);
        assert!(left.contains("Ln 3, Col 4"), "left: {left}");
    }

    #[test]
    fn status_line_shows_overwrite_indicator() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.status.clear();
        app.overwrite = true;
        let (left, _) = app.status_line(80);
        assert!(left.contains("OVR"), "left: {left}");
    }

    #[test]
    fn status_line_omits_overwrite_when_off() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.status.clear();
        let (left, _) = app.status_line(80);
        assert!(!left.contains("OVR"), "left: {left}");
    }

    #[test]
    fn status_line_shows_read_only_marker() {
        let mut doc = Document::new_with_path(std::path::Path::new("/tmp/big.bin"));
        doc.set_read_only();
        let mut app = App::new(vec![doc], Lang::En);
        app.status.clear();
        let (left, _) = app.status_line(80);
        assert!(left.contains("read-only"), "left: {left}");
    }

    #[test]
    fn status_line_shows_language_indicator() {
        let dir = temp_dir("lang");
        let path = dir.join("lib.rs");
        std::fs::write(&path, "fn main() {}").expect("pre-create");
        let app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        let (_, right) = app.status_line(80);
        assert!(right.contains("Rust"), "right: {right}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_line_shows_plain_text_for_unknown_extension() {
        let dir = temp_dir("plain");
        let path = dir.join("data.xyz");
        std::fs::write(&path, "hello").expect("pre-create");
        let app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        let (_, right) = app.status_line(80);
        assert!(right.contains("Plain text"), "right: {right}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_line_shows_plain_text_for_untitled() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.status.clear();
        let (_, right) = app.status_line(80);
        assert!(right.contains("Plain text"), "right: {right}");
    }

    #[test]
    fn status_line_truncates_long_filename_with_ellipsis() {
        let dir = temp_dir("trunc");
        let long_name = "a".repeat(200);
        let path = dir.join(format!("{long_name}.rs"));
        std::fs::write(&path, "fn main() {}").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.status.clear();
        let (left, _) = app.status_line(20);
        // With a 20-char-wide viewport the left segment must be
        // shorter than the full (very long) title and must end with ellipsis.
        assert!(left.len() < 20, "left len: {}", left.len());
        assert!(left.ends_with("…"), "left should end with ellipsis: {left}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_line_right_side_includes_transient_status() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.status = "Saved".to_string();
        let (_, right) = app.status_line(80);
        assert!(right.contains("Saved"), "right: {right}");
    }

    #[test]
    fn status_line_right_side_includes_language() {
        let dir = temp_dir("langright");
        let path = dir.join("readme.md");
        std::fs::write(&path, "# Hi").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.status.clear();
        let (_, right) = app.status_line(80);
        assert!(right.contains("Markdown"), "right: {right}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_line_spanish_plain_text() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::Es);
        app.status.clear();
        let (_, right) = app.status_line(80);
        assert!(right.contains("Texto sin formato"), "right: {right}");
    }
}
