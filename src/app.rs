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
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::command::{Command, CommandKind, all_commands};
use crate::config::ThemeChoice;
use crate::document::{Document, DocumentError, EditRecord};
use crate::fuzzy;
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
    /// Replace all occurrences.
    /// Second element is the number of documents affected (1 for active-doc, >1 for cross-doc).
    ReplaceAll(usize, usize),
}

/// Whether the search/replace dialog is in find mode or replace mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    Find,
    Replace,
}

/// Which input field has focus in the replace dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplaceField {
    Query,
    Replacement,
}

/// What a pending text-input prompt is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// Open a file from disk.
    OpenFile,
    /// Save an untitled document to a new path.
    SaveAs,
    /// Jump the cursor to a line number.
    GotoLine,
}

/// Search options struct, present for future toggles (case-sensitive,
/// regex) but no UI toggles yet in this task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub is_regex: bool,
}

/// What the palette modal is listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKind {
    /// Command palette (Ctrl+Shift+P / F4).
    Command,
    /// Quick-open file picker (Ctrl+P).
    QuickOpen,
}

/// One selectable row of the palette.
#[derive(Debug, Clone)]
pub enum PaletteEntry {
    /// A registry command to execute.
    Command(Command),
    /// An already-open tab to focus.
    Tab(usize),
    /// A file on disk to open as a new tab.
    File(PathBuf),
}

/// Command palette / quick-open modal state (milestone 2).
#[derive(Debug)]
pub struct PaletteState {
    /// What the palette lists.
    pub kind: PaletteKind,
    /// Typed filter text.
    pub query: String,
    /// Filtered rows in ranked (best-first) order.
    pub entries: Vec<PaletteEntry>,
    /// Highlighted row within `entries`.
    pub selected: usize,
}

impl PaletteState {
    /// Open a fresh palette of `kind` with an empty query. Entries are
    /// filled by `App::refresh_palette`, which knows the tab/dir context.
    pub fn new(kind: PaletteKind) -> Self {
        Self {
            kind,
            query: String::new(),
            entries: Vec::new(),
            selected: 0,
        }
    }
}

/// Per-match position within a line (char offsets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MatchPos {
    line: usize,
    start_col: usize,
    end_col: usize,
    /// Index of the document this match belongs to.
    doc_index: usize,
}

/// State of the active search/replace dialog.
pub struct SearchState {
    query: String,
    replacement: String,
    matches: Vec<MatchPos>,
    /// Index into `matches` of the current match, or `None` if no
    /// navigation has happened yet (or no matches exist).
    current_idx: Option<usize>,
    pre_search_line: usize,
    pre_search_col: usize,
    /// True once the user has navigated to at least one match.
    navigated: bool,
    mode: SearchMode,
    field_focus: ReplaceField,
    /// True when searching/replacing across all open documents.
    cross_doc: bool,
    /// Document index where the search was initiated (for Esc restore).
    start_doc: usize,
}

impl SearchState {
    fn new_find(query: String, matches: Vec<MatchPos>) -> Self {
        SearchState {
            query,
            replacement: String::new(),
            matches,
            current_idx: None,
            pre_search_line: 0,
            pre_search_col: 0,
            navigated: false,
            mode: SearchMode::Find,
            field_focus: ReplaceField::Query,
            cross_doc: false,
            start_doc: 0,
        }
    }

    fn new_replace(query: String, matches: Vec<MatchPos>) -> Self {
        SearchState {
            query,
            replacement: String::new(),
            matches,
            current_idx: None,
            pre_search_line: 0,
            pre_search_col: 0,
            navigated: false,
            mode: SearchMode::Replace,
            field_focus: ReplaceField::Query,
            cross_doc: false,
            start_doc: 0,
        }
    }
}

/// Cursor and scroll state saved per tab while another tab is focused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DocView {
    pub line: usize,
    pub col: usize,
    pub scroll_top: usize,
    pub col_off: usize,
}

/// Selection state per tab: anchor is fixed, head follows the cursor
/// during Shift+movement. When `None` there is no active selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: (usize, usize),
    pub head: (usize, usize),
}

impl Selection {
    /// Returns `(start, end)` ordered lexicographically by (line, col).
    fn normalized(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

/// Maximum undo steps kept per tab (oldest dropped first).
pub const UNDO_LIMIT: usize = 100;

/// Rows the interactive Ctrl+O list shows at once; the PageUp/PageDown step.
/// Visible rows in the Ctrl+O folder browser popup. Larger than a single
/// screen page so deep directories scroll naturally instead of clipping.
const OPEN_LIST_PAGE: usize = 15;

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
    /// Selection state per tab, index-aligned with `docs`.
    pub selections: Vec<Option<Selection>>,
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
    /// Interactive Ctrl+O candidate files (dir of the active document,
    /// filtered by the typed input). Empty for non-open prompts.
    pub open_candidates: Vec<PathBuf>,
    /// Highlighted row within `open_candidates`.
    pub open_selected: usize,
    /// Directory the Ctrl+O list is browsing (None = the active document's
    /// directory or cwd). Entering folders sets it; Esc/close resets it.
    pub open_dir: Option<PathBuf>,
    /// Pending search dialog state.
    pub search: Option<SearchState>,
    /// Search options struct (present for future toggles, no UI yet).
    pub search_options: SearchOptions,
    /// Command palette / quick-open modal (milestone 2).
    pub palette: Option<PaletteState>,
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
        let selections = (0..docs.len()).map(|_| None).collect();
        App {
            docs,
            tree: PaneTree::single(0),
            active: 0,
            views,
            histories,
            selections,
            cursor_line: 0,
            cursor_col: 0,
            scroll_top: 0,
            col_off: 0,
            viewport_h: 1,
            viewport_w: 1,
            quit: false,
            confirm: None,
            prompt: None,
            open_candidates: Vec::new(),
            open_selected: 0,
            open_dir: None,
            search: None,
            search_options: SearchOptions::default(),
            palette: None,
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

    // -- Selection helpers -----------------------------------------------

    fn clear_selection(&mut self) {
        self.selections[self.active] = None;
    }

    /// Delete the active selection (when non-empty) as one undo step,
    /// leaving the cursor at its start. Returns true when a selection was
    /// deleted; false means there was nothing selected and the caller
    /// should fall back to its single-character behavior.
    fn delete_selected_range(&mut self) -> bool {
        let Some(((sl, sc), (el, ec))) = self
            .normalized_selection()
            .filter(|((sl, sc), (el, ec))| sl != el || sc != ec)
        else {
            return false;
        };
        match self.doc_mut().delete_range(sl, sc, el, ec) {
            Ok(record) => {
                self.record_edits(vec![record], false);
                self.cursor_line = sl;
                self.cursor_col = sc;
                self.clear_selection();
                self.clamp_cursor();
                self.clamp_scroll();
                true
            }
            Err(_) => false,
        }
    }

    fn start_selection(&mut self) {
        if self.selections[self.active].is_none() {
            self.selections[self.active] = Some(Selection {
                anchor: (self.cursor_line, self.cursor_col),
                head: (self.cursor_line, self.cursor_col),
            });
        }
    }

    fn update_selection_head(&mut self) {
        if let Some(ref mut sel) = self.selections[self.active] {
            sel.head = (self.cursor_line, self.cursor_col);
        }
    }

    fn normalized_selection(&self) -> Option<((usize, usize), (usize, usize))> {
        self.selections[self.active].map(|sel| sel.normalized())
    }

    /// Character-column range of the selection on `buf_line`, or `None`.
    fn selection_range_for_line(&self, buf_line: usize) -> Option<(usize, usize)> {
        let ((start_line, start_col), (end_line, end_col)) = self.normalized_selection()?;
        if buf_line < start_line || buf_line > end_line {
            return None;
        }
        if buf_line == start_line && buf_line == end_line {
            Some((start_col, end_col))
        } else if buf_line == start_line {
            Some((start_col, self.doc().line_len(buf_line)))
        } else if buf_line == end_line {
            Some((0, end_col))
        } else {
            Some((0, self.doc().line_len(buf_line)))
        }
    }

    /// Apply selection background to spans, without overriding search-match bg.
    fn apply_selection_bg(
        &self,
        spans: Vec<Span<'static>>,
        sel_start: usize,
        sel_end: usize,
    ) -> Vec<Span<'static>> {
        if sel_start >= sel_end {
            return spans;
        }
        let selection_bg = self.theme.selection_bg;
        let mut result = Vec::new();
        let mut char_pos = 0;
        for span in spans {
            let text = span.content.as_ref();
            let len = text.chars().count();
            let span_start = char_pos;
            let span_end = char_pos + len;
            if span_end <= sel_start || span_start >= sel_end {
                result.push(span);
            } else {
                let overlap_start = span_start.max(sel_start);
                let overlap_end = span_end.min(sel_end);
                if span_start < sel_start {
                    let before_len = sel_start - span_start;
                    let before_text: String = text.chars().take(before_len).collect();
                    result.push(Span::styled(before_text, span.style));
                }
                let sel_text_len = overlap_end - overlap_start;
                let sel_text: String = text
                    .chars()
                    .skip(overlap_start - span_start)
                    .take(sel_text_len)
                    .collect();
                result.push(Span::styled(sel_text, span.style.bg(selection_bg)));
                if span_end > sel_end {
                    let after_start = sel_end - span_start;
                    let after_text: String = text.chars().skip(after_start).collect();
                    result.push(Span::styled(after_text, span.style));
                }
            }
            char_pos += len;
        }
        result
    }

    // -- Clipboard helpers ---------------------------------------------

    /// Materialize the normalized selection as a String using char-based
    /// slicing per line (first line from start_col to line end, middle
    /// lines whole, last line 0..end_col, joined with "\n").
    fn build_selection_text(
        &self,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> String {
        let doc = self.doc();
        if start_line == end_line {
            let line = doc.line(start_line);
            line.chars()
                .skip(start_col)
                .take(end_col - start_col)
                .collect()
        } else {
            let mut parts = Vec::new();
            parts.push(doc.line(start_line).chars().skip(start_col).collect());
            for i in (start_line + 1)..end_line {
                parts.push(doc.line(i).to_string());
            }
            parts.push(doc.line(end_line).chars().take(end_col).collect());
            parts.join("\n")
        }
    }

    /// Copy text to the system clipboard. On failure, sets status to the
    /// i18n ClipboardError key instead of panicking.
    fn copy_to_clipboard(&mut self, text: &str) {
        if arboard::Clipboard::new()
            .and_then(|mut clip| clip.set_text(text))
            .is_err()
        {
            self.status = i18n::t(self.lang, Key::ClipboardError).to_string();
        }
    }

    /// Copy the current selection (or current line without trailing
    /// newline when no selection is active) to the system clipboard.
    /// VS Code includes the trailing newline when copying a line without
    /// selection; this editor does not (documented deviation).
    fn copy_selection_or_line(&mut self) {
        let text = if let Some(((sl, sc), (el, ec))) = self.normalized_selection() {
            self.build_selection_text(sl, sc, el, ec)
        } else {
            // No selection: copy the current line without its trailing
            // newline.  VS Code includes it; this is a deliberate deviation.
            self.doc().line(self.cursor_line).to_string()
        };
        self.copy_to_clipboard(&text);
    }

    /// Cut the current selection (or current line when no selection) to
    /// the clipboard and delete it from the document.  Both the copy
    /// and the delete form ONE undo step.
    fn cut_selection_or_line(&mut self) {
        let text = if let Some(((sl, sc), (el, ec))) = self.normalized_selection() {
            self.build_selection_text(sl, sc, el, ec)
        } else {
            // No selection: copy the current line without its trailing
            // newline (same deviation as copy).  The delete below removes
            // the whole line including its line break.
            self.doc().line(self.cursor_line).to_string()
        };
        self.copy_to_clipboard(&text);
        self.perform_cut();
    }

    /// Execute the delete portion of a cut operation.  A non-empty selection
    /// is deleted as-is; otherwise the whole line (including its line break)
    /// is removed.  A collapsed selection counts as no selection (VS Code
    /// treats Ctrl+X with an empty selection as cut-line).
    fn perform_cut(&mut self) {
        let record = if let Some(((sl, sc), (el, ec))) = self
            .normalized_selection()
            .filter(|((sl, sc), (el, ec))| sl != el || sc != ec)
        {
            self.doc_mut().delete_range(sl, sc, el, ec)
        } else {
            let line = self.cursor_line;
            let line_len = self.doc().line_len(line);
            if line + 1 < self.doc().line_count() {
                // Delete the line including its line break by joining with
                // the next line.
                self.doc_mut().delete_range(line, 0, line + 1, 0)
            } else {
                // Last line: just clear its content.
                self.doc_mut().delete_range(line, 0, line, line_len)
            }
        };
        match record {
            Ok(r) => {
                self.record_edits(vec![r], false);
                self.clear_selection();
                self.clamp_cursor();
            }
            Err(e) => self.status = i18n::error_message(self.lang, &e),
        }
    }

    /// Paste clipboard text at the cursor.  Normalizes "\r\n" and "\r"
    /// to "\n" before inserting.  If a selection is active it is deleted
    /// first; the delete+insert records form ONE undo step.  Cursor
    /// lands at the end of the pasted text.
    fn paste_from_clipboard(&mut self) {
        let clipboard_text = match arboard::Clipboard::new().and_then(|mut clip| clip.get_text()) {
            Ok(text) => text,
            Err(_) => {
                self.status = i18n::t(self.lang, Key::ClipboardError).to_string();
                return;
            }
        };
        // Normalize CRLF and lone CR to LF before inserting.
        let normalized = clipboard_text.replace("\r\n", "\n").replace('\r', "\n");
        let mut records = Vec::new();
        // If a selection is active, delete it first.
        if let Some(((sl, sc), (el, ec))) = self.normalized_selection() {
            match self.doc_mut().delete_range(sl, sc, el, ec) {
                Ok(record) => {
                    records.push(record);
                    self.cursor_line = sl;
                    self.cursor_col = sc;
                    self.clear_selection();
                }
                Err(e) => {
                    self.status = i18n::error_message(self.lang, &e);
                    return;
                }
            }
        }
        if let Some(record) = self.do_insert(&normalized) {
            records.push(record);
        }
        if !records.is_empty() {
            self.record_edits(records, false);
        }
        self.clamp_cursor();
    }

    // -- Word movement helpers -------------------------------------------

    fn is_word_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn char_class(c: char) -> usize {
        if Self::is_word_char(c) {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    }

    /// Move cursor one word to the right (VS Code boundaries).
    fn move_word_right(&mut self) {
        let doc = self.doc();
        let (mut line, mut col) = (self.cursor_line, self.cursor_col);
        // Skip the word/punctuation run under the cursor, if any.
        if col < doc.line_len(line) {
            let cls = Self::char_class(doc.line(line).chars().nth(col).unwrap());
            if cls != 1 {
                while col < doc.line_len(line)
                    && Self::char_class(doc.line(line).chars().nth(col).unwrap()) == cls
                {
                    col += 1;
                }
            }
        }
        // Skip whitespace; the line break itself is a word boundary, so the
        // crossing stops at the arrival column (VS Code: EOL → next line col 0
        // when the next line starts with a word char).
        loop {
            while col < doc.line_len(line)
                && doc.line(line).chars().nth(col).unwrap().is_whitespace()
            {
                col += 1;
            }
            if col >= doc.line_len(line) && line + 1 < doc.line_count() {
                line += 1;
                col = 0;
                continue;
            }
            break;
        }
        self.cursor_line = line;
        self.cursor_col = col;
        self.clamp_scroll();
    }

    /// Move cursor one word to the left (VS Code boundaries).
    fn move_word_left(&mut self) {
        let doc = self.doc();
        let mut line = self.cursor_line;
        let mut col = self.cursor_col;
        if col == 0 {
            if line == 0 {
                self.clamp_scroll();
                return;
            }
            line -= 1;
            col = doc.line_len(line);
        }
        // Skip the current token backwards
        let c = doc.line(line).chars().nth(col - 1).unwrap();
        let cls = Self::char_class(c);
        while col > 0 {
            let ch = doc.line(line).chars().nth(col - 1).unwrap();
            if Self::char_class(ch) == cls {
                col -= 1;
            } else {
                break;
            }
        }
        // Skip whitespace backwards, crossing line boundaries
        loop {
            while col > 0 {
                let ch = doc.line(line).chars().nth(col - 1).unwrap();
                if ch.is_whitespace() {
                    col -= 1;
                } else {
                    break;
                }
            }
            if col == 0 && line > 0 {
                line -= 1;
                col = doc.line_len(line);
                continue;
            } else {
                break;
            }
        }
        self.cursor_line = line;
        self.cursor_col = col;
        self.clamp_scroll();
    }

    // -- Selection movement ---------------------------------------------

    fn ctrl_shift_home_end(&mut self, to_start: bool) {
        self.start_selection();
        if to_start {
            self.cursor_line = 0;
            self.cursor_col = 0;
        } else {
            self.cursor_line = self.doc().line_count().saturating_sub(1);
            self.cursor_col = self.doc().line_len(self.cursor_line);
        }
        self.clamp_cursor();
        self.clamp_scroll();
        self.update_selection_head();
    }

    fn ctrl_home_end(&mut self, to_start: bool) {
        self.clear_selection();
        if to_start {
            self.cursor_line = 0;
            self.cursor_col = 0;
        } else {
            self.cursor_line = self.doc().line_count().saturating_sub(1);
            self.cursor_col = self.doc().line_len(self.cursor_line);
        }
        self.clamp_cursor();
        self.clamp_scroll();
    }

    fn ctrl_shift_word_left(&mut self) {
        self.start_selection();
        self.move_word_left();
        self.update_selection_head();
    }

    fn ctrl_shift_word_right(&mut self) {
        self.start_selection();
        self.move_word_right();
        self.update_selection_head();
    }

    fn shift_left(&mut self) {
        self.start_selection();
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.doc().line_len(self.cursor_line);
        }
        self.clamp_scroll();
        self.update_selection_head();
    }

    fn shift_right(&mut self) {
        self.start_selection();
        if self.cursor_col < self.doc().line_len(self.cursor_line) {
            self.cursor_col += 1;
        } else if self.cursor_line + 1 < self.doc().line_count() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
        self.clamp_scroll();
        self.update_selection_head();
    }

    fn shift_up(&mut self) {
        self.start_selection();
        self.cursor_line = self.cursor_line.saturating_sub(1);
        self.clamp_cursor();
        self.clamp_scroll();
        self.update_selection_head();
    }

    fn shift_down(&mut self) {
        self.start_selection();
        self.cursor_line += 1;
        self.clamp_cursor();
        self.clamp_scroll();
        self.update_selection_head();
    }

    fn shift_home(&mut self) {
        self.start_selection();
        self.cursor_col = 0;
        self.clamp_scroll();
        self.update_selection_head();
    }

    fn shift_end(&mut self) {
        self.start_selection();
        self.cursor_col = self.doc().line_len(self.cursor_line);
        self.clamp_scroll();
        self.update_selection_head();
    }

    fn shift_page_up(&mut self) {
        self.start_selection();
        self.cursor_line = self
            .cursor_line
            .saturating_sub(self.viewport_h.saturating_sub(1));
        self.clamp_cursor();
        self.clamp_scroll();
        self.update_selection_head();
    }

    fn shift_page_down(&mut self) {
        self.start_selection();
        self.cursor_line += self.viewport_h.saturating_sub(1);
        self.clamp_cursor();
        self.clamp_scroll();
        self.update_selection_head();
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
        self.clear_selection();
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
        self.clear_selection();
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
        self.clear_selection();
        if let Some(record) = self.do_insert(text) {
            self.record_edits(vec![record], continues_burst);
        }
        self.clamp_cursor();
    }

    /// Type one printable key. In overwrite mode (Insert), the character under
    /// the cursor is replaced instead of shifting text right; the replacement
    /// (delete + insert) is ONE undo step per keystroke.
    fn type_char(&mut self, c: char) {
        self.clear_selection();
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
        self.clear_selection();
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
        self.clear_selection();
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
        self.open_dir = None;
        self.refresh_open_candidates();
        self.open_selected = 0;
    }

    /// Ctrl+G: prompt for a 1-based line number.
    fn open_goto_line_prompt(&mut self) {
        self.prompt = Some((PromptKind::GotoLine, String::new()));
    }

    /// Ctrl+Shift+K: delete the whole current line (VS Code semantics:
    /// the column is preserved, clamped to the replacement line). With an
    /// active selection, ALL lines the selection spans are deleted.
    fn delete_line(&mut self) {
        if let Some(((sl, _), (el, _))) = self.normalized_selection() {
            let record = if el + 1 < self.doc().line_count() {
                self.doc_mut().delete_range(sl, 0, el + 1, 0)
            } else if sl > 0 {
                let prev_len = self.doc().line_len(sl - 1);
                let last_len = self.doc().line_len(el);
                self.doc_mut().delete_range(sl - 1, prev_len, el, last_len)
            } else {
                // The selection covers every line: clear the document.
                let last_len = self.doc().line_len(el);
                self.doc_mut().delete_range(sl, 0, el, last_len)
            };
            match record {
                Ok(r) => {
                    self.record_edits(vec![r], false);
                    self.clear_selection();
                    self.cursor_line = sl.min(self.doc().line_count().saturating_sub(1));
                    self.clamp_cursor();
                    self.clamp_scroll();
                }
                Err(e) => self.status = i18n::error_message(self.lang, &e),
            }
            return;
        }
        let line = self.cursor_line;
        let record = if line + 1 < self.doc().line_count() {
            // Join with the next line: the break disappears with the line.
            self.doc_mut().delete_range(line, 0, line + 1, 0)
        } else if line > 0 {
            // Last line: remove the preceding break instead.
            let prev_len = self.doc().line_len(line - 1);
            let cur_len = self.doc().line_len(line);
            self.doc_mut()
                .delete_range(line - 1, prev_len, line, cur_len)
        } else {
            // Only line: clear its content.
            let cur_len = self.doc().line_len(0);
            self.doc_mut().delete_range(0, 0, 0, cur_len)
        };
        match record {
            Ok(r) => {
                self.record_edits(vec![r], false);
                self.clamp_cursor();
                self.clamp_scroll();
            }
            Err(e) => self.status = i18n::error_message(self.lang, &e),
        }
    }

    /// Alt+Up/Down: move the current line — or the whole selected block —
    /// one position up/down. The selection (and cursor) travel with the
    /// block; the whole move is ONE undo step.
    fn move_lines(&mut self, delta: isize) {
        let (block_start, block_end, sel_cols) = if let Some(((sl, sc), (el, ec))) = self
            .normalized_selection()
            .filter(|((sl, sc), (el, ec))| sl != el || sc != ec)
        {
            (sl, el, Some((sc, ec)))
        } else {
            (self.cursor_line, self.cursor_line, None)
        };
        if !self.move_line_block(block_start, block_end, delta) {
            return;
        }
        let new_start = (block_start as isize + delta).max(0) as usize;
        let new_end = (block_end as isize + delta).max(0) as usize;
        match sel_cols {
            Some((sc, ec)) => {
                self.selections[self.active] = Some(Selection {
                    anchor: (new_start, sc),
                    head: (new_end, ec),
                });
                self.cursor_line = new_end;
                self.cursor_col = ec;
            }
            None => {
                self.cursor_line = new_start;
            }
        }
        self.clamp_cursor();
        self.clamp_scroll();
    }

    /// Swap the line block `s..=e` with its neighbour in `delta`
    /// direction. Returns false at document boundaries.
    fn move_line_block(&mut self, s: usize, e: usize, delta: isize) -> bool {
        let last = self.doc().line_count().saturating_sub(1);
        if (delta < 0 && s == 0) || (delta > 0 && e == last) {
            return false;
        }
        let block_text = (s..=e)
            .map(|i| self.doc().line(i).to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let mut records = Vec::new();
        if delta < 0 {
            let end_len = self.doc().line_len(e);
            if e + 1 < self.doc().line_count() {
                if let Ok(r) = self.doc_mut().delete_range(s, 0, e + 1, 0) {
                    records.push(r);
                }
            } else if let Ok(r) = self.doc_mut().delete_range(s, 0, e, end_len) {
                records.push(r);
            }
            if let Ok(r) = self.doc_mut().insert(s - 1, 0, &format!("{block_text}\n")) {
                records.push(r);
            }
        } else {
            if let Ok(r) = self.doc_mut().delete_range(s, 0, e + 1, 0) {
                records.push(r);
            }
            // After the deletion the line that followed the block sits at
            // index s: append the block after it.
            let insert_col = self.doc().line_len(s);
            if let Ok(r) = self
                .doc_mut()
                .insert(s, insert_col, &format!("\n{block_text}"))
            {
                records.push(r);
            }
        }
        if !records.is_empty() {
            self.record_edits(records, false);
        }
        true
    }

    /// Recompute the interactive Ctrl+O list: files in the active
    /// document's directory (cwd when untitled) whose name contains the
    /// typed input, case-insensitively, capped at 50 entries.
    fn refresh_open_candidates(&mut self) {
        let Some((PromptKind::OpenFile, input)) = &self.prompt else {
            return;
        };
        let input = input.clone();
        let dir = self.browsing_dir();
        self.open_candidates = Self::compute_open_browser_entries(&dir, &input);
        self.open_selected = 0;
    }

    /// Directory the open-list is browsing: the explicit navigation root
    /// when set, otherwise the active document's directory (cwd untitled).
    fn browsing_dir(&self) -> PathBuf {
        self.open_dir.clone().unwrap_or_else(|| {
            self.doc()
                .path()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        })
    }

    /// Entries for the interactive open-list browser: `..` (only while the
    /// filter is empty, so a typed path keeps winning when nothing
    /// matches), then subdirectories, then files — names containing
    /// `filter` case-insensitively, each group sorted, capped at 50.
    fn compute_open_browser_entries(dir: &std::path::Path, filter: &str) -> Vec<PathBuf> {
        let mut entries: Vec<PathBuf> = Vec::new();
        if filter.is_empty() && dir.parent().is_some() {
            entries.push(PathBuf::from(".."));
        }
        let filter_lc = filter.to_lowercase();
        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut files: Vec<PathBuf> = Vec::new();
        for entry in std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            let matches = path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase().contains(&filter_lc))
                .unwrap_or(false);
            if !matches {
                continue;
            }
            if path.is_dir() {
                dirs.push(path);
            } else if path.is_file() {
                files.push(path);
            }
        }
        dirs.sort();
        files.sort();
        entries.append(&mut dirs);
        entries.append(&mut files);
        entries.truncate(50);
        entries
    }

    /// Files in `dir` whose file name contains `filter`
    /// (case-insensitive), sorted by name, capped at 50.
    fn compute_open_candidates(dir: &std::path::Path, filter: &str) -> Vec<PathBuf> {
        let filter_lc = filter.to_lowercase();
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|p| p.is_file())
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().to_lowercase().contains(&filter_lc))
                    .unwrap_or(false)
            })
            .collect();
        files.sort();
        files.truncate(50);
        files
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
        self.selections.remove(index);
        if self.docs.is_empty() {
            self.docs.push(Document::new_untitled());
            self.views.push(DocView::default());
            self.histories.push(History::new());
            self.selections.push(None);
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
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        // The palette consumes everything while it is open; other modals
        // never coexist with it (it only opens with none active).
        if self.palette.is_some() {
            self.on_palette_key(key);
            return;
        }
        // Ctrl+H opens the replace dialog (even when search is active).
        // Ctrl+Shift+H opens cross-doc replace.
        if ctrl && matches!(key.code, KeyCode::Char('h' | 'H')) {
            if shift {
                self.open_cross_doc_replace();
            } else {
                self.open_replace();
            }
            return;
        }
        // Ctrl+Shift+F opens cross-doc find.
        if ctrl && shift && matches!(key.code, KeyCode::Char('f' | 'F')) {
            self.open_cross_doc_search();
            return;
        }
        if self.confirm.is_some() {
            self.on_confirm_key(key.code);
            return;
        }
        if self.search.is_some() {
            self.on_search_key(key);
            return;
        }
        if self.prompt.is_some() {
            self.on_prompt_key(key);
            return;
        }
        match (key.code, ctrl, shift) {
            // Ctrl+Shift+S → save all (must precede plain Ctrl+S)
            (KeyCode::Char('s' | 'S'), true, true) => self.save_all(),
            // Ctrl+S → save active
            (KeyCode::Char('s' | 'S'), true, _) => self.save_active(),
            // F2 → save active
            (KeyCode::F(2), _, _) => self.save_active(),
            // F3 → save all (fallback for Ctrl+Shift+S on Windows)
            (KeyCode::F(3), _, _) => self.save_all(),
            // F10 → quit (terminal-proof fallback for Ctrl+Q)
            (KeyCode::F(10), _, _) => self.request_quit(),
            // F8 → delete the current line (fallback for Ctrl+Shift+K when
            // the terminal/PTY strips the modifiers)
            (KeyCode::F(8), _, _) => self.delete_line(),
            // Ctrl+Shift+P → command palette (must precede the generic
            // Ctrl+P quick-open arm that milestone 2 T4 adds)
            (KeyCode::Char('p' | 'P'), true, true) => self.open_palette(),
            // F4 → command palette (terminal-proof fallback; F2/F3/F5/F6/F8
            // precedent)
            (KeyCode::F(4), _, _) => self.open_palette(),
            // Ctrl+P → quick-open fuzzy picker
            (KeyCode::Char('p' | 'P'), true, _) => self.open_quick_open(),
            // Ctrl+O → open file prompt
            (KeyCode::Char('o' | 'O'), true, _) => self.open_file_prompt(),
            (KeyCode::Char('f' | 'F'), true, _) => self.open_search(),
            (KeyCode::Char('q'), true, _) => self.request_quit(),
            (KeyCode::Char('w'), true, _) => self.request_close(),
            (KeyCode::Char('z'), true, _) => self.undo(),
            (KeyCode::Char('Z'), true, _) => self.redo(),
            (KeyCode::Char('y'), true, _) => self.redo(),
            (KeyCode::Char('1'), true, _) => self.last_doc_of_panel(1),
            (KeyCode::Char('2'), true, _) => self.last_doc_of_panel(2),
            (KeyCode::PageDown, true, _) => self.next_doc(1),
            (KeyCode::PageUp, true, _) => self.next_doc(-1),
            // Ctrl+Insert → copy (Windows-classic fallback)
            (KeyCode::Insert, true, _) => self.copy_selection_or_line(),
            // Shift+Insert → paste (Windows-classic fallback)
            (KeyCode::Insert, _, true) => self.paste_from_clipboard(),
            // Plain Insert → toggle overwrite mode
            (KeyCode::Insert, _, _) => self.toggle_overwrite(),
            // Ctrl+C / Ctrl+Insert → copy
            (KeyCode::Char('c' | 'C'), true, _) => self.copy_selection_or_line(),
            // Ctrl+X / Shift+Delete → cut
            (KeyCode::Char('x' | 'X'), true, _) => self.cut_selection_or_line(),
            (KeyCode::Delete, _, true) => self.cut_selection_or_line(),
            // Ctrl+V / Shift+Insert → paste
            (KeyCode::Char('v' | 'V'), true, _) => self.paste_from_clipboard(),
            // Ctrl+K (Shift accepted but not required — the owner's
            // terminal mangles the Ctrl+Shift+K delivery) → delete line
            (KeyCode::Char('k' | 'K'), true, _) => self.delete_line(),
            // Ctrl+G → go to line prompt
            (KeyCode::Char('g' | 'G'), true, _) => self.open_goto_line_prompt(),
            (KeyCode::Char(c), false, _) => {
                self.clear_selection();
                self.type_char(c);
            }
            (KeyCode::Enter, _, _) => {
                self.clear_selection();
                self.insert_text("\n", false);
            }
            (KeyCode::Backspace, _, _) => {
                if !self.delete_selected_range() {
                    self.backspace();
                }
            }
            // Plain Delete → delete forward, or the selection when active
            (KeyCode::Delete, _, _) => {
                if !self.delete_selected_range() {
                    self.delete_forward();
                }
            }
            // Esc → collapse selection
            (KeyCode::Esc, _, _) => self.clear_selection(),
            // Ctrl+Shift+Home/End → select to document start/end
            (KeyCode::Home, true, true) => {
                self.ctrl_shift_home_end(true);
            }
            (KeyCode::End, true, true) => {
                self.ctrl_shift_home_end(false);
            }
            // Ctrl+Home/End → jump to document start/end
            (KeyCode::Home, true, _) => self.ctrl_home_end(true),
            (KeyCode::End, true, _) => self.ctrl_home_end(false),
            // Ctrl+Shift+Left/Right → word selection
            (KeyCode::Left, true, true) => self.ctrl_shift_word_left(),
            (KeyCode::Right, true, true) => self.ctrl_shift_word_right(),
            // Ctrl+Left/Right → word movement
            (KeyCode::Left, true, _) => self.move_word_left(),
            (KeyCode::Right, true, _) => self.move_word_right(),
            // Alt+Up/Down → move the current line (or the selected block)
            (KeyCode::Up, _, _) if key.modifiers.contains(KeyModifiers::ALT) => self.move_lines(-1),
            (KeyCode::Down, _, _) if key.modifiers.contains(KeyModifiers::ALT) => {
                self.move_lines(1)
            }
            // Shift+Left/Right/Up/Down/Home/End/PageUp/PageDown → selection
            (KeyCode::Left, _, true) => self.shift_left(),
            (KeyCode::Right, _, true) => self.shift_right(),
            (KeyCode::Up, _, true) => self.shift_up(),
            (KeyCode::Down, _, true) => self.shift_down(),
            (KeyCode::Home, _, true) => self.shift_home(),
            (KeyCode::End, _, true) => self.shift_end(),
            (KeyCode::PageUp, _, true) => self.shift_page_up(),
            (KeyCode::PageDown, _, true) => self.shift_page_down(),
            // Plain movement → clear selection
            (KeyCode::Left, _, _) => {
                self.clear_selection();
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                } else if self.cursor_line > 0 {
                    self.cursor_line -= 1;
                    self.cursor_col = self.doc().line_len(self.cursor_line);
                }
                self.clamp_scroll();
            }
            (KeyCode::Right, _, _) => {
                self.clear_selection();
                if self.cursor_col < self.doc().line_len(self.cursor_line) {
                    self.cursor_col += 1;
                } else if self.cursor_line + 1 < self.doc().line_count() {
                    self.cursor_line += 1;
                    self.cursor_col = 0;
                }
                self.clamp_scroll();
            }
            (KeyCode::Up, _, _) => {
                self.clear_selection();
                self.cursor_line = self.cursor_line.saturating_sub(1);
                self.clamp_cursor();
                self.clamp_scroll();
            }
            (KeyCode::Down, _, _) => {
                self.clear_selection();
                self.cursor_line += 1;
                self.clamp_cursor();
                self.clamp_scroll();
            }
            (KeyCode::Home, _, _) => {
                self.clear_selection();
                self.cursor_col = 0;
                self.clamp_scroll();
            }
            (KeyCode::End, _, _) => {
                self.clear_selection();
                self.cursor_col = self.doc().line_len(self.cursor_line);
                self.clamp_scroll();
            }
            (KeyCode::PageUp, _, _) => {
                self.clear_selection();
                self.cursor_line = self
                    .cursor_line
                    .saturating_sub(self.viewport_h.saturating_sub(1));
                self.clamp_cursor();
                self.clamp_scroll();
            }
            (KeyCode::PageDown, _, _) => {
                self.clear_selection();
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
            // Quit confirmation accepts F10 as an additional close shortcut.
            (ConfirmKind::Quit, KeyCode::F(10))
            | (ConfirmKind::Quit, KeyCode::Char('y' | 'Y' | 's' | 'S')) => {
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
            // Replace-all confirmation: Y/yes/S/sí performs the replacement.
            (ConfirmKind::ReplaceAll(count, _), KeyCode::Char('y' | 'Y' | 's' | 'S')) => {
                self.confirm = None;
                self.perform_replace_all(count);
            }
            (ConfirmKind::ReplaceAll(_, _), KeyCode::Char('n' | 'N')) => {
                self.confirm = None;
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
        let is_open = matches!(kind, PromptKind::OpenFile);
        match key.code {
            // Interactive open-file list navigation (OpenFile only).
            KeyCode::Up if is_open => {
                self.open_selected = self.open_selected.saturating_sub(1);
            }
            KeyCode::Down if is_open => {
                if self.open_selected + 1 < self.open_candidates.len() {
                    self.open_selected += 1;
                }
            }
            KeyCode::Home if is_open => {
                self.open_selected = 0;
            }
            KeyCode::End if is_open => {
                if !self.open_candidates.is_empty() {
                    self.open_selected = self.open_candidates.len() - 1;
                }
            }
            KeyCode::PageDown if is_open => {
                self.open_selected = (self.open_selected + OPEN_LIST_PAGE)
                    .min(self.open_candidates.len().saturating_sub(1));
            }
            KeyCode::PageUp if is_open => {
                self.open_selected = self.open_selected.saturating_sub(OPEN_LIST_PAGE);
            }
            KeyCode::Enter => {
                let kind = *kind;
                let input_owned = input.clone();
                match kind {
                    PromptKind::OpenFile => {
                        // The highlighted candidate wins over the raw input.
                        let Some(path) = self.open_candidates.get(self.open_selected).cloned()
                        else {
                            // No candidates: the typed input is the path.
                            self.prompt = None;
                            self.open_candidates.clear();
                            self.open_selected = 0;
                            self.open_dir = None;
                            self.handle_prompt_confirm(&kind, &input_owned);
                            return;
                        };
                        if path.ends_with("..") {
                            // Climb one browsing level.
                            let parent = self.browsing_dir().parent().map(|p| p.to_path_buf());
                            if let Some(parent) = parent {
                                self.open_dir = Some(parent);
                            }
                            if let Some((_, input)) = self.prompt.as_mut() {
                                input.clear();
                            }
                            self.open_selected = 0;
                            self.refresh_open_candidates();
                            return;
                        }
                        if path.is_dir() {
                            // Enter the directory and list its contents.
                            self.open_dir = Some(path);
                            if let Some((_, input)) = self.prompt.as_mut() {
                                input.clear();
                            }
                            self.open_selected = 0;
                            self.refresh_open_candidates();
                            return;
                        }
                        let path = path.to_string_lossy().into_owned();
                        self.prompt = None;
                        self.open_candidates.clear();
                        self.open_selected = 0;
                        self.open_dir = None;
                        self.handle_prompt_confirm(&kind, &path);
                    }
                    PromptKind::GotoLine => {
                        // Invalid input keeps the prompt open for correction.
                        if let Some(n) =
                            input_owned.trim().parse::<usize>().ok().filter(|n| *n >= 1)
                        {
                            self.prompt = None;
                            self.cursor_line = (n - 1).min(self.doc().line_count() - 1);
                            self.clamp_cursor();
                            self.clamp_scroll();
                        }
                    }
                    PromptKind::SaveAs => {
                        self.prompt = None;
                        self.handle_prompt_confirm(&kind, &input_owned);
                    }
                }
            }
            KeyCode::Esc => {
                self.prompt = None;
                self.open_candidates.clear();
                self.open_selected = 0;
                self.open_dir = None;
            }
            KeyCode::Backspace => {
                input.pop();
                if is_open {
                    self.refresh_open_candidates();
                }
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                input.push(c);
                if is_open {
                    self.refresh_open_candidates();
                }
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
                            self.selections.push(None);
                            // Opening a file focuses its tab (VS Code behavior).
                            self.active = self.docs.len() - 1;
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
                    self.selections.push(None);
                    self.active = self.docs.len() - 1;
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
            // GotoLine confirms inline in on_prompt_key and never reaches here.
            PromptKind::GotoLine => {}
        }
    }

    /// Open the command palette (Ctrl+Shift+P, F4 fallback). Only reachable
    /// from the main key dispatch, so it never fights another modal.
    fn open_palette(&mut self) {
        self.palette = Some(PaletteState::new(PaletteKind::Command));
        self.refresh_palette();
    }

    /// Open the quick-open picker (Ctrl+P): open tabs first, then the files
    /// of the active document's directory that are not already open.
    fn open_quick_open(&mut self) {
        self.palette = Some(PaletteState::new(PaletteKind::QuickOpen));
        self.refresh_palette();
    }

    /// Recompute palette entries for the current query: commands rank by
    /// their localized label; quick-open ranks open-tab titles first and
    /// then the candidate files of the active document's directory.
    fn refresh_palette(&mut self) {
        let Some(mut state) = self.palette.take() else {
            return;
        };
        let query = state.query.clone();
        state.entries = match state.kind {
            PaletteKind::Command => {
                let source = all_commands();
                let labels: Vec<&'static str> = source
                    .iter()
                    .map(|c| i18n::t(self.lang, c.label_key))
                    .collect();
                fuzzy::rank(&query, labels)
                    .into_iter()
                    .map(|(i, _)| PaletteEntry::Command(source[i]))
                    .collect()
            }
            PaletteKind::QuickOpen => {
                // Labels and entries are built in LOCKSTEP: every open doc
                // gets a Tab entry (untitled included), then unopened dir
                // files — one label per entry, same order, equal lengths.
                let mut labels: Vec<String> = Vec::new();
                let mut source: Vec<PaletteEntry> = Vec::new();
                for i in 0..self.docs.len() {
                    labels.push(self.doc_title(i));
                    source.push(PaletteEntry::Tab(i));
                }
                let open_paths: Vec<PathBuf> = self
                    .docs
                    .iter()
                    .filter_map(|d| d.path().map(|p| p.to_path_buf()))
                    .collect();
                let dir = self
                    .doc()
                    .path()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                for path in App::compute_open_candidates(&dir, "") {
                    if open_paths.contains(&path) {
                        continue;
                    }
                    labels.push(
                        path.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    );
                    source.push(PaletteEntry::File(path));
                }
                let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
                fuzzy::rank(&query, refs)
                    .into_iter()
                    .map(|(i, _)| source[i].clone())
                    .collect()
            }
        };
        state.selected = 0;
        self.palette = Some(state);
    }

    /// Handle keys while the palette is open: type to filter, ↑/↓ to pick,
    /// Enter to run, Esc to cancel. Everything else is swallowed.
    fn on_palette_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => self.palette = None,
            KeyCode::Up => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.selected = palette.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(palette) = self.palette.as_mut()
                    && palette.selected + 1 < palette.entries.len()
                {
                    palette.selected += 1;
                }
            }
            KeyCode::Enter => {
                // Take the palette first so the executed command can open
                // another modal (search, prompt, confirm) without a fight.
                let Some(palette) = self.palette.take() else {
                    return;
                };
                match palette.entries.get(palette.selected) {
                    Some(PaletteEntry::Command(command)) => {
                        let kind = command.kind;
                        self.execute_command(kind);
                    }
                    Some(PaletteEntry::Tab(index)) => self.switch_to(*index),
                    Some(PaletteEntry::File(path)) => {
                        let path = path.to_string_lossy().into_owned();
                        self.handle_prompt_confirm(&PromptKind::OpenFile, &path);
                    }
                    // No candidates: the query is a typed path (Ctrl+O
                    // accepts direct paths; quick-open inherits that).
                    None if !palette.query.trim().is_empty() => {
                        let path = palette.query.trim().to_string();
                        self.handle_prompt_confirm(&PromptKind::OpenFile, &path);
                    }
                    None => {}
                }
            }
            KeyCode::Backspace => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.query.pop();
                }
                self.refresh_palette();
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.query.push(c);
                }
                self.refresh_palette();
            }
            _ => {}
        }
    }

    /// Run a registry command by mapping it to its existing App action.
    fn execute_command(&mut self, kind: CommandKind) {
        match kind {
            CommandKind::Save => self.save_active(),
            CommandKind::SaveAll => self.save_all(),
            CommandKind::Open => self.open_file_prompt(),
            CommandKind::CloseTab => self.request_close(),
            CommandKind::Quit => self.request_quit(),
            CommandKind::Undo => self.undo(),
            CommandKind::Redo => self.redo(),
            CommandKind::Find => self.open_search(),
            CommandKind::Replace => self.open_replace(),
            CommandKind::FindAllDocs => self.open_cross_doc_search(),
            CommandKind::ReplaceAllDocs => self.open_cross_doc_replace(),
            CommandKind::DeleteLine => self.delete_line(),
            CommandKind::MoveLineUp => self.move_lines(-1),
            CommandKind::MoveLineDown => self.move_lines(1),
            CommandKind::ToggleOverwrite => self.toggle_overwrite(),
            CommandKind::GotoLine => self.open_goto_line_prompt(),
            CommandKind::NextTab => self.next_doc(1),
            CommandKind::PrevTab => self.next_doc(-1),
            CommandKind::LastDocPanel1 => self.last_doc_of_panel(1),
        }
    }

    /// Toggle overwrite mode (Insert / palette command).
    fn toggle_overwrite(&mut self) {
        self.overwrite = !self.overwrite;
        let key = if self.overwrite {
            Key::OverwriteOn
        } else {
            Key::OverwriteOff
        };
        self.status = i18n::t(self.lang, key).to_string();
    }

    /// Open the search dialog, saving the current cursor position
    /// so it can be restored on Esc if no navigation happened.
    fn open_search(&mut self) {
        let pre_line = self.cursor_line;
        let pre_col = self.cursor_col;
        let mut search = SearchState::new_find(String::new(), vec![]);
        search.pre_search_line = pre_line;
        search.pre_search_col = pre_col;
        self.search = Some(search);
    }

    /// Open the replace dialog, reusing the existing search query
    /// if one is active, and starting in Replace mode.
    fn open_replace(&mut self) {
        let pre_line = self.cursor_line;
        let pre_col = self.cursor_col;
        let query = self
            .search
            .as_ref()
            .map(|s| s.query.clone())
            .unwrap_or_default();
        let matches = self.find_matches(&query);
        let has_matches = !matches.is_empty();
        let first_idx = has_matches.then_some(0);
        let mut search = SearchState::new_replace(query, matches);
        search.pre_search_line = pre_line;
        search.pre_search_col = pre_col;
        search.current_idx = first_idx;
        search.navigated = false;
        let has_matches = !search.matches.is_empty();
        self.search = Some(search);
        if has_matches {
            self.goto_match(0);
        }
    }

    /// Open cross-doc find dialog (Ctrl+Shift+F), searching all open tabs.
    fn open_cross_doc_search(&mut self) {
        let pre_line = self.cursor_line;
        let pre_col = self.cursor_col;
        let mut search = SearchState::new_find(String::new(), vec![]);
        search.pre_search_line = pre_line;
        search.pre_search_col = pre_col;
        search.cross_doc = true;
        search.start_doc = self.active;
        self.search = Some(search);
    }

    /// Open cross-doc replace dialog (Ctrl+Shift+H), searching all open tabs.
    fn open_cross_doc_replace(&mut self) {
        let pre_line = self.cursor_line;
        let pre_col = self.cursor_col;
        let query = self
            .search
            .as_ref()
            .map(|s| s.query.clone())
            .unwrap_or_default();
        let mut search = SearchState::new_replace(query, vec![]);
        search.pre_search_line = pre_line;
        search.pre_search_col = pre_col;
        search.navigated = false;
        search.cross_doc = true;
        search.start_doc = self.active;
        // Seed the state BEFORE computing matches so find_matches sees
        // cross_doc = true and spans every open document.
        self.search = Some(search);
        self.search_reset_matches();
        let has_matches = self.search.as_ref().is_some_and(|s| !s.matches.is_empty());
        if has_matches {
            self.goto_match(0);
        }
    }

    /// Handle keys while the search/replace dialog is open.
    fn on_search_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let is_replace = self
            .search
            .as_ref()
            .is_some_and(|s| s.mode == SearchMode::Replace);
        match (key.code, ctrl, shift) {
            // Esc closes the dialog; restore cursor if no navigation.
            // For cross-doc searches, also restores the starting doc.
            (KeyCode::Esc, _, _) => {
                let restore_cursor = self.search.as_ref().is_some_and(|s| !s.navigated);
                if restore_cursor {
                    let (pre_line, pre_col) = self
                        .search
                        .as_ref()
                        .map(|s| (s.pre_search_line, s.pre_search_col))
                        .unwrap();
                    if self.search.as_ref().is_some_and(|s| s.cross_doc) {
                        let start_doc = self.search.as_ref().map(|s| s.start_doc).unwrap_or(0);
                        if start_doc != self.active {
                            self.switch_to(start_doc);
                        }
                    }
                    self.cursor_line = pre_line;
                    self.cursor_col = pre_col;
                }
                self.search = None;
                self.clamp_cursor();
                self.clamp_scroll();
            }
            // Ctrl+Enter = replace current match and advance (replace mode only).
            (KeyCode::Enter, true, _) if is_replace => {
                self.replace_current_match();
            }
            // Shift+Enter = previous match (wrap).
            (KeyCode::Enter, _, true) => {
                self.search_prev_match();
            }
            // Enter = next match (wrap).
            (KeyCode::Enter, _, _) => {
                self.search_next_match();
            }
            // Shift+F3 = previous match (fallback for Shift+Enter).
            (KeyCode::F(3), _, true) => {
                self.search_prev_match();
            }
            // F3 = next match (function-key fallback for Enter, both modes).
            // Outside the dialog F3 keeps its save-all binding.
            (KeyCode::F(3), _, _) => {
                self.search_next_match();
            }
            // F5 = replace current match (fallback for Ctrl+Enter).
            (KeyCode::F(5), _, _) if is_replace => {
                self.replace_current_match();
            }
            // F6 = replace all (fallback).
            (KeyCode::F(6), _, _) if is_replace => {
                self.request_replace_all();
            }
            // Tab = switch field focus (replace mode only).
            (KeyCode::Tab, _, _) if is_replace => {
                if let Some(search) = &mut self.search {
                    search.field_focus = match search.field_focus {
                        ReplaceField::Query => ReplaceField::Replacement,
                        ReplaceField::Replacement => ReplaceField::Query,
                    };
                }
            }
            // Shift+Tab = switch field focus back (replace mode only).
            (KeyCode::Tab, _, true) if is_replace => {
                if let Some(search) = &mut self.search {
                    search.field_focus = match search.field_focus {
                        ReplaceField::Query => ReplaceField::Replacement,
                        ReplaceField::Replacement => ReplaceField::Query,
                    };
                }
            }
            // Printable characters in focused field: append, reset matches.
            // Accept any shift state (crossterm delivers the shifted char in `c`).
            (KeyCode::Char(c), false, _) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                if let Some(search) = &mut self.search {
                    let focused = search.field_focus;
                    if focused == ReplaceField::Query {
                        search.query.push(c);
                    } else {
                        search.replacement.push(c);
                    }
                }
                self.search_reset_matches();
            }
            // Backspace in focused field.
            (KeyCode::Backspace, _, _) => {
                if let Some(search) = &mut self.search {
                    let focused = search.field_focus;
                    if focused == ReplaceField::Query {
                        search.query.pop();
                    } else {
                        search.replacement.pop();
                    }
                }
                self.search_reset_matches();
            }
            _ => {}
        }
    }

    /// Recompute matches for the current query and reset to the
    /// first match (or None if no matches / empty query).
    fn search_reset_matches(&mut self) {
        let query = self
            .search
            .as_ref()
            .map(|s| s.query.clone())
            .unwrap_or_default();
        let matches = self.find_matches(&query);
        let has_matches = !matches.is_empty();
        let first_idx = has_matches.then_some(0);
        if let Some(search) = &mut self.search {
            search.matches = matches;
            search.current_idx = first_idx;
            search.navigated = false;
        }
        if has_matches {
            self.goto_match(0);
        }
    }

    /// Replace the current match with the replacement text and
    /// advance to the next match. Routes through document edit
    /// ops so Ctrl+Z undoes the replacement. For cross-doc
    /// searches, switches tabs to the match's document first.
    fn replace_current_match(&mut self) {
        // Extract all needed data from search before any mutable borrows.
        let (m, replacement, query, cross_doc) = {
            let Some(search) = &self.search else {
                return;
            };
            let Some(idx) = search.current_idx else {
                return;
            };
            if search.matches.is_empty() {
                return;
            }
            let m = search.matches[idx];
            let replacement = search.replacement.clone();
            let query = search.query.clone();
            let cross_doc = search.cross_doc;
            (m, replacement, query, cross_doc)
        };
        // For cross-doc, switch to the match's document first.
        if cross_doc && m.doc_index != self.active {
            self.switch_to(m.doc_index);
        }
        // Delete the match then insert the replacement; both
        // records are pushed as one undo step.
        let delete_record =
            match self
                .doc_mut()
                .delete_range(m.line, m.start_col, m.line, m.end_col)
            {
                Ok(r) => r,
                Err(e) => {
                    self.status = i18n::error_message(self.lang, &e);
                    return;
                }
            };
        let insert_record = match self.doc_mut().insert(m.line, m.start_col, &replacement) {
            Ok(r) => r,
            Err(e) => {
                if let Err(e2) = self.doc_mut().apply(&delete_record.inverse()) {
                    self.status = i18n::error_message(self.lang, &e2);
                }
                self.status = i18n::error_message(self.lang, &e);
                return;
            }
        };
        self.record_edits(vec![delete_record, insert_record], false);
        self.status = i18n::t(self.lang, Key::ReplacedCount).replace("{count}", "1");
        // Recompute matches and advance.
        let matches = self.find_matches(&query);
        let mut goto_idx: Option<usize> = None;
        if let Some(search) = &mut self.search {
            search.matches = matches;
            if search.matches.is_empty() {
                search.current_idx = None;
            } else {
                let next = match search.current_idx {
                    Some(i) if i + 1 < search.matches.len() => i + 1,
                    _ => 0,
                };
                search.current_idx = Some(next);
                goto_idx = Some(next);
            }
            search.navigated = true;
        }
        if let Some(idx) = goto_idx {
            self.goto_match(idx);
        }
    }

    /// Request replace-all: show a typed confirm modal with the
    /// occurrence count. The actual replacement happens in
    /// `on_confirm_key` when the user confirms.
    fn request_replace_all(&mut self) {
        let Some(search) = &self.search else {
            return;
        };
        if search.matches.is_empty() {
            return;
        }
        let doc_count = if search.cross_doc {
            search
                .matches
                .iter()
                .map(|m| m.doc_index)
                .collect::<std::collections::HashSet<_>>()
                .len()
        } else {
            1
        };
        self.confirm = Some(ConfirmKind::ReplaceAll(search.matches.len(), doc_count));
    }

    /// Perform the replace-all: iterate matches in reverse order
    /// (bottom-to-top so offsets stay valid), apply each delete+
    /// insert pair, and record all edits as one undo step.
    fn perform_replace_all(&mut self, count: usize) {
        let query = self
            .search
            .as_ref()
            .map(|s| s.query.clone())
            .unwrap_or_default();
        let replacement = self
            .search
            .as_ref()
            .map(|s| s.replacement.clone())
            .unwrap_or_default();
        let cross_doc = self.search.as_ref().is_some_and(|s| s.cross_doc);
        let matches: Vec<MatchPos> = self.find_matches(&query);
        if matches.is_empty() {
            return;
        }
        if cross_doc {
            // Group matches by document for per-doc undo.
            let mut doc_matches: std::collections::HashMap<usize, Vec<MatchPos>> =
                std::collections::HashMap::new();
            for m in matches {
                doc_matches.entry(m.doc_index).or_default().push(m);
            }
            let mut total_replaced = 0usize;
            let mut skipped = 0usize;
            for doc_idx in 0..self.docs.len() {
                let doc_m = match doc_matches.get(&doc_idx) {
                    Some(m) => m.clone(),
                    None => continue,
                };
                if self.docs[doc_idx].is_read_only() {
                    skipped += 1;
                    continue;
                }
                if doc_m.is_empty() {
                    continue;
                }
                // Temporarily switch active so doc_mut and record_edits target it.
                let prev_active = self.active;
                self.active = doc_idx;
                let mut records: Vec<EditRecord> = Vec::new();
                for m in doc_m.iter().rev() {
                    if let Ok(delete_rec) =
                        self.doc_mut()
                            .delete_range(m.line, m.start_col, m.line, m.end_col)
                    {
                        if let Ok(insert_rec) =
                            self.doc_mut().insert(m.line, m.start_col, &replacement)
                        {
                            records.push(delete_rec);
                            records.push(insert_rec);
                        } else {
                            let _ = self.doc_mut().apply(&delete_rec.inverse());
                        }
                    }
                }
                if !records.is_empty() {
                    self.record_edits(records, false);
                    total_replaced += doc_m.len();
                }
                self.active = prev_active;
            }
            if total_replaced > 0 {
                self.status = i18n::tr(
                    self.lang,
                    Key::ReplacedCount,
                    &[("count", &total_replaced.to_string())],
                );
            }
            if skipped > 0 {
                let skipped_msg = i18n::t(self.lang, Key::SkippedReadOnly);
                self.status = format!("{}: {}", self.status, skipped_msg);
            }
        } else {
            let mut records: Vec<EditRecord> = Vec::new();
            // Process bottom-to-top so earlier offsets stay valid.
            for m in matches.iter().rev() {
                if let Ok(delete_rec) =
                    self.doc_mut()
                        .delete_range(m.line, m.start_col, m.line, m.end_col)
                {
                    if let Ok(insert_rec) = self.doc_mut().insert(m.line, m.start_col, &replacement)
                    {
                        records.push(delete_rec);
                        records.push(insert_rec);
                    } else {
                        // If insert fails, undo the delete for this match.
                        let _ = self.doc_mut().apply(&delete_rec.inverse());
                    }
                }
            }
            if !records.is_empty() {
                self.record_edits(records, false);
                self.status = i18n::tr(
                    self.lang,
                    Key::ReplacedCount,
                    &[("count", &count.to_string())],
                );
            }
        }
        // Clear the search state since all matches are replaced.
        self.search = None;
    }

    /// Advance to the next match (wraps around).
    fn search_next_match(&mut self) {
        let Some(search) = &mut self.search else {
            return;
        };
        if search.matches.is_empty() {
            return;
        }
        search.navigated = true;
        let next = match search.current_idx {
            Some(i) => (i + 1) % search.matches.len(),
            None => 0,
        };
        search.current_idx = Some(next);
        self.goto_match(next);
    }

    /// Go to the previous match (wraps around).
    fn search_prev_match(&mut self) {
        let Some(search) = &mut self.search else {
            return;
        };
        if search.matches.is_empty() {
            return;
        }
        search.navigated = true;
        let prev = match search.current_idx {
            Some(i) => (i + search.matches.len() - 1) % search.matches.len(),
            None => 0,
        };
        search.current_idx = Some(prev);
        self.goto_match(prev);
    }

    /// Move cursor to the start of match `idx` and scroll into view.
    /// For cross-doc searches, switches tabs when the match is in a
    /// different document than the active one.
    fn goto_match(&mut self, idx: usize) {
        let Some(search) = &self.search else {
            return;
        };
        let m = search.matches[idx];
        if search.cross_doc && m.doc_index != self.active {
            self.switch_to(m.doc_index);
        }
        self.cursor_line = m.line;
        self.cursor_col = m.start_col;
        self.clamp_cursor();
        self.clamp_scroll();
    }

    /// Find all single-line matches of `query`.
    /// When the active search is cross-doc, searches every open
    /// document in tab order; otherwise searches only the active doc.
    /// Returns char-offset positions; empty query returns no matches.
    fn find_matches(&self, query: &str) -> Vec<MatchPos> {
        if query.is_empty() {
            return vec![];
        }
        let case_sensitive = self.search_options.case_sensitive;
        let cross_doc = self.search.as_ref().is_some_and(|s| s.cross_doc);
        let mut results = Vec::new();
        let doc_indices: Vec<usize> = if cross_doc {
            (0..self.docs.len()).collect()
        } else {
            vec![self.active]
        };
        for doc_idx in doc_indices {
            let doc = &self.docs[doc_idx];
            for line_idx in 0..doc.line_count() {
                let line = doc.line(line_idx);
                if case_sensitive {
                    let mut start = 0;
                    while let Some(pos) = line[start..].find(query) {
                        let byte_start = start + pos;
                        let byte_end = byte_start + query.len();
                        results.push(MatchPos {
                            line: line_idx,
                            start_col: line[..byte_start].chars().count(),
                            end_col: line[..byte_end].chars().count(),
                            doc_index: doc_idx,
                        });
                        start = byte_end;
                    }
                } else {
                    let line_lower = line.to_lowercase();
                    let query_lower = query.to_lowercase();
                    let mut start = 0;
                    while let Some(pos) = line_lower[start..].find(&query_lower) {
                        let byte_start = start + pos;
                        let byte_end = byte_start + query_lower.len();
                        results.push(MatchPos {
                            line: line_idx,
                            start_col: line[..byte_start].chars().count(),
                            end_col: line[..byte_end].chars().count(),
                            doc_index: doc_idx,
                        });
                        start = byte_end;
                    }
                }
            }
        }
        results
    }

    /// Compose styled spans for a line with search match highlights.
    /// Used by the renderer and by tests directly.
    fn compose_highlight_spans(
        _line_text: &str,
        highlights: &[&MatchPos],
        current_idx: usize,
        search_match_bg: Color,
        search_current_match_bg: Color,
        syntax_spans: &[Span<'static>],
    ) -> Vec<Span<'static>> {
        if highlights.is_empty() {
            return syntax_spans.to_vec();
        }
        let ranges: Vec<(usize, usize, Color)> = highlights
            .iter()
            .enumerate()
            .map(|(hi, m)| {
                let bg = if hi == current_idx {
                    search_current_match_bg
                } else {
                    search_match_bg
                };
                (m.start_col, m.end_col, bg)
            })
            .collect();
        let mut result = Vec::new();
        let mut char_pos = 0;
        for span in syntax_spans {
            let text = span.content.as_ref();
            let len = text.chars().count();
            let start = char_pos;
            let end = char_pos + len;
            let mut splits = vec![0];
            for &(ms, me, _) in &ranges {
                if me <= start || ms >= end {
                    continue;
                }
                let rs = ms.saturating_sub(start);
                let re = (me - start).min(len);
                if rs > 0 {
                    splits.push(rs);
                }
                if re < len {
                    splits.push(re);
                }
            }
            splits.push(len);
            splits.sort_unstable();
            splits.dedup();
            let mut prev = 0;
            for &next in &splits[1..] {
                let seg: String = text.chars().skip(prev).take(next - prev).collect();
                let seg_start = start + prev;
                let seg_end = start + next;
                let mut seg_style = span.style;
                for &(ms, me, bg) in &ranges {
                    if seg_start >= ms && seg_end <= me {
                        seg_style = seg_style.bg(bg);
                        break;
                    }
                }
                if !seg.is_empty() {
                    result.push(Span::styled(seg, seg_style));
                }
                prev = next;
            }
            char_pos += len;
        }
        result
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
                let gutter_w = self.gutter_width();
                let buf_col = (column as usize).saturating_sub(gutter_w) + self.col_off;
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

    // -- Gutter helper (single source of truth for width) ----------------

    /// Width of the line-number gutter in terminal cells: digits of the
    /// highest visible 1-based line number plus one space padding.
    /// Used by both the renderer and the mouse-mapping to stay in sync.
    fn gutter_width(&self) -> usize {
        let last = (self.scroll_top + self.viewport_h).min(self.doc().line_count());
        let highest = last.max(1); // at least line 1 for empty docs
        highest.to_string().len() + 1
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
        self.draw_open_list(f, editor_area);
        self.draw_palette(f, editor_area);
    }

    /// Interactive Ctrl+O candidate list: a bordered popup at the bottom of
    /// the editor area, width fitted to the longest visible name, the
    /// highlighted entry in prompt colors, hint line in the title.
    fn draw_open_list(&mut self, f: &mut Frame, editor_area: ratatui::prelude::Rect) {
        if !matches!(self.prompt, Some((PromptKind::OpenFile, _)))
            || self.open_candidates.is_empty()
        {
            return;
        }
        let candidates = self.open_candidates.clone();
        let selected = self.open_selected;
        let visible = candidates.len().min(OPEN_LIST_PAGE);
        // Display names: `..` for the parent, `name/` for directories,
        // plain file names otherwise.
        let display: Vec<String> = candidates
            .iter()
            .map(|p| {
                if p.ends_with("..") {
                    "..".to_string()
                } else if p.is_dir() {
                    format!(
                        "{}/",
                        p.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    )
                } else {
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                }
            })
            .collect();
        // Fit the width to the longest visible name (marker + padding +
        // borders), never narrower than 48 nor wider than the editor.
        // Scroll so the SELECTED row stays inside the window: at most one
        // row above it moves off-screen per Down press.
        let skip = selected.saturating_sub(visible.saturating_sub(1));
        let longest = display
            .iter()
            .skip(skip)
            .take(visible)
            .map(|name| name.chars().count())
            .max()
            .unwrap_or(0);
        // A centered, roomy modal: 2x wider floor (48 cols) and 3x taller
        // floor (11 rows, vs the old 5-row page), so even short lists render
        // as a real centered dialog instead of a cramped bottom strip.
        let width = (longest as u16 + 8).max(48).min(editor_area.width);
        let height = (visible as u16 + 2).max(11).min(editor_area.height);
        let x = editor_area.x + editor_area.width.saturating_sub(width) / 2;
        let y = editor_area.y + editor_area.height.saturating_sub(height) / 2;
        let area = ratatui::prelude::Rect {
            x,
            y,
            width,
            height,
        };
        let lines: Vec<Line> = display
            .iter()
            .enumerate()
            .skip(skip)
            .take(visible)
            .map(|(i, name)| {
                let idx = skip + i;
                let style = if idx == selected {
                    Style::new()
                        .fg(self.theme.prompt_fg)
                        .bg(self.theme.prompt_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(self.theme.fg).bg(self.theme.bg)
                };
                let marker = if idx == selected { "> " } else { "  " };
                Line::from(Span::styled(format!("{marker}{name}"), style))
            })
            .collect();
        let hints = i18n::t(self.lang, Key::OpenListHints);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(self.theme.accent))
            .title(Span::styled(
                hints,
                Style::new()
                    .fg(self.theme.line_number_fg)
                    .add_modifier(Modifier::DIM),
            ));
        // Blank the popup area first so editor text cannot bleed through.
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(lines)
                .block(block)
                .style(Style::new().bg(self.theme.bg)),
            area,
        );
    }

    /// Command palette popup: top-center over the editor, echoing the query
    /// on an input row and the ranked entries below it (VS Code style). The
    /// input row uses prompt colors; the highlighted entry is bold with a
    /// `>` marker; no matches shows a dim row instead of nothing.
    fn draw_palette(&mut self, f: &mut Frame, editor_area: ratatui::prelude::Rect) {
        let Some(palette) = self.palette.as_ref() else {
            return;
        };
        let lang = self.lang;
        let query = palette.query.clone();
        let selected = palette.selected;
        let hints = match palette.kind {
            PaletteKind::Command => i18n::t(lang, Key::PaletteHints),
            PaletteKind::QuickOpen => i18n::t(lang, Key::QuickOpenHints),
        };
        let entries: Vec<String> = palette
            .entries
            .iter()
            .map(|entry| match entry {
                PaletteEntry::Command(command) => i18n::t(lang, command.label_key).to_string(),
                PaletteEntry::Tab(index) => self.doc_title(*index),
                PaletteEntry::File(path) => path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            })
            .collect();
        let visible = entries.len().clamp(1, 7);
        let skip = selected.saturating_sub(visible.saturating_sub(1));
        let longest = entries
            .iter()
            .skip(skip)
            .take(visible)
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0)
            .max(query.chars().count() + 2);
        let width = (longest as u16 + 10).max(30).min(editor_area.width);
        let rows = if entries.is_empty() { 2 } else { 1 + visible }; // input + entries (or No matches)
        let height = (rows as u16 + 2).min(editor_area.height);
        let x = editor_area.x + editor_area.width.saturating_sub(width) / 2;
        let area = ratatui::prelude::Rect {
            x,
            y: editor_area.y,
            width,
            height,
        };
        let input = Line::from(Span::styled(
            format!("> {query}"),
            Style::new()
                .fg(self.theme.prompt_fg)
                .bg(self.theme.prompt_bg)
                .add_modifier(Modifier::BOLD),
        ));
        let mut lines = vec![input];
        if entries.is_empty() {
            lines.push(Line::from(Span::styled(
                i18n::t(lang, Key::NoMatches).to_string(),
                Style::new()
                    .fg(self.theme.line_number_fg)
                    .add_modifier(Modifier::DIM),
            )));
        } else {
            for (i, label) in entries.iter().enumerate().skip(skip).take(visible) {
                let marker = if i == selected { "> " } else { "  " };
                let style = if i == selected {
                    Style::new()
                        .fg(self.theme.prompt_fg)
                        .bg(self.theme.prompt_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(self.theme.fg).bg(self.theme.bg)
                };
                lines.push(Line::from(Span::styled(format!("{marker}{label}"), style)));
            }
        }
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(self.theme.accent))
            .title(Span::styled(hints, Style::new().fg(self.theme.fg)));
        // Blank the popup area first so editor text cannot bleed through.
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(lines)
                .block(block)
                .style(Style::new().bg(self.theme.bg)),
            area,
        );
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
        let gutter_w = self.gutter_width();

        let extension = highlight::extension_of(self.doc().path());
        let lines: Vec<&str> = (first..last).map(|i| self.doc().line(i)).collect();
        let styled =
            highlight::styled_lines(&lines, extension.as_deref(), self.theme.fg, self.theme.bg);

        // Build a map of line_idx → Vec<MatchPos> for visible lines,
        // so we can highlight matches when the search dialog is open.
        let match_highlights: std::collections::HashMap<usize, Vec<&MatchPos>> = self
            .search
            .as_ref()
            .map(|search| {
                let mut map: std::collections::HashMap<usize, Vec<&MatchPos>> =
                    std::collections::HashMap::new();
                for m in &search.matches {
                    if m.line >= first && m.line < last {
                        map.entry(m.line).or_default().push(m);
                    }
                }
                map
            })
            .unwrap_or_default();

        // Prepend a right-aligned line-number span (+ trailing space) to each
        // visible line. Rows past EOF receive a blank gutter.
        let mut gutter_lines: Vec<Line> = styled
            .into_iter()
            .enumerate()
            .map(|(i, mut line)| {
                let line_num = first + i + 1; // 1-based
                let buf_line = line_num - 1;
                let is_cursor_line = line_num - 1 == self.cursor_line;
                let num_style = Style::new()
                    .fg(if is_cursor_line {
                        self.theme.line_number_cursor_fg
                    } else {
                        self.theme.line_number_fg
                    })
                    .bg(self.theme.bg);
                let num_span = Span::styled(
                    format!("{:>width$}", line_num, width = gutter_w - 1),
                    num_style,
                );
                let space_span = Span::styled(" ", num_style);
                let mut spans = vec![num_span, space_span];

                // Apply selection bg first, then search highlights
                // override selection bg for matched cells.
                let mut line_spans = line.spans;
                if let Some((sel_start, sel_end)) = self.selection_range_for_line(buf_line) {
                    line_spans = self.apply_selection_bg(line_spans, sel_start, sel_end);
                }
                if let Some(highlights) = match_highlights.get(&buf_line) {
                    let line_text = self.doc().line(buf_line);
                    let current_idx = self
                        .search
                        .as_ref()
                        .and_then(|s| s.current_idx)
                        .unwrap_or(0);
                    spans.extend(App::compose_highlight_spans(
                        line_text,
                        highlights,
                        current_idx,
                        self.theme.search_match_bg,
                        self.theme.search_current_match_bg,
                        &line_spans,
                    ));
                } else {
                    spans.extend(line_spans);
                }

                line.spans = spans;
                line
            })
            .collect();

        // Blank gutter for rows past EOF.
        let visible_count = last - first;
        for _ in visible_count..self.viewport_h {
            let blank_style = Style::new().fg(self.theme.line_number_fg).bg(self.theme.bg);
            gutter_lines.push(Line::from(vec![Span::styled(
                " ".repeat(gutter_w),
                blank_style,
            )]));
        }

        let widget = Paragraph::new(gutter_lines);
        f.render_widget(widget, area);

        // Build the search/replace dialog widget early; render it after the
        // text-lines so it appears on top of editor content.
        let dialog_widget = self.search.as_ref().map(|search| {
            let query = &search.query;
            let is_replace = search.mode == SearchMode::Replace;
            let counter = if search.matches.is_empty() {
                if is_replace {
                    i18n::t(self.lang, Key::NoMatchesReplace).to_string()
                } else {
                    i18n::t(self.lang, Key::NoMatches).to_string()
                }
            } else {
                let current = search.current_idx.map_or(0, |i| i + 1);
                if search.cross_doc {
                    let doc_title = self.doc_title(search.matches[current].doc_index);
                    i18n::tr(
                        self.lang,
                        Key::MatchCounterCross,
                        &[
                            ("current", &current.to_string()),
                            ("total", &search.matches.len().to_string()),
                            ("doc", &doc_title),
                        ],
                    )
                } else {
                    i18n::tr(
                        self.lang,
                        Key::MatchCounter,
                        &[
                            ("current", &current.to_string()),
                            ("total", &search.matches.len().to_string()),
                        ],
                    )
                }
            };
            let hints = if is_replace {
                format!(
                    "{}  {}  {}  {}  {}",
                    i18n::t(self.lang, Key::ReplaceNext),
                    i18n::t(self.lang, Key::ReplaceFallback),
                    i18n::t(self.lang, Key::ReplaceAll),
                    i18n::t(self.lang, Key::ReplaceSwitchField),
                    i18n::t(self.lang, Key::ReplaceClose),
                )
            } else {
                format!(
                    "{}  {}  {}  {}",
                    i18n::t(self.lang, Key::FindNext),
                    i18n::t(self.lang, Key::FindPrev),
                    i18n::t(self.lang, Key::FindClose),
                    i18n::t(self.lang, Key::FindOptions),
                )
            };
            let dialog_text = if is_replace {
                let replacement = &search.replacement;
                // The ACTIVE field is wrapped in brackets — the old ">"/"<"
                // marker did not communicate which field receives typing.
                let (q_open, q_close, r_open, r_close) = match search.field_focus {
                    ReplaceField::Query => ("[", "]", " ", " "),
                    ReplaceField::Replacement => (" ", " ", "[", "]"),
                };
                format!(
                    "{}  {}{} {}{}  {}{} {}{}  {}",
                    i18n::t(self.lang, Key::ReplaceDialogTitle),
                    q_open,
                    i18n::t(self.lang, Key::ReplaceFieldQuery),
                    query,
                    q_close,
                    r_open,
                    i18n::t(self.lang, Key::ReplaceFieldReplacement),
                    replacement,
                    r_close,
                    counter
                )
            } else {
                format!(
                    "{}  {}  {}",
                    i18n::t(self.lang, Key::FindDialogTitle),
                    query,
                    counter
                )
            };
            let line = Line::from(vec![
                Span::styled(
                    dialog_text,
                    // Dark prompt fg on the amber bar: the editor's light
                    // gray fg was unreadable there (owner feedback).
                    Style::default()
                        .fg(self.theme.prompt_fg)
                        .bg(self.theme.prompt_bg),
                ),
                Span::styled(
                    format!("  {hints}"),
                    Style::default()
                        .fg(self.theme.prompt_fg)
                        .bg(self.theme.prompt_bg)
                        .add_modifier(Modifier::DIM),
                ),
            ]);
            Paragraph::new(line).style(
                Style::default()
                    .fg(self.theme.prompt_fg)
                    .bg(self.theme.prompt_bg),
            )
        });

        // Render the search dialog on top of editor content, but confined to
        // its own single row: a Paragraph's base style fills the entire rect
        // it is rendered into, so handing it the full editor area washed the
        // whole screen in the prompt background (unreadable).
        if let Some(dialog) = dialog_widget {
            f.render_widget(dialog, ratatui::prelude::Rect { height: 1, ..area });
        }

        let x = area.x + gutter_w as u16 + (self.cursor_col - self.col_off) as u16;
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
            "{} {mode}{pos}{read_only}",
            self.doc_title(self.active).trim_end()
        );
        let ext = highlight::extension_of(doc.path());
        let language = highlight::language_name(ext.as_deref())
            .unwrap_or_else(|| i18n::t(self.lang, Key::PlainText));
        // Build stamp (UTC MMDD-HHMM from build.rs) + package version first,
        // then the language indicator further right (VS Code order), then the
        // transient status message at the far right.
        let build_id = option_env!("EDITORMINI_BUILD_ID").unwrap_or("dev");
        let mut right = format!(
            " v{}·{build_id} {language} {}",
            env!("CARGO_PKG_VERSION"),
            self.status
        );
        // The left side (doc identity + position) always wins the room it
        // needs; the right side is trimmed with an ellipsis when narrow.
        // Owner-visible: long hints used to crowd the filename out entirely.
        let left_len = left.chars().count();
        let right_len = right.chars().count();
        if left_len + right_len > width {
            if left_len < width {
                let room = width - left_len;
                right = format!(
                    "{}…",
                    right
                        .chars()
                        .take(room.saturating_sub(1))
                        .collect::<String>()
                );
            } else {
                left = format!(
                    "{}…",
                    left.chars()
                        .take(width.saturating_sub(1))
                        .collect::<String>()
                );
                right.clear();
            }
        }
        (left, right)
    }

    fn draw_status(&mut self, f: &mut Frame, area: ratatui::prelude::Rect) {
        if let Some((kind, input)) = &self.prompt {
            let title = match kind {
                PromptKind::OpenFile => i18n::t(self.lang, Key::OpenFile),
                PromptKind::SaveAs => i18n::t(self.lang, Key::SaveAs),
                PromptKind::GotoLine => i18n::t(self.lang, Key::GotoLine),
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
            let prompt = match self.confirm {
                Some(ConfirmKind::ReplaceAll(count, doc_count)) => {
                    let title = i18n::t(self.lang, Key::ReplaceConfirmTitle);
                    let msg = if doc_count <= 1 {
                        i18n::tr(
                            self.lang,
                            Key::ReplaceConfirmMessage,
                            &[("count", &count.to_string())],
                        )
                    } else {
                        i18n::tr(
                            self.lang,
                            Key::ReplaceConfirmMessageCross,
                            &[
                                ("count", &count.to_string()),
                                ("docs", &doc_count.to_string()),
                            ],
                        )
                    };
                    format!("{title} {msg}")
                }
                _ => i18n::t(self.lang, Key::SaveChangesPrompt).to_string(),
            };
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

    fn shift(code: KeyCode) -> KeyEvent {
        press(code, KeyModifiers::SHIFT)
    }

    fn alt(code: KeyCode) -> KeyEvent {
        press(code, KeyModifiers::ALT)
    }

    fn ctrl_shift(code: KeyCode) -> KeyEvent {
        press(code, KeyModifiers::CONTROL | KeyModifiers::SHIFT)
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
            !app.status.is_empty(),
            "the prompt bar replaces the status only while it is open"
        );
    }

    #[test]
    fn status_persists_until_replaced_by_a_new_message() {
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
        // ...and so does editing: messages are only replaced, never cleared
        // (owner: the startup hints must not vanish with the first keystroke).
        app.on_key(plain(KeyCode::Char('x')));
        assert_eq!(app.status, saved);
        // A new message (overwrite toggle) replaces the old one.
        app.on_key(plain(KeyCode::Insert));
        assert_eq!(app.status, "Overwrite ON");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn typing_keeps_the_last_status_message_visible() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::Es);
        let hints = app.status.clone();
        assert!(hints.contains("guardar"), "status: {}", hints);
        typed(&mut app, "hola");
        assert_eq!(app.status, hints, "typing must not clear the status");
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
    fn ctrl_q_f10_accepts_quit_confirmation() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "keep me");
        app.on_key(ctrl(KeyCode::Char('q')));
        assert_eq!(app.confirm, Some(ConfirmKind::Quit));
        // F10 is the terminal-proof fallback for Ctrl+Q — it accepts the
        // confirmation the same way Y/Sí does (the confirm state is cleared
        // by the caller, not here).
        app.on_key(plain(KeyCode::F(10)));
        assert!(app.quit);
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
        // Gutter width is 2 for single-digit line numbers.
        // Click at screen column 8 (gutter_w=2 + buf_col=6) → buffer col 6
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            8,
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
        // Gutter width is 2. Click far past end of line 0 → clamps to col 2
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            101,
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
        // Click row 50 (far past last line) → clamps to line 0 end.
        // Column 0 is in the gutter; saturating_sub clamps to buf_col=0,
        // then clamp_cursor caps it at the line end.
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
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
        // Gutter width is 2 for visible lines 4-8 (1-based). Click at
        // screen column 2 (past the gutter) → buf_col = 2-2+0 = 0,
        // editor_row=2 → buffer line 3+2=5.
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
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
        let app = App::new(
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
        // Gutter width is 2. Click at screen column 5 (gutter_w=2 + buf_col=3)
        // → buffer col = (5-2)+5 = 8
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            5,
            1,
            KeyModifiers::NONE,
        );
        assert_eq!(
            app.cursor_col, 8,
            "buffer col = screen col - gutter_w + col_off"
        );
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
        // Column 2 is past the gutter (width 2); gutter click would clamp.
        app.on_mouse(MouseEventKind::ScrollUp, 2, 5, KeyModifiers::NONE);
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
        app.on_mouse(MouseEventKind::ScrollDown, 2, 5, KeyModifiers::NONE);
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
        app.on_mouse(MouseEventKind::ScrollUp, 2, 5, KeyModifiers::NONE);
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
        app.on_mouse(MouseEventKind::ScrollDown, 2, 5, KeyModifiers::NONE);
        assert_eq!(app.scroll_top, max_scroll, "scroll_down must clamp at max");
    }

    #[test]
    fn click_below_viewport_ignored() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        typed(&mut app, "hello");
        let original_line = app.cursor_line;
        // Click below the editor viewport (row >= 1 + viewport_h);
        // column 2 is past the gutter (width 2).
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
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
        // Column 7 is past the gutter (width 2).
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Right),
            7,
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
        // Gutter width is 2 for visible lines 9-10 (1-based). Click at
        // screen column 2 (past gutter) → buf_col = 2-2+0 = 0.
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
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
            2,
            1,
            KeyModifiers::NONE,
        );
        assert_eq!(app.cursor_line, 8);
        // clamp_scroll should have adjusted scroll_top since cursor (8) is now
        // within the viewport (8..13), so scroll_top stays 8
        assert_eq!(app.scroll_top, 8);
    }

    // -- Gutter tests ------------------------------------------------

    #[test]
    fn gutter_width_1_digit() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        // Highest visible line is 1 (1 digit) → width = 1 + 1 = 2
        assert_eq!(app.gutter_width(), 2);
    }

    #[test]
    fn gutter_width_2_digits() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        // Type enough lines to push the highest visible line into 2 digits
        for i in 0..15 {
            typed(&mut app, &format!("line{i}"));
            if i < 14 {
                app.on_key(plain(KeyCode::Enter));
            }
        }
        // Highest visible line is 15 (2 digits) → width = 2 + 1 = 3
        assert_eq!(app.gutter_width(), 3);
    }

    #[test]
    fn gutter_width_grows_at_10() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        // 9 lines: highest visible = 9 (1 digit) → width 2
        for i in 0..9 {
            typed(&mut app, &format!("line{i}"));
            if i < 8 {
                app.on_key(plain(KeyCode::Enter));
            }
        }
        assert_eq!(app.gutter_width(), 2, "9 lines: 1 digit + 1 padding");
        // Add a 10th line (Enter first to create a new line) →
        // highest visible = 10 (2 digits) → width 3
        app.on_key(plain(KeyCode::Enter));
        typed(&mut app, "line9");
        assert_eq!(app.gutter_width(), 3, "10 lines: 2 digits + 1 padding");
    }

    #[test]
    fn cursor_screen_col_formula_includes_gutter() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        typed(&mut app, "hello");
        // Move cursor to col 0 so the formula is easy to verify.
        app.on_key(plain(KeyCode::Home));
        // cursor_col=0, col_off=0, gutter_w=2.
        // Expected screen x = gutter_w + cursor_col - col_off = 2 + 0 - 0 = 2.
        let expected_x = app.gutter_width() as u16 + (app.cursor_col - app.col_off) as u16;
        assert_eq!(expected_x, 2);
    }

    #[test]
    fn gutter_renders_blank_for_rows_past_eof() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 5, 40);
        // Only 1 line of content; viewport is 5 rows tall.
        typed(&mut app, "only line");
        // The draw_editor pads with blank gutter lines for rows past EOF.
        // We verify this by checking that the Paragraph widget receives
        // exactly viewport_h lines (1 content + 4 blank).
        // The gutter_width helper returns 2 for line 1.
        assert_eq!(app.gutter_width(), 2);
    }

    #[test]
    fn mouse_click_in_gutter_clamps_to_first_visible_col() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        typed(&mut app, "hello world");
        // Click at screen column 1 (inside the gutter, width 2).
        // saturating_sub(2) gives 0, so buf_col = 0 + col_off(0) = 0.
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            1,
            1,
            KeyModifiers::NONE,
        );
        assert_eq!(app.cursor_line, 0);
        assert_eq!(
            app.cursor_col, 0,
            "gutter click clamps to first visible col"
        );
    }

    #[test]
    fn mouse_click_first_text_column_maps_correctly() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        typed(&mut app, "hello world");
        // Gutter width is 2. Click at screen column 2 (first text column)
        // → buf_col = (2-2) + 0 = 0.
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
            1,
            KeyModifiers::NONE,
        );
        assert_eq!(app.cursor_line, 0);
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn mouse_click_gutter_width_1_to_2_digit_boundary() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        setup_viewport(&mut app, 10, 40);
        // 9 lines: gutter_w = 2. Add a 10th line → gutter_w = 3.
        for i in 0..9 {
            typed(&mut app, &format!("line{i}"));
            if i < 8 {
                app.on_key(plain(KeyCode::Enter));
            }
        }
        assert_eq!(app.gutter_width(), 2);
        app.on_key(plain(KeyCode::Enter));
        typed(&mut app, "line9");
        assert_eq!(app.gutter_width(), 3);
        // Click at the first text column (screen col = gutter_w=3)
        // for the 10-line case → buf_col = (3-3)+0 = 0.
        app.on_mouse(
            MouseEventKind::Down(MouseButton::Left),
            3,
            1,
            KeyModifiers::NONE,
        );
        assert_eq!(app.cursor_col, 0, "first text col after 10-line growth");
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
    fn status_bar_separates_filename_from_cursor_position() {
        // Owner: the end of the filename ran into the Ln/Col coordinates.
        let dir = temp_dir("status_spacing");
        let path = dir.join("notes.txt");
        let app = App::new(vec![Document::new_with_path(&path)], Lang::En);
        let (left, _) = app.status_line(120);
        assert!(left.contains("notes.txt Ln"), "left: {left}");
        let _ = dir;
    }

    #[test]
    fn language_indicator_sits_right_of_the_build_stamp() {
        // Owner: like VS Code, the recognized language belongs further right.
        let app = App::new(vec![Document::new_untitled()], Lang::En);
        let (_, right) = app.status_line(120);
        let version = right.find("v0.").expect("version marker");
        let language = right.find("Plain text").expect("language");
        assert!(
            version < language,
            "language must sit right of the build stamp: {right}"
        );
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
        // (Char count: the ellipsis itself is 3 UTF-8 bytes.)
        assert!(
            left.chars().count() <= 20,
            "left len: {}",
            left.chars().count()
        );
        assert!(left.ends_with('…'), "left should end with ellipsis: {left}");
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

    // -- Search dialog tests -------------------------------------------

    #[test]
    fn ctrl_f_opens_search_dialog() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        assert!(app.search.is_none());
        app.on_key(ctrl(KeyCode::Char('f')));
        assert!(app.search.is_some());
        assert_eq!(app.search.as_ref().unwrap().query, "");
        assert_eq!(app.search.as_ref().unwrap().matches.len(), 0);
    }

    #[test]
    fn typing_updates_search_query() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "rust");
        assert_eq!(app.search.as_ref().unwrap().query, "rust");
    }

    #[test]
    fn counter_shows_match_count() {
        let dir = temp_dir("search_match");
        let path = dir.join("search.txt");
        std::fs::write(&path, "rust is great\nrust is fast\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "rust");
        let search = app.search.as_ref().unwrap();
        assert_eq!(search.matches.len(), 2);
        assert!(search.current_idx.is_some());
        assert_eq!(search.current_idx, Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn counter_shows_no_matches() {
        let dir = temp_dir("search_nomatch");
        let path = dir.join("search.txt");
        std::fs::write(&path, "hello world\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "xyz");
        let search = app.search.as_ref().unwrap();
        assert!(search.matches.is_empty());
        assert!(search.current_idx.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn enter_navigates_next_match_with_wrap() {
        let dir = temp_dir("search_next");
        let path = dir.join("search.txt");
        std::fs::write(&path, "rust is great\nrust is fast\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "rust");
        // After typing, search_reset_matches positions at match 0.
        assert_eq!(app.search.as_ref().unwrap().current_idx, Some(0));
        // First Enter advances to match 1.
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.search.as_ref().unwrap().current_idx, Some(1));
        let line_after_second = app.cursor_line;
        // Second Enter wraps around to match 0.
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.search.as_ref().unwrap().current_idx, Some(0));
        assert_ne!(app.cursor_line, line_after_second);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn f3_navigates_next_match() {
        let dir = temp_dir("search_f3_next");
        let path = dir.join("search_f3.txt");
        std::fs::write(&path, "rust is great\nrust is fast\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "rust");
        assert_eq!(app.search.as_ref().unwrap().current_idx, Some(0));
        // F3 is the function-key fallback for Enter = next match.
        app.on_key(plain(KeyCode::F(3)));
        assert_eq!(app.search.as_ref().unwrap().current_idx, Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shift_enter_navigates_prev_match() {
        let dir = temp_dir("search_prev");
        let path = dir.join("search.txt");
        std::fs::write(&path, "rust is great\nrust is fast\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "rust");
        // After typing, positioned at match 0.
        assert_eq!(app.search.as_ref().unwrap().current_idx, Some(0));
        // Advance to match 1.
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.search.as_ref().unwrap().current_idx, Some(1));
        // Shift+Enter goes back to match 0.
        app.on_key(shift(KeyCode::Enter));
        assert_eq!(app.search.as_ref().unwrap().current_idx, Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shift_f3_navigates_prev_match() {
        let dir = temp_dir("search_shift_f3");
        let path = dir.join("shift_f3.txt");
        std::fs::write(&path, "rust one\nrust two\nrust three\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "rust");
        assert_eq!(app.search.as_ref().unwrap().current_idx, Some(0));
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.search.as_ref().unwrap().current_idx, Some(1));
        // Shift+F3 = previous match (function-key fallback for Shift+Enter).
        // Three matches keep this unambiguous: next would wrap to 2, prev is 0.
        app.on_key(shift(KeyCode::F(3)));
        assert_eq!(app.search.as_ref().unwrap().current_idx, Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn esc_restores_cursor_when_no_navigation() {
        let dir = temp_dir("search_esc_noop");
        let path = dir.join("search.txt");
        std::fs::write(&path, "rust is great\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.cursor_line = 0;
        app.cursor_col = 0;
        app.on_key(ctrl(KeyCode::Char('f')));
        // No navigation yet; Esc should restore cursor.
        app.on_key(plain(KeyCode::Esc));
        assert!(app.search.is_none());
        assert_eq!(app.cursor_line, 0);
        assert_eq!(app.cursor_col, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn esc_keeps_match_position_after_navigation() {
        let dir = temp_dir("search_esc_keep");
        let path = dir.join("search.txt");
        std::fs::write(&path, "rust is great\nrust is fast\n").expect("pre-create");
        let app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        let mut app = app;
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "rust");
        // Navigate to a match first.
        app.on_key(plain(KeyCode::Enter));
        assert!(app.search.as_ref().unwrap().navigated);
        let line_after_nav = app.cursor_line;
        // Esc should keep the cursor at the match position.
        app.on_key(plain(KeyCode::Esc));
        assert!(app.search.is_none());
        assert_eq!(app.cursor_line, line_after_nav);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn highlight_spans_present_on_visible_line() {
        // Test compose_highlight_spans directly (pure helper, no rendering needed).
        let line_text = "hello rust world";
        let matches = vec![MatchPos {
            line: 0,
            start_col: 6,
            end_col: 10,
            doc_index: 0,
        }];
        let plain_span = Span::styled(line_text.to_string(), Style::default());
        let spans = App::compose_highlight_spans(
            line_text,
            &[],
            0,
            Color::Rgb(0x61, 0x32, 0x14),
            Color::Rgb(0x9E, 0x6A, 0x03),
            &[plain_span.clone()],
        );
        assert!(spans.len() >= 1);
        let spans = App::compose_highlight_spans(
            line_text,
            &[&matches[0]],
            0,
            Color::Rgb(0x61, 0x32, 0x14),
            Color::Rgb(0x9E, 0x6A, 0x03),
            &[plain_span],
        );
        assert!(spans.len() >= 3);
    }

    #[test]
    fn empty_query_no_op() {
        let dir = temp_dir("search_empty");
        let path = dir.join("search.txt");
        std::fs::write(&path, "rust is great\naxe stands out\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        // Empty query should produce no matches.
        assert!(app.search.as_ref().unwrap().matches.is_empty());
        // Typing then backspace to empty should keep no matches.
        typed(&mut app, "x");
        assert!(!app.search.as_ref().unwrap().matches.is_empty());
        app.on_key(plain(KeyCode::Backspace));
        assert!(app.search.as_ref().unwrap().matches.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn case_insensitive_default() {
        let dir = temp_dir("search_case");
        let path = dir.join("search.txt");
        std::fs::write(&path, "rust is great\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        // Uppercase query should find lowercase content by default.
        typed(&mut app, "RUST");
        assert_eq!(app.search.as_ref().unwrap().matches.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // R3-002
    #[test]
    fn shift_letter_appends_to_search_query() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        app.on_key(shift(KeyCode::Char('R')));
        assert_eq!(app.search.as_ref().unwrap().query, "R");
    }

    // R3-004
    #[test]
    fn syntax_fg_preserved_on_non_match_segments() {
        let line_text = "fn main() {}";
        let syntax_spans = vec![
            Span::styled("fn ", Style::default().fg(Color::Blue)),
            Span::styled("main", Style::default().fg(Color::Yellow)),
            Span::styled("() {}", Style::default().fg(Color::Blue)),
        ];
        let matches = vec![MatchPos {
            line: 0,
            start_col: 0,
            end_col: 2,
            doc_index: 0,
        }];
        let spans = App::compose_highlight_spans(
            line_text,
            &[&matches[0]],
            0,
            Color::Rgb(0x61, 0x32, 0x14),
            Color::Rgb(0x9E, 0x6A, 0x03),
            &syntax_spans,
        );
        let fn_span = spans
            .iter()
            .find(|s| s.content.as_ref() == "fn")
            .expect("fn");
        assert_eq!(fn_span.style.bg, Some(Color::Rgb(0x9E, 0x6A, 0x03)));
        assert_eq!(fn_span.style.fg, Some(Color::Blue));
        let main_span = spans
            .iter()
            .find(|s| s.content.as_ref() == "main")
            .expect("main");
        assert_eq!(main_span.style.fg, Some(Color::Yellow));
        assert_eq!(main_span.style.bg, None);
    }

    // R3-003
    #[test]
    fn search_dialog_renders_on_top_of_editor() {
        let dir = temp_dir("search_dialog_draw_order");
        let path = dir.join("search_draw_order.txt");
        std::fs::write(&path, "hello world\nfoo bar baz\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "bar");
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 10)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let dialog_cell = terminal.backend().buffer().get(0, 1);
        assert_eq!(dialog_cell.symbol(), "F");
        assert_eq!(dialog_cell.style().bg, Some(Color::Rgb(0xE2, 0xC0, 0x89)));
        let _ = dir;
    }

    // Regression: the dialog Paragraph's base style was rendered over the
    // whole editor area, washing the entire screen in the prompt background.
    #[test]
    fn search_dialog_paints_only_its_own_row() {
        let dir = temp_dir("search_dialog_row_scope");
        let path = dir.join("search_row_scope.txt");
        std::fs::write(&path, "alpha beta\ngamma delta\nepsilon zeta\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "delta");
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 10)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        // The dialog bar occupies only the first editor row (frame row 1).
        let dialog_cell = terminal.backend().buffer().get(0, 1);
        assert_eq!(dialog_cell.style().bg, Some(Color::Rgb(0xE2, 0xC0, 0x89)));
        // Rows below the dialog keep the editor background, not the prompt one.
        for row in [2u16, 5, 8] {
            let cell = terminal.backend().buffer().get(0, row);
            assert_eq!(
                cell.style().bg,
                Some(Color::Rgb(0x1E, 0x1E, 0x1E)),
                "row {row} must keep the editor background"
            );
        }
        let _ = dir;
    }

    // Status bar must expose the package version and the build stamp so the
    // owner can tell at a glance which build is under manual test.
    #[test]
    fn status_bar_shows_version_and_build_id() {
        let app = App::new(vec![Document::new_untitled()], Lang::En);
        let (_, right) = app.status_line(80);
        let marker = format!(
            "v{}·{}",
            env!("CARGO_PKG_VERSION"),
            option_env!("EDITORMINI_BUILD_ID").unwrap_or("dev")
        );
        assert!(
            right.contains(&marker),
            "status right side must contain version marker {marker:?}, got {right:?}"
        );
    }

    // Regression: dialog text used the editor's light-gray fg and the
    // mid-gray hint fg on the amber prompt background — unreadable.
    #[test]
    fn search_dialog_text_is_dark_on_prompt_background() {
        let dir = temp_dir("search_dialog_text_contrast");
        let path = dir.join("search_text_contrast.txt");
        std::fs::write(&path, "alpha beta\ngamma delta\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "delta");
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 10)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        // Every cell on the dialog row must use the dark prompt fg — no
        // light-gray text (0xD4D4D4) or mid-gray hints (0x858585) on amber.
        for col in 0..80u16 {
            let fg = terminal.backend().buffer().get(col, 1).style().fg;
            assert_ne!(fg, Some(Color::Rgb(0xD4, 0xD4, 0xD4)), "col {col}");
            assert_ne!(fg, Some(Color::Rgb(0x85, 0x85, 0x85)), "col {col}");
        }
        let _ = dir;
    }

    // The replace dialog must make the ACTIVE field obvious: brackets wrap
    // the focused field's label+value, no cryptic ">"/"<" marker.
    #[test]
    fn replace_dialog_marks_active_field_with_brackets() {
        let dir = temp_dir("replace_dialog_brackets");
        let path = dir.join("replace_brackets.txt");
        std::fs::write(&path, "alpha beta\n").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "beta");
        app.on_key(ctrl(KeyCode::Char('h')));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 10)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let row = |t: &ratatui::Terminal<ratatui::backend::TestBackend>| {
            let mut s = String::new();
            for col in 0..80u16 {
                s.push_str(t.backend().buffer().get(col, 1).symbol());
            }
            s
        };
        // Query focused: its label+value are bracketed.
        assert!(
            row(&terminal).contains("[Query: beta]"),
            "row: {}",
            row(&terminal)
        );
        // Switch to the replacement field: the brackets move there.
        app.on_key(plain(KeyCode::Tab));
        terminal.draw(|f| app.draw(f)).unwrap();
        let r = row(&terminal);
        assert!(r.contains("[Replace with:"), "row: {r}");
        assert!(
            !r.contains('>') && !r.contains('<'),
            "no cryptic marker: {r}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // R3-005: Ctrl+H opens the replace dialog.
    #[test]
    fn ctrl_h_opens_replace_dialog() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        assert!(app.search.is_none());
        app.on_key(ctrl(KeyCode::Char('h')));
        assert!(app.search.is_some());
        let search = app.search.as_ref().unwrap();
        assert_eq!(search.mode, SearchMode::Replace);
        assert_eq!(search.field_focus, ReplaceField::Query);
    }

    // R3-006: Ctrl+H pre-fills query from an active search.
    #[test]
    fn ctrl_h_prefills_query_from_active_search() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "hello");
        assert_eq!(app.search.as_ref().unwrap().query, "hello");
        app.on_key(ctrl(KeyCode::Char('h')));
        let search = app.search.as_ref().unwrap();
        assert_eq!(search.mode, SearchMode::Replace);
        assert_eq!(search.query, "hello");
    }

    // R3-007: Tab switches field focus in replace dialog.
    #[test]
    fn tab_switches_replace_field_focus() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl(KeyCode::Char('h')));
        assert_eq!(
            app.search.as_ref().unwrap().field_focus,
            ReplaceField::Query
        );
        app.on_key(plain(KeyCode::Tab));
        assert_eq!(
            app.search.as_ref().unwrap().field_focus,
            ReplaceField::Replacement
        );
        app.on_key(plain(KeyCode::Tab));
        assert_eq!(
            app.search.as_ref().unwrap().field_focus,
            ReplaceField::Query
        );
    }

    // R3-008: Shift+Tab switches field focus back in replace dialog.
    #[test]
    fn shift_tab_switches_replace_field_focus_back() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl(KeyCode::Char('h')));
        app.on_key(plain(KeyCode::Tab));
        assert_eq!(
            app.search.as_ref().unwrap().field_focus,
            ReplaceField::Replacement
        );
        app.on_key(shift(KeyCode::Tab));
        assert_eq!(
            app.search.as_ref().unwrap().field_focus,
            ReplaceField::Query
        );
    }

    // R3-009: Ctrl+Enter replaces current match and advances.
    #[test]
    fn ctrl_enter_replaces_current_match_and_advances() {
        let dir = temp_dir("replace_current");
        let path = dir.join("replace_current.txt");
        std::fs::write(&path, "hello world hello").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        // Open search, find matches, then switch to replace.
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "hello");
        assert!(app.search.as_ref().unwrap().matches.len() >= 2);
        app.on_key(ctrl(KeyCode::Char('h')));
        assert_eq!(app.search.as_ref().unwrap().mode, SearchMode::Replace);
        // Switch to replacement field and type replacement text.
        app.on_key(plain(KeyCode::Tab));
        typed(&mut app, "HI");
        // Ctrl+Enter replaces current match and advances.
        app.on_key(ctrl(KeyCode::Enter));
        // The replacement should have been applied.
        let doc = app.doc();
        assert!(doc.line(0).contains("HI"));
        // Counter should have refreshed (matches decreased).
        let search = app.search.as_ref().unwrap();
        assert!(search.matches.len() >= 1);
        let _ = dir;
    }

    // R3-010: Replace-all via confirm modal replaces all and Ctrl+Z restores.
    #[test]
    fn replace_all_confirms_and_restores_with_undo() {
        let dir = temp_dir("replace_all_undo");
        let path = dir.join("replace_all_undo.txt");
        std::fs::write(&path, "hello world hello").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        // Open replace dialog with pre-filled query.
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "hello");
        let matches_before = app.search.as_ref().unwrap().matches.len();
        assert!(
            matches_before >= 2,
            "expected >= 2 matches, got {matches_before}"
        );
        app.on_key(ctrl(KeyCode::Char('h')));
        // Switch to replacement field and type replacement.
        app.on_key(plain(KeyCode::Tab));
        typed(&mut app, "HI");
        let repl = app.search.as_ref().unwrap().replacement.clone();
        assert_eq!(repl, "HI", "replacement field should contain HI");
        // F6 requests replace-all confirmation.
        app.on_key(plain(KeyCode::F(6)));
        assert!(app.confirm.is_some());
        // Confirm with Y.
        app.on_key(plain(KeyCode::Char('y')));
        // All occurrences replaced.
        let doc = app.doc();
        eprintln!("DEBUG: doc line after replace-all: '{}'", doc.line(0));
        eprintln!("DEBUG: search is_none: {}", app.search.is_none());
        assert!(
            doc.line(0).contains("HI world HI"),
            "doc line: {}",
            doc.line(0)
        );
        assert!(!doc.line(0).contains("hello"));
        // Ctrl+Z restores original text.
        app.on_key(ctrl(KeyCode::Char('z')));
        let doc_after_undo = app.doc();
        assert!(doc_after_undo.line(0).contains("hello world hello"));
        let _ = dir;
    }

    // R3-011: Empty replacement deletes matches.
    #[test]
    fn empty_replacement_deletes_matches() {
        let dir = temp_dir("empty_replace");
        let path = dir.join("empty_replace.txt");
        std::fs::write(&path, "hello world hello").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "hello");
        app.on_key(ctrl(KeyCode::Char('h')));
        // Switch to replacement field (empty by default).
        app.on_key(plain(KeyCode::Tab));
        assert!(app.search.as_ref().unwrap().replacement.is_empty());
        // F6 requests replace-all confirmation.
        app.on_key(plain(KeyCode::F(6)));
        app.on_key(plain(KeyCode::Char('y')));
        // All "hello" occurrences deleted.
        let doc = app.doc();
        assert!(!doc.line(0).contains("hello"));
        assert!(doc.line(0).contains("world"));
        let _ = dir;
    }

    // R3-012: Uppercase query works in replace dialog.
    #[test]
    fn uppercase_query_works_in_replace_dialog() {
        let dir = temp_dir("uppercase_replace");
        let path = dir.join("uppercase_replace.txt");
        std::fs::write(&path, "Hello World Hello").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "Hello");
        app.on_key(ctrl(KeyCode::Char('h')));
        // Switch to replacement field and type uppercase.
        app.on_key(plain(KeyCode::Tab));
        typed(&mut app, "BYE");
        // F6 replace-all.
        app.on_key(plain(KeyCode::F(6)));
        app.on_key(plain(KeyCode::Char('y')));
        let doc = app.doc();
        assert!(doc.line(0).contains("BYE World BYE"));
        let _ = dir;
    }

    // R3-013: Replace counter refreshes after replaces.
    #[test]
    fn replace_counter_refreshes_after_replaces() {
        let dir = temp_dir("replace_counter");
        let path = dir.join("replace_counter.txt");
        std::fs::write(&path, "hello hello hello").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "hello");
        let initial_count = app.search.as_ref().unwrap().matches.len();
        assert_eq!(initial_count, 3);
        app.on_key(ctrl(KeyCode::Char('h')));
        app.on_key(plain(KeyCode::Tab));
        typed(&mut app, "HI");
        app.on_key(plain(KeyCode::F(6)));
        app.on_key(plain(KeyCode::Char('y')));
        // After replace-all, search state is cleared.
        assert!(app.search.is_none());
        let _ = dir;
    }

    // R3-014: F5 replaces current match (fallback for Ctrl+Enter).
    #[test]
    fn f5_replaces_current_match_fallback() {
        let dir = temp_dir("f5_replace");
        let path = dir.join("f5_replace.txt");
        std::fs::write(&path, "hello world hello").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "hello");
        app.on_key(ctrl(KeyCode::Char('h')));
        app.on_key(plain(KeyCode::Tab));
        typed(&mut app, "HI");
        // F5 replaces current match.
        app.on_key(plain(KeyCode::F(5)));
        let doc = app.doc();
        assert!(doc.line(0).contains("HI"));
        let _ = dir;
    }

    // Task 3: Ctrl+Shift+F opens cross-doc find dialog.
    #[test]
    fn ctrl_shift_f_opens_cross_doc_find() {
        let mut app = App::new(
            vec![Document::new_untitled(), Document::new_untitled()],
            Lang::En,
        );
        app.on_key(ctrl_shift(KeyCode::Char('f')));
        assert!(app.search.is_some());
        assert!(app.search.as_ref().unwrap().cross_doc);
        assert_eq!(app.search.as_ref().unwrap().start_doc, 0);
    }

    // Task 3: Cross-doc navigation switches active tab.
    #[test]
    fn cross_doc_navigation_switches_tabs() {
        let dir = temp_dir("cross_doc_nav");
        let path1 = dir.join("doc1.txt");
        let path2 = dir.join("doc2.txt");
        std::fs::write(&path1, "hello world").expect("pre-create");
        std::fs::write(&path2, "hello there").expect("pre-create");
        let mut app = App::new(
            vec![
                Document::from_path(&path1).expect("load1"),
                Document::from_path(&path2).expect("load2"),
            ],
            Lang::En,
        );
        assert_eq!(app.active, 0);
        app.on_key(ctrl_shift(KeyCode::Char('f')));
        typed(&mut app, "hello");
        // First match is in doc 0 (active). Press Enter to go to next match.
        app.on_key(plain(KeyCode::Enter));
        // Second match is in doc 1 — navigation should switch tabs.
        assert_eq!(app.active, 1);
        let _ = dir;
    }

    // Task 3: Cross-doc wrap-around.
    #[test]
    fn cross_doc_navigation_wraps_around() {
        let dir = temp_dir("cross_doc_wrap");
        let path1 = dir.join("doc1.txt");
        let path2 = dir.join("doc2.txt");
        std::fs::write(&path1, "hello").expect("pre-create");
        std::fs::write(&path2, "hello").expect("pre-create");
        let mut app = App::new(
            vec![
                Document::from_path(&path1).expect("load1"),
                Document::from_path(&path2).expect("load2"),
            ],
            Lang::En,
        );
        app.on_key(ctrl_shift(KeyCode::Char('f')));
        typed(&mut app, "hello");
        // 2 matches total, one per doc. Navigate past the last match.
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.active, 1);
        app.on_key(plain(KeyCode::Enter));
        // Should wrap back to doc 0.
        assert_eq!(app.active, 0);
        let _ = dir;
    }

    // Task 3: Esc restores starting doc for cross-doc if un-navigated.
    #[test]
    fn cross_doc_esc_restores_starting_doc_when_unnavigated() {
        let dir = temp_dir("cross_doc_esc");
        let path1 = dir.join("doc1.txt");
        let path2 = dir.join("doc2.txt");
        std::fs::write(&path1, "hello\nworld\nfoo\nbar\nbaz").expect("pre-create");
        std::fs::write(&path2, "world").expect("pre-create");
        let mut app = App::new(
            vec![
                Document::from_path(&path1).expect("load1"),
                Document::from_path(&path2).expect("load2"),
            ],
            Lang::En,
        );
        app.cursor_line = 3;
        app.cursor_col = 1;
        app.on_key(ctrl_shift(KeyCode::Char('f')));
        // Don't navigate — just close with Esc.
        app.on_key(plain(KeyCode::Esc));
        // Should be back on doc 0 with cursor restored.
        assert_eq!(app.active, 0);
        assert_eq!(app.cursor_line, 3);
        assert_eq!(app.cursor_col, 1);
        let _ = dir;
    }

    // Task 3: Esc does NOT restore starting doc when navigation happened.
    #[test]
    fn cross_doc_esc_does_not_restore_when_navigated() {
        let dir = temp_dir("cross_doc_esc_nav");
        let path1 = dir.join("doc1.txt");
        let path2 = dir.join("doc2.txt");
        std::fs::write(&path1, "hello").expect("pre-create");
        std::fs::write(&path2, "hello").expect("pre-create");
        let mut app = App::new(
            vec![
                Document::from_path(&path1).expect("load1"),
                Document::from_path(&path2).expect("load2"),
            ],
            Lang::En,
        );
        app.on_key(ctrl_shift(KeyCode::Char('f')));
        typed(&mut app, "hello");
        // Navigate to a match (switches to doc 1).
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.active, 1);
        // Now Esc — should NOT restore doc 0 cursor because we navigated.
        app.on_key(plain(KeyCode::Esc));
        assert!(app.search.is_none());
        // We should still be on doc 1 (not restored to doc 0).
        assert_eq!(app.active, 1);
        let _ = dir;
    }

    // Task 4: Ctrl+Shift+H opens cross-doc replace dialog.
    #[test]
    fn ctrl_shift_h_opens_cross_doc_replace() {
        let mut app = App::new(
            vec![Document::new_untitled(), Document::new_untitled()],
            Lang::En,
        );
        app.on_key(ctrl_shift(KeyCode::Char('h')));
        assert!(app.search.is_some());
        assert!(app.search.as_ref().unwrap().cross_doc);
        assert_eq!(app.search.as_ref().unwrap().mode, SearchMode::Replace);
    }

    // Task 4: Cross-doc replace-current-and-advance switches tabs.
    #[test]
    fn cross_doc_replace_current_advances_and_switches_tabs() {
        let dir = temp_dir("cross_doc_replace_current");
        let path1 = dir.join("doc1.txt");
        let path2 = dir.join("doc2.txt");
        std::fs::write(&path1, "hello world").expect("pre-create");
        std::fs::write(&path2, "hello there").expect("pre-create");
        let mut app = App::new(
            vec![
                Document::from_path(&path1).expect("load1"),
                Document::from_path(&path2).expect("load2"),
            ],
            Lang::En,
        );
        app.on_key(ctrl_shift(KeyCode::Char('h')));
        typed(&mut app, "hello");
        // Switch to replacement field and type replacement.
        app.on_key(plain(KeyCode::Tab));
        typed(&mut app, "HI");
        // Ctrl+Enter replaces current match and advances.
        app.on_key(ctrl(KeyCode::Enter));
        // First match was in doc 0, replaced. Next match is in doc 1.
        assert_eq!(app.active, 1);
        // Replacements are in-memory until an explicit save.
        let doc1: String = (0..app.docs[0].line_count())
            .map(|i| app.docs[0].line(i))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(doc1.contains("HI world"));
        let _ = dir;
    }

    // Task 4: Cross-doc replace-all with confirm updates all docs.
    #[test]
    fn cross_doc_replace_all_confirms_and_replaces_in_all_docs() {
        let dir = temp_dir("cross_doc_replace_all");
        let path1 = dir.join("doc1.txt");
        let path2 = dir.join("doc2.txt");
        std::fs::write(&path1, "hello hello").expect("pre-create");
        std::fs::write(&path2, "hello world hello").expect("pre-create");
        let mut app = App::new(
            vec![
                Document::from_path(&path1).expect("load1"),
                Document::from_path(&path2).expect("load2"),
            ],
            Lang::En,
        );
        app.on_key(ctrl_shift(KeyCode::Char('h')));
        typed(&mut app, "hello");
        // Switch to replacement field.
        app.on_key(plain(KeyCode::Tab));
        typed(&mut app, "HI");
        // F6 requests replace-all confirmation.
        app.on_key(plain(KeyCode::F(6)));
        assert!(app.confirm.is_some());
        // Confirm with Y.
        app.on_key(plain(KeyCode::Char('y')));
        // Both docs should have replacements in memory (save is explicit).
        let doc_text = |i: usize| -> String {
            (0..app.docs[i].line_count())
                .map(|l| app.docs[i].line(l))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let doc1 = doc_text(0);
        let doc2 = doc_text(1);
        assert!(doc1.contains("HI HI"));
        assert!(doc2.contains("HI world HI"));
        assert!(!doc1.contains("hello"));
        assert!(!doc2.contains("hello"));
        let _ = dir;
    }

    // Task 4: Per-doc undo restores each doc independently.
    #[test]
    fn cross_doc_replace_all_per_doc_undo_restores() {
        let dir = temp_dir("cross_doc_undo");
        let path1 = dir.join("doc1.txt");
        let path2 = dir.join("doc2.txt");
        std::fs::write(&path1, "hello hello").expect("pre-create");
        std::fs::write(&path2, "hello world hello").expect("pre-create");
        let mut app = App::new(
            vec![
                Document::from_path(&path1).expect("load1"),
                Document::from_path(&path2).expect("load2"),
            ],
            Lang::En,
        );
        app.on_key(ctrl_shift(KeyCode::Char('h')));
        typed(&mut app, "hello");
        app.on_key(plain(KeyCode::Tab));
        typed(&mut app, "HI");
        app.on_key(plain(KeyCode::F(6)));
        app.on_key(plain(KeyCode::Char('y')));
        // Undo should restore doc 0 (current active doc after replace-all).
        // Assert in-memory: the file was never saved (explicit-save design).
        app.on_key(ctrl(KeyCode::Char('z')));
        let doc1: String = (0..app.docs[0].line_count())
            .map(|i| app.docs[0].line(i))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(doc1.contains("hello hello"));
        assert!(!doc1.contains("HI"));
        let _ = dir;
    }

    // Task 4: Read-only docs are skipped in cross-doc replace-all.
    #[test]
    fn cross_doc_replace_all_skips_read_only_docs() {
        let dir = temp_dir("cross_doc_skip_readonly");
        let path1 = dir.join("doc1.txt");
        let path2 = dir.join("doc2.txt");
        std::fs::write(&path1, "hello hello").expect("pre-create");
        std::fs::write(&path2, "hello world hello").expect("pre-create");
        let mut app = App::new(
            vec![
                Document::from_path(&path1).expect("load1"),
                Document::from_path(&path2).expect("load2"),
            ],
            Lang::En,
        );
        // Mark doc 1 as read-only.
        app.docs[1].set_read_only();
        app.on_key(ctrl_shift(KeyCode::Char('h')));
        typed(&mut app, "hello");
        app.on_key(plain(KeyCode::Tab));
        typed(&mut app, "HI");
        app.on_key(plain(KeyCode::F(6)));
        app.on_key(plain(KeyCode::Char('y')));
        // Doc 0 should be replaced, doc 1 (read-only) should be untouched.
        // Assert in-memory (replacements apply to buffers, not files).
        let doc_text = |i: usize| -> String {
            (0..app.docs[i].line_count())
                .map(|l| app.docs[i].line(l))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let doc1 = doc_text(0);
        let doc2 = doc_text(1);
        assert!(doc1.contains("HI HI"));
        assert!(doc2.contains("hello"));
        let _ = dir;
    }

    // Task 4: Cross-doc replace dialog pre-filled from a previous active-doc
    // search must compute matches across ALL docs (cross_doc flag must be
    // visible to find_matches before the initial match computation).
    #[test]
    fn cross_doc_replace_prefill_matches_span_all_docs() {
        let dir = temp_dir("cross_doc_prefill");
        let path1 = dir.join("doc1.txt");
        let path2 = dir.join("doc2.txt");
        std::fs::write(&path1, "hello").expect("pre-create");
        std::fs::write(&path2, "hello again").expect("pre-create");
        let mut app = App::new(
            vec![
                Document::from_path(&path1).expect("load1"),
                Document::from_path(&path2).expect("load2"),
            ],
            Lang::En,
        );
        // Seed a previous ACTIVE-doc search with the query (pre-fill source).
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "hello");
        // Cross-doc replace pre-fills the query without retyping it.
        app.on_key(ctrl_shift(KeyCode::Char('h')));
        // Navigation must cross into doc 1: the match set spans both docs.
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.active, 1);
        let _ = dir;
    }

    // Task 4: Docs with zero matches are untouched in cross-doc replace-all.
    #[test]
    fn cross_doc_replace_all_leaves_zero_match_docs_untouched() {
        let dir = temp_dir("cross_doc_zero_match");
        let path1 = dir.join("doc1.txt");
        let path2 = dir.join("doc2.txt");
        std::fs::write(&path1, "hello hello").expect("pre-create");
        std::fs::write(&path2, "no match here").expect("pre-create");
        let mut app = App::new(
            vec![
                Document::from_path(&path1).expect("load1"),
                Document::from_path(&path2).expect("load2"),
            ],
            Lang::En,
        );
        app.on_key(ctrl_shift(KeyCode::Char('h')));
        typed(&mut app, "hello");
        app.on_key(plain(KeyCode::Tab));
        typed(&mut app, "HI");
        app.on_key(plain(KeyCode::F(6)));
        app.on_key(plain(KeyCode::Char('y')));
        let doc2 = std::fs::read_to_string(&path2).expect("read");
        assert_eq!(doc2.trim(), "no match here");
        let _ = dir;
    }

    // Task 3+4: Uppercase query works in cross-doc search.
    #[test]
    fn uppercase_query_works_in_cross_doc_search() {
        let dir = temp_dir("uppercase_cross_doc");
        let path1 = dir.join("doc1.txt");
        let path2 = dir.join("doc2.txt");
        std::fs::write(&path1, "Hello World").expect("pre-create");
        std::fs::write(&path2, "HELLO THERE").expect("pre-create");
        let mut app = App::new(
            vec![
                Document::from_path(&path1).expect("load1"),
                Document::from_path(&path2).expect("load2"),
            ],
            Lang::En,
        );
        app.on_key(ctrl_shift(KeyCode::Char('f')));
        typed(&mut app, "HELLO");
        // Case-insensitive by default, so both docs should have matches.
        let search = app.search.as_ref().unwrap();
        assert!(search.matches.len() >= 2);
        let _ = dir;
    }

    // Task 3+4: Empty query is a no-op in cross-doc search.
    #[test]
    fn empty_query_no_op_in_cross_doc_search() {
        let mut app = App::new(
            vec![Document::new_untitled(), Document::new_untitled()],
            Lang::En,
        );
        app.on_key(ctrl_shift(KeyCode::Char('f')));
        // Empty query — no matches.
        let search = app.search.as_ref().unwrap();
        assert!(search.matches.is_empty());
    }

    // ============ T1: Selection state per tab ============

    #[test]
    fn shift_right_extends_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        // Cursor at col 5. Start selection with Shift+Left.
        app.on_key(shift(KeyCode::Left));
        let sel = app.normalized_selection().unwrap();
        assert_eq!(sel, ((0, 4), (0, 5)));
        // Extend with Shift+Left again.
        app.on_key(shift(KeyCode::Left));
        let sel = app.normalized_selection().unwrap();
        assert_eq!(sel, ((0, 3), (0, 5)));
    }

    #[test]
    fn shift_left_extends_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        // Move cursor left first so we have room to select leftward.
        app.on_key(plain(KeyCode::Left));
        // Cursor at col 4. Start selection with Shift+Right.
        app.on_key(shift(KeyCode::Right));
        let sel = app.normalized_selection().unwrap();
        assert_eq!(sel, ((0, 4), (0, 5)));
    }

    #[test]
    fn shift_up_down_extend_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "abc\ndef");
        // Cursor at line 1, col 3. Start selection with Shift+Up.
        app.on_key(shift(KeyCode::Up));
        let sel = app.normalized_selection().unwrap();
        assert_eq!(sel, ((0, 3), (1, 3)));
    }

    #[test]
    fn shift_home_end_extend_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        // Cursor at col 5. Shift+Home selects to start of line.
        app.on_key(shift(KeyCode::Home));
        let sel = app.normalized_selection().unwrap();
        assert_eq!(sel, ((0, 0), (0, 5)));
        // Shift+End returns the head to the anchor: VS Code keeps the anchor
        // fixed during selection, so the selection collapses (empty range).
        app.on_key(shift(KeyCode::End));
        let sel = app.normalized_selection().unwrap();
        assert_eq!(sel, ((0, 5), (0, 5)));
    }

    #[test]
    fn plain_movement_clears_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        app.on_key(shift(KeyCode::Left));
        assert!(app.normalized_selection().is_some());
        // Plain Left clears the selection.
        app.on_key(plain(KeyCode::Left));
        assert!(app.normalized_selection().is_none());
    }

    #[test]
    fn typing_clears_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        app.on_key(shift(KeyCode::Left));
        assert!(app.normalized_selection().is_some());
        typed(&mut app, "x");
        assert!(app.normalized_selection().is_none());
        assert_eq!(app.doc().line(0), "hellxo");
    }

    #[test]
    fn backspace_clears_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        app.on_key(shift(KeyCode::Left));
        assert!(app.normalized_selection().is_some());
        app.on_key(plain(KeyCode::Backspace));
        assert!(app.normalized_selection().is_none());
    }

    #[test]
    fn delete_forward_clears_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        app.on_key(shift(KeyCode::Left));
        assert!(app.normalized_selection().is_some());
        app.on_key(plain(KeyCode::Delete));
        assert!(app.normalized_selection().is_none());
    }

    #[test]
    fn undo_clears_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        app.on_key(shift(KeyCode::Left));
        assert!(app.normalized_selection().is_some());
        app.on_key(ctrl(KeyCode::Char('z')));
        assert!(app.normalized_selection().is_none());
    }

    #[test]
    fn redo_clears_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        app.on_key(ctrl(KeyCode::Char('z')));
        app.on_key(shift(KeyCode::Left));
        assert!(app.normalized_selection().is_some());
        app.on_key(ctrl(KeyCode::Char('y')));
        assert!(app.normalized_selection().is_none());
    }

    #[test]
    fn esc_clears_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        app.on_key(shift(KeyCode::Left));
        assert!(app.normalized_selection().is_some());
        app.on_key(plain(KeyCode::Esc));
        assert!(app.normalized_selection().is_none());
    }

    #[test]
    fn selection_persists_across_tab_switches() {
        let mut app = App::new(
            vec![Document::new_untitled(), Document::new_untitled()],
            Lang::En,
        );
        typed(&mut app, "hello");
        app.on_key(shift(KeyCode::Left));
        assert!(app.normalized_selection().is_some());
        app.on_key(ctrl(KeyCode::PageDown));
        // Tab 1 has no selection.
        assert!(app.normalized_selection().is_none());
        app.on_key(ctrl(KeyCode::PageUp));
        // Back to tab 0, selection is preserved.
        assert!(app.normalized_selection().is_some());
    }

    #[test]
    fn normalized_selection_orders_anchor_head() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        // Select right-to-left (anchor > head).
        app.on_key(shift(KeyCode::Left));
        let sel = app.normalized_selection().unwrap();
        assert_eq!(sel.0, (0, 4)); // start is the smaller
        assert_eq!(sel.1, (0, 5)); // end is the larger
    }

    // ============ T2: Render selection with theme ============

    #[test]
    fn selection_bg_rendered_dark_plus() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.set_theme(ThemeChoice::DarkPlus);
        typed(&mut app, "hello");
        // Move cursor to col 1, then Shift+Right 4 times to select "ello" (cols 1-5).
        app.on_key(plain(KeyCode::Home));
        app.on_key(plain(KeyCode::Right));
        app.on_key(shift(KeyCode::Right));
        app.on_key(shift(KeyCode::Right));
        app.on_key(shift(KeyCode::Right));
        app.on_key(shift(KeyCode::Right));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 10)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        // Cell at col 3 is the first SELECTED char ('e': gutter 2 + col 1)
        // and must have Dark+ selection_bg = 0x264F78.
        let cell = terminal.backend().buffer().get(3, 1);
        assert_eq!(cell.style().bg, Some(Color::Rgb(0x26, 0x4F, 0x78)));
    }

    #[test]
    fn selection_bg_rendered_light_plus() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.set_theme(ThemeChoice::LightPlus);
        typed(&mut app, "hello");
        // Move cursor to col 1, then Shift+Right 4 times to select "ello" (cols 1-5).
        app.on_key(plain(KeyCode::Home));
        app.on_key(plain(KeyCode::Right));
        app.on_key(shift(KeyCode::Right));
        app.on_key(shift(KeyCode::Right));
        app.on_key(shift(KeyCode::Right));
        app.on_key(shift(KeyCode::Right));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 10)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        // Cell at col 3 is the first SELECTED char ('e') and must have
        // Light+ selection_bg = 0xADD8E6.
        let cell = terminal.backend().buffer().get(3, 1);
        assert_eq!(cell.style().bg, Some(Color::Rgb(0xAD, 0xD8, 0xE6)));
    }

    #[test]
    fn selection_bg_does_not_override_search_match_bg() {
        let dir = temp_dir("sel_vs_search");
        let path = dir.join("sel_vs_search.txt");
        std::fs::write(&path, "xx\nhello world").expect("pre-create");
        let mut app = App::new(vec![Document::from_path(&path).expect("load")], Lang::En);
        app.set_theme(ThemeChoice::DarkPlus);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 10)).unwrap();
        // Prime the viewport: a real app is drawn between keystrokes, so
        // clamp_scroll has a real viewport height to work with.
        terminal.draw(|f| app.draw(f)).unwrap();
        // Select the whole second line BEFORE opening search: movement keys
        // are consumed by the dialog while it is open.
        app.on_key(plain(KeyCode::Down));
        app.on_key(plain(KeyCode::Home));
        app.on_key(shift(KeyCode::End));
        // Search for "world": it sits on line 1, cols 6-11, inside the
        // selection.
        app.on_key(ctrl(KeyCode::Char('f')));
        typed(&mut app, "world");
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 10)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        // Frame rows: 1 = dialog bar overlaying line 0, 2 = line 1; gutter 2.
        // The current match "world" (buffer col 2+6=8, row 2) keeps the
        // current-match bg even though it lies inside the selection...
        let match_cell = terminal.backend().buffer().get(8, 2);
        assert_eq!(match_cell.style().bg, Some(Color::Rgb(0x9E, 0x6A, 0x03)));
        // ...while selected non-match cells keep the selection bg.
        let sel_cell = terminal.backend().buffer().get(2, 2);
        assert_eq!(sel_cell.style().bg, Some(Color::Rgb(0x26, 0x4F, 0x78)));
        let _ = dir;
    }

    // ============ T3: Ctrl+Word movement ============

    #[test]
    fn ctrl_right_moves_to_next_word() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello world");
        // Move cursor to start of line first.
        app.on_key(plain(KeyCode::Home));
        // Ctrl+Right should go to col 6 (start of "world").
        app.on_key(ctrl(KeyCode::Right));
        assert_eq!(app.cursor_col, 6, "should land at start of next word");
    }

    #[test]
    fn ctrl_left_moves_to_prev_word_end() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello world");
        // Move cursor to start of "world" (col 6).
        app.on_key(ctrl(KeyCode::Right));
        // Ctrl+Left should go to end of "hello" (col 5, the space).
        app.on_key(ctrl(KeyCode::Left));
        assert_eq!(app.cursor_col, 5, "should land at end of previous word");
    }

    #[test]
    fn ctrl_right_crosses_whitespace() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello  world");
        // Move cursor to start of line first.
        app.on_key(plain(KeyCode::Home));
        // Ctrl+Right should skip double space.
        app.on_key(ctrl(KeyCode::Right));
        assert_eq!(
            app.cursor_col, 7,
            "should skip whitespace and land at start of next word"
        );
    }

    #[test]
    fn ctrl_right_stops_at_punctuation() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello, world");
        // Move cursor to start of line first.
        app.on_key(plain(KeyCode::Home));
        // Ctrl+Right should stop at the comma.
        app.on_key(ctrl(KeyCode::Right));
        assert_eq!(app.cursor_col, 5, "should stop at punctuation");
    }

    #[test]
    fn ctrl_left_crosses_whitespace() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello  world");
        // Move to start of "world" (col 7).
        app.on_key(ctrl(KeyCode::Right));
        // Ctrl+Left should skip double space and land at end of "hello".
        app.on_key(ctrl(KeyCode::Left));
        assert_eq!(
            app.cursor_col, 5,
            "should skip whitespace and land at end of prev word"
        );
    }

    #[test]
    fn ctrl_right_crosses_lines() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello\nworld");
        // Move to start of line 0 (typed leaves the cursor at doc end).
        app.on_key(plain(KeyCode::Up));
        app.on_key(plain(KeyCode::Home));
        // Ctrl+Right should cross to start of "world" on line 1.
        app.on_key(ctrl(KeyCode::Right));
        assert_eq!(app.cursor_line, 1);
        assert_eq!(app.cursor_col, 0, "should cross to start of next line");
    }

    #[test]
    fn ctrl_left_crosses_lines() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello\nworld");
        // Move to start of "world" (line 1, col 0).
        app.on_key(ctrl(KeyCode::Right));
        // Ctrl+Left should go to end of "hello" (line 0, col 5).
        app.on_key(ctrl(KeyCode::Left));
        assert_eq!(app.cursor_line, 0);
        assert_eq!(app.cursor_col, 5, "should cross to end of prev line");
    }

    #[test]
    fn ctrl_shift_right_extends_selection_by_word() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello world");
        // Move cursor to start of line first.
        app.on_key(plain(KeyCode::Home));
        // Ctrl+Shift+Right selects to start of "world".
        app.on_key(ctrl_shift(KeyCode::Right));
        let sel = app.normalized_selection().unwrap();
        assert_eq!(sel, ((0, 0), (0, 6)));
    }

    #[test]
    fn ctrl_shift_left_extends_selection_by_word() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello world");
        // Move to start of "world" (col 6): Home, then one word right.
        // (Ctrl+Right at doc end would be a no-op, like VS Code.)
        app.on_key(plain(KeyCode::Home));
        app.on_key(ctrl(KeyCode::Right));
        // Ctrl+Shift+Left selects back to end of "hello".
        app.on_key(ctrl_shift(KeyCode::Left));
        let sel = app.normalized_selection().unwrap();
        assert_eq!(sel, ((0, 5), (0, 6)));
    }

    #[test]
    fn ctrl_home_moves_to_doc_start() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        app.on_key(plain(KeyCode::End));
        assert_eq!(app.cursor_col, 5);
        app.on_key(ctrl(KeyCode::Home));
        assert_eq!(app.cursor_line, 0);
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn ctrl_end_moves_to_doc_end() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        app.on_key(ctrl(KeyCode::End));
        assert_eq!(app.cursor_line, 0);
        assert_eq!(app.cursor_col, 5);
    }

    #[test]
    fn ctrl_shift_home_selects_to_doc_start() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        app.on_key(plain(KeyCode::End));
        app.on_key(ctrl_shift(KeyCode::Home));
        let sel = app.normalized_selection().unwrap();
        assert_eq!(sel, ((0, 0), (0, 5)));
    }

    #[test]
    fn ctrl_shift_end_selects_to_doc_end() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello");
        app.on_key(ctrl(KeyCode::Home));
        app.on_key(ctrl_shift(KeyCode::End));
        let sel = app.normalized_selection().unwrap();
        assert_eq!(sel, ((0, 0), (0, 5)));
    }

    #[test]
    fn ctrl_right_at_end_of_line_goes_to_next_line() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello\nworld");
        // Move to end of line 0 (typed leaves the cursor at doc end).
        app.on_key(plain(KeyCode::Up));
        app.on_key(plain(KeyCode::End));
        // Ctrl+Right should cross to start of "world" on line 1.
        app.on_key(ctrl(KeyCode::Right));
        assert_eq!(app.cursor_line, 1);
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn ctrl_left_at_start_of_line_goes_to_prev_line() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello\nworld");
        // Move to start of "world" (line 1, col 0).
        app.on_key(ctrl(KeyCode::Right));
        // Ctrl+Left should cross to end of "hello" on line 0.
        app.on_key(ctrl(KeyCode::Left));
        assert_eq!(app.cursor_line, 0);
        assert_eq!(app.cursor_col, 5);
    }

    #[test]
    fn backspace_deletes_the_active_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello world");
        app.on_key(plain(KeyCode::Home));
        for _ in 0..5 {
            app.on_key(shift(KeyCode::Right));
        }
        // Select "hello", then Backspace must delete the WHOLE selection.
        app.on_key(plain(KeyCode::Backspace));
        assert_eq!(app.doc().line(0), " world");
        assert_eq!(app.cursor_col, 0);
        assert!(app.normalized_selection().is_none());
        // One undo restores it (single step).
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line(0), "hello world");
    }

    #[test]
    fn delete_forward_deletes_the_active_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "hello world");
        app.on_key(plain(KeyCode::End));
        for _ in 0..5 {
            app.on_key(shift(KeyCode::Left));
        }
        // Select "world", then Delete must delete the WHOLE selection.
        app.on_key(plain(KeyCode::Delete));
        assert_eq!(app.doc().line(0), "hello ");
        assert_eq!(app.cursor_col, 6);
    }

    #[test]
    fn ctrl_shift_k_works_with_uppercase_k_delivery() {
        // Windows terminals deliver Ctrl+Shift+K as Char('K').
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "one\ntwo\nthree");
        app.on_key(plain(KeyCode::Up));
        app.on_key(plain(KeyCode::Home));
        app.on_key(press(
            KeyCode::Char('K'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        assert_eq!(app.doc().line_count(), 2);
        assert_eq!(app.doc().line(1), "three");
    }

    #[test]
    fn ctrl_shift_k_with_selection_deletes_all_selected_lines() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "one\ntwo\nthree\nfour");
        app.on_key(ctrl(KeyCode::Home));
        app.on_key(shift(KeyCode::Down));
        app.on_key(shift(KeyCode::Down));
        // Selection spans lines 0-2: delete ALL of them.
        app.on_key(press(
            KeyCode::Char('K'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        assert_eq!(app.doc().line_count(), 1);
        assert_eq!(app.doc().line(0), "four");
        assert!(app.normalized_selection().is_none());
        // One undo restores everything.
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line_count(), 4);
    }

    #[test]
    fn f8_deletes_the_current_line_as_fallback() {
        // Terminals/PTY bridges can strip the Ctrl+Shift modifiers from
        // Ctrl+Shift+K (it then arrives as a plain typed K); F8 is the
        // fallback that always works, like F2/F3/F5/F6 precedents.
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(
            &mut app,
            "one
two
three",
        );
        app.on_key(plain(KeyCode::Up));
        app.on_key(plain(KeyCode::Home));
        app.on_key(plain(KeyCode::F(8)));
        assert_eq!(app.doc().line_count(), 2);
        assert_eq!(app.doc().line(1), "three");
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line(1), "two");
    }

    // ============ T5: interactive open list ============

    #[test]
    fn compute_open_candidates_filters_and_skips_dirs() {
        let dir = temp_dir("open_candidates");
        std::fs::write(dir.join("beta.txt"), "b").expect("write");
        std::fs::write(dir.join("alpha.md"), "a").expect("write");
        std::fs::create_dir(dir.join("subdir")).expect("mkdir");
        let all = App::compute_open_candidates(&dir, "");
        assert_eq!(all.len(), 2, "directories are not candidates");
        let filtered = App::compute_open_candidates(&dir, "ALP");
        assert_eq!(filtered.len(), 1, "filter is case-insensitive");
        assert!(filtered[0].ends_with("alpha.md"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_prompt_filters_enter_opens_selected() {
        let dir = temp_dir("open_prompt_flow");
        std::fs::write(dir.join("beta.txt"), "beta-content").expect("write");
        std::fs::write(dir.join("gamma.txt"), "gamma-content").expect("write");
        let seed = dir.join("seed.txt");
        std::fs::write(&seed, "seed").expect("write");
        let mut app = App::new(vec![Document::new_with_path(&seed)], Lang::En);
        app.on_key(ctrl(KeyCode::Char('o')));
        assert!(app.prompt.is_some());
        // ".." (browser parent, empty filter) + all dir files listed.
        assert_eq!(app.open_candidates.len(), 4);
        // Typing filters the candidate list (and feeds the input field).
        typed(&mut app, "gam");
        assert_eq!(app.open_candidates.len(), 1);
        // Enter opens the highlighted candidate.
        app.on_key(plain(KeyCode::Enter));
        assert!(app.prompt.is_none());
        assert_eq!(app.docs.len(), 2);
        assert!(
            app.doc()
                .path()
                .map(|p| p.ends_with("gamma.txt"))
                .unwrap_or(false),
            "active doc must be gamma.txt"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_prompt_arrows_move_selection_with_clamp() {
        let dir = temp_dir("open_prompt_arrows");
        std::fs::write(dir.join("a.txt"), "a").expect("write");
        std::fs::write(dir.join("b.txt"), "b").expect("write");
        let seed = dir.join("seed.txt");
        std::fs::write(&seed, "seed").expect("write");
        let mut app = App::new(vec![Document::new_with_path(&seed)], Lang::En);
        app.on_key(ctrl(KeyCode::Char('o')));
        assert_eq!(app.open_selected, 0);
        app.on_key(plain(KeyCode::Down));
        assert_eq!(app.open_selected, 1);
        app.on_key(plain(KeyCode::Down));
        assert_eq!(
            app.open_selected, 2,
            "clamped at the last candidate (3 files)"
        );
        app.on_key(plain(KeyCode::Up));
        assert_eq!(app.open_selected, 1);
        app.on_key(plain(KeyCode::Up));
        assert_eq!(app.open_selected, 0);
        app.on_key(plain(KeyCode::Up));
        assert_eq!(app.open_selected, 0, "clamped at the first candidate");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_prompt_renders_candidate_list() {
        let dir = temp_dir("open_prompt_render");
        std::fs::write(dir.join("only.txt"), "x").expect("write");
        let seed = dir.join("seed.txt");
        std::fs::write(&seed, "seed").expect("write");
        let mut app = App::new(vec![Document::new_with_path(&seed)], Lang::En);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 10)).unwrap();
        // Prime the viewport before any key handling.
        terminal.draw(|f| app.draw(f)).unwrap();
        app.on_key(ctrl(KeyCode::Char('o')));
        terminal.draw(|f| app.draw(f)).unwrap();
        // 3 candidates (".." + 2 files) -> bordered popup, height 7
        // (3 entries + borders + min height), centered: width 48, x=16 on
        // 80 cols, y=1 on a 10-row terminal (editor area y=1). Row 1: top
        // border with hint; row 2: "> .."; row 3: only.txt; row 4: seed.txt;
        // row 7: bottom border.
        let mut top = String::new();
        let mut up_row = String::new();
        let mut sel_row = String::new();
        let mut second = String::new();
        for col in 0..64u16 {
            top.push_str(terminal.backend().buffer().get(col, 1).symbol());
            up_row.push_str(terminal.backend().buffer().get(col, 2).symbol());
            sel_row.push_str(terminal.backend().buffer().get(col, 3).symbol());
            second.push_str(terminal.backend().buffer().get(col, 4).symbol());
        }
        assert!(top.contains('┌'), "top border: {top}");
        assert!(top.contains("Home"), "hint title with nav keys: {top}");
        assert!(up_row.contains("> .."), "row 2: {up_row}");
        assert!(sel_row.contains("only.txt"), "row 3: {sel_row}");
        assert!(second.contains("seed.txt"), "row 4: {second}");
        // The right border closes the fitted width at col 63 (16 + 48 - 1).
        assert_eq!(terminal.backend().buffer().get(63, 3).symbol(), "│");
        let cell = terminal.backend().buffer().get(17, 2);
        assert_eq!(cell.style().bg, Some(Color::Rgb(0xE2, 0xC0, 0x89)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ============ Line ops + go-to-line ============

    #[test]
    fn ctrl_shift_k_deletes_the_current_line() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(
            &mut app,
            "one
two
three",
        );
        // Position on "two" (typed leaves the cursor at doc end).
        app.on_key(plain(KeyCode::Up));
        app.on_key(plain(KeyCode::Home));
        // Cursor on "two": delete the whole line.
        app.on_key(ctrl_shift(KeyCode::Char('k')));
        assert_eq!(app.doc().line_count(), 2);
        assert_eq!(app.doc().line(1), "three");
        // Single undo restores the line.
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line(1), "two");
    }

    #[test]
    fn ctrl_shift_k_on_last_line_joins_with_previous() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(
            &mut app, "one
two",
        );
        app.on_key(ctrl_shift(KeyCode::Char('k')));
        assert_eq!(app.doc().line_count(), 1);
        assert_eq!(app.doc().line(0), "one");
    }

    #[test]
    fn ctrl_g_moves_cursor_to_line_keeping_column() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(
            &mut app,
            "a
bc
cdef",
        );
        // Position at (0,1): doc start, then one right.
        app.on_key(ctrl(KeyCode::Home));
        app.on_key(plain(KeyCode::Right));
        assert_eq!((app.cursor_line, app.cursor_col), (0, 1));
        app.on_key(ctrl(KeyCode::Char('g')));
        typed(&mut app, "3");
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.cursor_line, 2);
        assert_eq!(app.cursor_col, 1, "column must be preserved (clamped)");
        assert!(app.prompt.is_none());
    }

    #[test]
    fn ctrl_g_clamps_beyond_document_and_invalid_keeps_prompt() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(
            &mut app, "a
b",
        );
        app.on_key(ctrl(KeyCode::Char('g')));
        typed(&mut app, "99");
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.cursor_line, 1, "out-of-range clamps to the last line");
        // Invalid input keeps the prompt open for correction.
        app.on_key(ctrl(KeyCode::Char('g')));
        typed(&mut app, "abc");
        app.on_key(plain(KeyCode::Enter));
        assert!(app.prompt.is_some(), "invalid number keeps the prompt open");
        app.on_key(plain(KeyCode::Esc));
        assert!(app.prompt.is_none());
    }

    #[test]
    fn alt_down_moves_line_and_alt_up_moves_back() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "alpha\nbeta\ngamma");
        // Go to line 1 ("beta").
        app.on_key(ctrl(KeyCode::Home));
        app.on_key(plain(KeyCode::Down));
        app.on_key(alt(KeyCode::Down));
        assert_eq!(app.doc().line(1), "gamma");
        assert_eq!(app.doc().line(2), "beta");
        assert_eq!(app.cursor_line, 2, "cursor travels with the line");
        // One undo restores the original order.
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line(1), "beta");
        app.on_key(ctrl(KeyCode::Char('y')));
        app.on_key(alt(KeyCode::Up));
        assert_eq!(app.doc().line(1), "beta");
        assert_eq!(app.doc().line(2), "gamma");
        app.on_key(alt(KeyCode::Down));
        // Boundary: Alt+Up on the first line is a no-op.
        app.on_key(ctrl(KeyCode::Home));
        app.on_key(alt(KeyCode::Up));
        assert_eq!(app.doc().line(0), "alpha");
    }

    #[test]
    fn alt_down_moves_selected_block_with_selection() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "alpha\nbeta\ngamma");
        // Select lines 0-1: doc start, Shift+Down, Shift+End.
        app.on_key(ctrl(KeyCode::Home));
        app.on_key(shift(KeyCode::Down));
        app.on_key(shift(KeyCode::End));
        assert_eq!(app.normalized_selection(), Some(((0, 0), (1, 4))));
        app.on_key(alt(KeyCode::Down));
        assert_eq!(app.doc().line(0), "gamma");
        assert_eq!(app.doc().line(1), "alpha");
        assert_eq!(app.doc().line(2), "beta");
        // The selection travels with the block.
        assert_eq!(app.normalized_selection(), Some(((1, 0), (2, 4))));
        // One undo restores the original order.
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line(0), "alpha");
    }

    #[test]
    fn scratch_alt_down_trace() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(
            &mut app,
            "alpha
beta
gamma",
        );
        app.on_key(ctrl(KeyCode::Home));
        app.on_key(plain(KeyCode::Down));
        app.on_key(alt(KeyCode::Down));
        println!(
            "AFTER  lines={:?}",
            (0..app.doc().line_count())
                .map(|i| app.doc().line(i).to_string())
                .collect::<Vec<_>>()
        );
        println!("AFTER  undo_stack={}", app.histories[0].undo.len());
        app.on_key(ctrl(KeyCode::Char('z')));
        println!(
            "UNDONE lines={:?}",
            (0..app.doc().line_count())
                .map(|i| app.doc().line(i).to_string())
                .collect::<Vec<_>>()
        );
        app.on_key(ctrl(KeyCode::Char('y')));
        println!(
            "REDONE lines={:?}",
            (0..app.doc().line_count())
                .map(|i| app.doc().line(i).to_string())
                .collect::<Vec<_>>()
        );
    }

    // ============ T4: clipboard ============

    #[test]
    fn build_selection_text_slices_by_chars_multiline() {
        // Char-based slicing: multi-byte chars and multi-line joins.
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "héllo\nwörld\n");
        app.on_key(plain(KeyCode::Home));
        app.on_key(shift(KeyCode::Down));
        app.on_key(shift(KeyCode::Right));
        app.on_key(shift(KeyCode::Right));
        app.on_key(shift(KeyCode::Right));
        // Selection (0,0)-(1,3) = "héllo\nwör".
        assert_eq!(app.build_selection_text(0, 0, 1, 3), "héllo\nwör");
    }

    #[test]
    fn clipboard_copy_paste_cut_roundtrip() {
        // The ONLY clipboard-touching test: cargo runs tests in parallel and
        // they would race on the real system clipboard, so every clipboard
        // interaction lives here.
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        typed(&mut app, "alpha beta\ngamma delta");
        // Select "beta" on line 0 (typed leaves the cursor at doc end).
        app.on_key(plain(KeyCode::Up));
        app.on_key(plain(KeyCode::Home));
        for _ in 0..6 {
            app.on_key(plain(KeyCode::Right));
        }
        for _ in 0..4 {
            app.on_key(shift(KeyCode::Right));
        }
        assert_eq!(app.normalized_selection(), Some(((0, 6), (0, 10))));
        // Copy it, jump to doc end, paste: line 1 gains "beta".
        app.on_key(ctrl(KeyCode::Char('c')));
        app.on_key(ctrl(KeyCode::End));
        app.on_key(ctrl(KeyCode::Char('v')));
        assert!(
            app.doc().line(1).ends_with("beta"),
            "line1: {}",
            app.doc().line(1)
        );
        // One undo removes the pasted text.
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(app.doc().line(1), "gamma delta");
        // Select "delta" and cut it (Ctrl+X).
        for _ in 0..5 {
            app.on_key(shift(KeyCode::Left));
        }
        assert_eq!(app.normalized_selection(), Some(((1, 6), (1, 11))));
        app.on_key(ctrl(KeyCode::Char('x')));
        assert_eq!(app.doc().line(1), "gamma ");
        // Shift+Insert pastes it back at the cursor.
        app.on_key(shift(KeyCode::Insert));
        assert_eq!(app.doc().line(1), "gamma delta");
        // Paste over an active selection deletes+inserts as ONE undo step.
        for _ in 0..5 {
            app.on_key(shift(KeyCode::Left));
        }
        app.on_key(ctrl(KeyCode::Char('v')));
        assert_eq!(app.doc().line(1), "gamma delta");
        // ONE undo restores the pre-paste state (grouped delete+insert):
        // with identical text the visible result is unchanged, which pins
        // the grouping (an ungrouped first undo would show "gamma ").
        app.on_key(ctrl(KeyCode::Char('z')));
        assert_eq!(
            app.doc().line(1),
            "gamma delta",
            "replace-paste undoes as one step"
        );
        // Shift+Delete with no selection cuts the current line.
        app.on_key(plain(KeyCode::Home));
        app.on_key(shift(KeyCode::Delete));
        assert_eq!(app.doc().line(1), "", "last-line cut clears the line");
    }

    // ---- Milestone 2 T3: command palette (Ctrl+Shift+P / F4) ----

    fn palette_labels(app: &App) -> Vec<String> {
        let palette = app.palette.as_ref().expect("palette must be open");
        palette
            .entries
            .iter()
            .map(|entry| match entry {
                PaletteEntry::Command(command) => i18n::t(app.lang, command.label_key).to_string(),
                PaletteEntry::Tab(index) => app.doc_title(*index),
                PaletteEntry::File(path) => path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            })
            .collect()
    }

    #[test]
    fn ctrl_shift_p_and_f4_open_the_palette() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl_shift(KeyCode::Char('p')));
        assert!(app.palette.is_some(), "Ctrl+Shift+P must open the palette");
        app.on_key(plain(KeyCode::Esc));
        assert!(app.palette.is_none(), "Esc must close the palette");
        app.on_key(plain(KeyCode::F(4)));
        assert!(app.palette.is_some(), "F4 must open the palette too");
    }

    #[test]
    fn empty_query_lists_every_registry_command() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl_shift(KeyCode::Char('p')));
        let labels = palette_labels(&app);
        assert_eq!(
            labels.len(),
            crate::command::all_commands().len(),
            "empty query must list the whole registry"
        );
        assert!(labels.iter().any(|l| l.contains("Save All")));
    }

    #[test]
    fn typing_filters_the_palette_fuzzily() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl_shift(KeyCode::Char('p')));
        for c in "sv".chars() {
            app.on_key(plain(KeyCode::Char(c)));
        }
        let labels = palette_labels(&app);
        assert_eq!(labels.len(), 2, "only the two Save commands match 'sv'");
        assert!(labels.iter().all(|l| l.contains("Save")));
        // Filter characters must not reach the buffer.
        assert_eq!(app.doc().line(0), "", "palette chars must not edit the doc");
    }

    #[test]
    fn enter_executes_the_highlighted_command() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl_shift(KeyCode::Char('p')));
        for c in "overw".chars() {
            app.on_key(plain(KeyCode::Char(c)));
        }
        let labels = palette_labels(&app);
        assert_eq!(labels.len(), 1, "only Toggle Overwrite matches 'overw'");
        assert!(
            labels[0].contains("Overwrite"),
            "best match first: {labels:?}"
        );
        app.on_key(plain(KeyCode::Enter));
        assert!(app.overwrite, "Enter must run the highlighted command");
        assert!(app.palette.is_none(), "palette closes after running");
        assert!(
            app.status.contains("Overwrite ON"),
            "status: {}",
            app.status
        );
    }

    #[test]
    fn esc_closes_the_palette_without_touching_the_buffer() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        for c in "hola".chars() {
            app.on_key(plain(KeyCode::Char(c)));
        }
        app.on_key(ctrl_shift(KeyCode::Char('p')));
        app.on_key(plain(KeyCode::Char('x')));
        app.on_key(plain(KeyCode::Esc));
        assert!(app.palette.is_none());
        assert_eq!(
            app.doc().line(0),
            "hola",
            "palette keys must not edit the doc"
        );
    }

    #[test]
    fn arrows_move_the_palette_highlight_and_clamp() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl_shift(KeyCode::Char('p')));
        let total = app.palette.as_ref().unwrap().entries.len();
        assert!(total > 2, "registry must have several commands");
        app.on_key(plain(KeyCode::Down));
        assert_eq!(app.palette.as_ref().unwrap().selected, 1);
        app.on_key(plain(KeyCode::Down));
        assert_eq!(app.palette.as_ref().unwrap().selected, 2);
        app.on_key(plain(KeyCode::Up));
        assert_eq!(app.palette.as_ref().unwrap().selected, 1);
        for _ in 0..5 {
            app.on_key(plain(KeyCode::Up));
        }
        assert_eq!(app.palette.as_ref().unwrap().selected, 0, "clamps at top");
    }

    #[test]
    fn palette_does_not_open_while_search_dialog_is_active() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        app.on_key(ctrl(KeyCode::Char('f')));
        assert!(app.search.is_some());
        app.on_key(ctrl_shift(KeyCode::Char('p')));
        assert!(app.palette.is_none(), "the search dialog keeps priority");
        assert!(app.search.is_some());
    }

    #[test]
    fn palette_renders_query_line_and_entries() {
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 10)).unwrap();
        // Prime the viewport before opening the palette (stale-viewport gotcha).
        terminal.draw(|f| app.draw(f)).unwrap();
        app.on_key(ctrl_shift(KeyCode::Char('p')));
        for c in "sv".chars() {
            app.on_key(plain(KeyCode::Char(c)));
        }
        terminal.draw(|f| app.draw(f)).unwrap();
        let mut frame = String::new();
        for y in 0..10u16 {
            for x in 0..80u16 {
                frame.push_str(terminal.backend().buffer().get(x, y).symbol());
            }
        }
        assert!(frame.contains("> sv"), "input row must echo the query");
        assert!(frame.contains("File: Save"), "entries must render");
    }

    // ---- Milestone 2 T4: quick open (Ctrl+P) ----

    #[test]
    fn ctrl_p_lists_open_tabs_first_then_unopened_dir_files() {
        let dir = temp_dir("quick_open_list");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        let c = dir.join("c.txt");
        std::fs::write(&a, "a").expect("write");
        std::fs::write(&b, "b").expect("write");
        std::fs::write(&c, "c").expect("write");
        // Two open tabs (a, b); c.txt exists only on disk.
        let mut app = App::new(
            vec![Document::new_with_path(&a), Document::new_with_path(&b)],
            Lang::En,
        );
        app.on_key(ctrl(KeyCode::Char('p')));
        let labels = palette_labels(&app);
        assert_eq!(labels.len(), 3, "2 tabs + 1 unopened file: {labels:?}");
        assert!(labels[0].contains("a.txt"), "tabs first: {labels:?}");
        assert!(labels[1].contains("b.txt"));
        assert!(
            labels[2].contains("c.txt"),
            "unopened file last: {labels:?}"
        );
    }

    #[test]
    fn quick_open_enter_on_file_opens_it_as_a_tab() {
        let dir = temp_dir("quick_open_file");
        let a = dir.join("a.txt");
        let c = dir.join("c.txt");
        std::fs::write(&a, "a").expect("write");
        std::fs::write(&c, "c").expect("write");
        let mut app = App::new(vec![Document::new_with_path(&a)], Lang::En);
        app.on_key(ctrl(KeyCode::Char('p')));
        for ch in "c.".chars() {
            app.on_key(plain(KeyCode::Char(ch)));
        }
        let before = app.docs.len();
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.docs.len(), before + 1, "file opens as a new tab");
        assert_eq!(app.active, before, "opened file focuses its tab");
        assert!(app.status.contains("Opened"), "status: {}", app.status);
        assert!(app.palette.is_none(), "palette closes after opening");
    }

    #[test]
    fn quick_open_enter_on_tab_switches_to_it() {
        let dir = temp_dir("quick_open_tab");
        let a = dir.join("alpha.md");
        let b = dir.join("beta.md");
        std::fs::write(&a, "a").expect("write");
        std::fs::write(&b, "b").expect("write");
        let mut app = App::new(
            vec![Document::new_with_path(&a), Document::new_with_path(&b)],
            Lang::En,
        );
        assert_eq!(app.active, 0);
        app.on_key(ctrl(KeyCode::Char('p')));
        for ch in "beta".chars() {
            app.on_key(plain(KeyCode::Char(ch)));
        }
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.active, 1, "Enter on a tab entry focuses that tab");
        assert!(app.palette.is_none());
    }

    #[test]
    fn quick_open_enter_with_no_candidates_opens_the_typed_path() {
        let dir = temp_dir("quick_open_path");
        let a = dir.join("only.md");
        std::fs::write(&a, "x").expect("write");
        let mut app = App::new(vec![Document::new_with_path(&a)], Lang::En);
        app.on_key(ctrl(KeyCode::Char('p')));
        for ch in "qq9".chars() {
            app.on_key(plain(KeyCode::Char(ch)));
        }
        assert!(
            app.palette.as_ref().unwrap().entries.is_empty(),
            "nothing matches 'qq9'"
        );
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.docs.len(), 2, "typed path opens as a new tab");
        let last = app
            .docs
            .last()
            .unwrap()
            .path()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(last.ends_with("qq9"), "new tab uses the typed path: {last}");
    }

    #[test]
    fn quick_open_survives_an_untitled_tab_and_tabs_address_real_docs() {
        // B1 regression: labels and sources must stay in lockstep when an
        // open tab has no path (the app's default startup state).
        let dir = temp_dir("quick_open_untitled");
        let a = dir.join("a.txt");
        std::fs::write(&a, "a").expect("write");
        let mut app = App::new(
            vec![Document::new_untitled(), Document::new_with_path(&a)],
            Lang::En,
        );
        app.on_key(ctrl(KeyCode::Char('p'))); // empty query ranks ALL labels
        assert!(app.palette.is_some());
        for entry in &app.palette.as_ref().unwrap().entries {
            if let PaletteEntry::Tab(i) = entry {
                assert!(*i < app.docs.len(), "Tab index must address a real doc");
            }
        }
    }

    #[test]
    fn palette_popup_fits_a_narrow_terminal_without_panicking() {
        // B2 regression: u16::clamp panics when min > max; the popup must
        // degrade gracefully below its preferred width (20 columns here).
        let mut app = App::new(vec![Document::new_untitled()], Lang::En);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 10)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        app.on_key(ctrl_shift(KeyCode::Char('p')));
        terminal.draw(|f| app.draw(f)).unwrap(); // must not panic at width 20
    }

    // ---- Ctrl+O browser: navigation keys + folder navigation ----

    #[test]
    fn open_list_home_end_page_keys_navigate() {
        let dir = temp_dir("open_nav_keys");
        for name in [
            "a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "f.txt", "g.txt",
        ] {
            std::fs::write(dir.join(name), "x").expect("write");
        }
        let seed = dir.join("a.txt");
        let mut app = App::new(vec![Document::new_with_path(&seed)], Lang::En);
        app.on_key(ctrl(KeyCode::Char('o')));
        // ".." (empty filter, parent exists) + the 7 files.
        let total = app.open_candidates.len();
        assert_eq!(total, 8, ".. + 7 files: {:?}", app.open_candidates);
        app.on_key(plain(KeyCode::End));
        assert_eq!(app.open_selected, total - 1, "End jumps to the last row");
        app.on_key(plain(KeyCode::Home));
        assert_eq!(app.open_selected, 0, "Home jumps to the first row");
        app.on_key(plain(KeyCode::PageDown));
        assert_eq!(
            app.open_selected, 7,
            "PgDn moves one visible page (clamped at end)"
        );
        app.on_key(plain(KeyCode::PageUp));
        assert_eq!(app.open_selected, 0);
        app.on_key(plain(KeyCode::PageUp));
        assert_eq!(app.open_selected, 0, "PgUp clamps at the top");
    }

    #[test]
    fn open_list_shows_parent_and_directories_and_enters_them() {
        let dir = temp_dir("open_nav_dirs");
        let sub = dir.join("sub");
        std::fs::create_dir(&sub).expect("mkdir");
        std::fs::write(dir.join("root.txt"), "x").expect("write");
        std::fs::write(sub.join("inner.txt"), "x").expect("write");
        let seed = dir.join("root.txt");
        let mut app = App::new(vec![Document::new_with_path(&seed)], Lang::En);
        app.on_key(ctrl(KeyCode::Char('o')));
        // ".." + dir "sub" (before files) + file "root.txt".
        assert_eq!(app.open_candidates.len(), 3, "{:?}", app.open_candidates);
        assert_eq!(app.open_candidates[0], PathBuf::from(".."));
        assert_eq!(app.open_candidates[1], sub, "dirs sort before files");
        // Enter on the directory enters it and lists its contents.
        app.on_key(plain(KeyCode::Down));
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.open_dir.as_deref(), Some(sub.as_path()));
        assert!(
            app.open_candidates.contains(&sub.join("inner.txt")),
            "{:?}",
            app.open_candidates
        );
        assert!(!app.open_candidates.contains(&dir.join("root.txt")));
        assert!(
            app.prompt.as_ref().unwrap().1.is_empty(),
            "navigation clears the filter"
        );
        // Enter on ".." (selected 0) climbs back up.
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.open_dir.as_deref(), Some(dir.as_path()));
        assert!(app.open_candidates.contains(&dir.join("root.txt")));
    }

    #[test]
    fn open_list_esc_resets_folder_navigation() {
        let dir = temp_dir("open_nav_esc");
        let sub = dir.join("sub");
        std::fs::create_dir(&sub).expect("mkdir");
        let seed = dir.join("seed.txt");
        std::fs::write(&seed, "x").expect("write");
        let mut app = App::new(vec![Document::new_with_path(&seed)], Lang::En);
        app.on_key(ctrl(KeyCode::Char('o')));
        app.on_key(plain(KeyCode::Down)); // on "sub"
        app.on_key(plain(KeyCode::Enter)); // enter sub
        assert!(app.open_dir.is_some());
        app.on_key(plain(KeyCode::Esc));
        assert!(app.prompt.is_none());
        assert!(app.open_dir.is_none(), "closing resets the browsing root");
    }

    #[test]
    fn open_list_enter_on_file_opens_from_browsed_dir() {
        let dir = temp_dir("open_nav_open");
        let sub = dir.join("sub");
        std::fs::create_dir(&sub).expect("mkdir");
        std::fs::write(sub.join("inner.txt"), "x").expect("write");
        let seed = dir.join("seed.txt");
        std::fs::write(&seed, "x").expect("write");
        let mut app = App::new(vec![Document::new_with_path(&seed)], Lang::En);
        app.on_key(ctrl(KeyCode::Char('o')));
        app.on_key(plain(KeyCode::Down)); // on "sub"
        app.on_key(plain(KeyCode::Enter)); // enter sub
        for ch in "inner".chars() {
            app.on_key(plain(KeyCode::Char(ch)));
        }
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.docs.len(), 2, "file inside the browsed dir opens");
        assert_eq!(app.active, 1);
        assert_eq!(
            app.docs[1].path().unwrap(),
            sub.join("inner.txt").as_path(),
            "opened from the browsed directory"
        );
        assert!(app.open_dir.is_none(), "opening resets navigation");
    }

    #[test]
    fn open_list_typed_path_still_wins_when_nothing_matches() {
        // ".." must not shadow the typed-path fallback: with a non-empty
        // filter that matches nothing, Enter opens the typed input.
        let dir = temp_dir("open_nav_typed");
        let seed = dir.join("seed.txt");
        std::fs::write(&seed, "x").expect("write");
        let mut app = App::new(vec![Document::new_with_path(&seed)], Lang::En);
        app.on_key(ctrl(KeyCode::Char('o')));
        for ch in "new_file.txt".chars() {
            app.on_key(plain(KeyCode::Char(ch)));
        }
        assert!(app.open_candidates.is_empty(), "no match for the new name");
        app.on_key(plain(KeyCode::Enter));
        assert_eq!(app.docs.len(), 2, "typed path opens as a new tab");
    }

    #[test]
    fn quick_open_palette_stays_files_only() {
        // The palette quick-open keeps tabs+files (VS Code style); folder
        // navigation belongs to the Ctrl+O browser.
        let dir = temp_dir("open_nav_palette");
        let sub = dir.join("sub");
        std::fs::create_dir(&sub).expect("mkdir");
        std::fs::write(dir.join("f.txt"), "x").expect("write");
        let seed = dir.join("seed.txt");
        std::fs::write(&seed, "x").expect("write");
        let mut app = App::new(vec![Document::new_with_path(&seed)], Lang::En);
        app.on_key(ctrl(KeyCode::Char('p')));
        for entry in &app.palette.as_ref().unwrap().entries {
            if let PaletteEntry::File(path) = entry {
                assert!(!path.is_dir(), "quick open must not list directories");
            }
        }
    }
}
