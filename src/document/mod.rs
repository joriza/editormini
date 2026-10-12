//! Line-based document model.
//!
//! Storage is a `Vec<String>` of lines WITHOUT terminators. A gap buffer or
//! rope may replace it later; the line-indexed API below is the stable surface
//! views and commands build on, so the swap stays internal.
//!
//! Milestone 1 encoding policy: UTF-8 only. A leading BOM is detected, stored,
//! and re-emitted on save. Line endings are detected at load (CRLF wins ties —
//! Windows-first), mixed files normalize to the dominant ending, and new files
//! default to CRLF.

use std::fmt;
use std::path::{Path, PathBuf};

/// Files above this size open read-only (protects future undo history and
/// keeps the editor responsive). Tested via [`Document::size_is_read_only`].
pub const READ_ONLY_THRESHOLD: u64 = 10 * 1024 * 1024;

/// Newline convention of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Crlf,
    Lf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Crlf => "\r\n",
            LineEnding::Lf => "\n",
        }
    }
}

/// Errors surfaced by document operations.
#[derive(Debug)]
pub enum DocumentError {
    Io(std::io::Error),
    NotUtf8,
    Untitled,
    ReadOnly,
    OutOfBounds { line: usize, col: usize },
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocumentError::Io(e) => write!(f, "I/O error: {e}"),
            DocumentError::NotUtf8 => write!(f, "file is not valid UTF-8"),
            DocumentError::Untitled => write!(f, "document has no file path yet"),
            DocumentError::ReadOnly => write!(f, "document is read-only"),
            DocumentError::OutOfBounds { line, col } => {
                write!(f, "position {line}:{col} is out of bounds")
            }
        }
    }
}

impl std::error::Error for DocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DocumentError::Io(e) => Some(e),
            _ => None,
        }
    }
}

/// Record of one applied edit, carrying everything needed to invert it.
/// Milestone task "bounded undo" pushes these onto a per-buffer stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditRecord {
    /// Text inserted at a position (may span lines via '\n').
    Insert {
        line: usize,
        col: usize,
        text: String,
    },
    /// Contiguous range deleted (may span lines); text is what was removed.
    Delete {
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
        text: String,
    },
}

impl EditRecord {
    /// The edit that undoes this one.
    pub fn inverse(&self) -> EditRecord {
        match self {
            EditRecord::Insert { line, col, text } => {
                let segments: Vec<&str> = text.split('\n').collect();
                if segments.len() == 1 {
                    // Single-line insert: the removed span ends on the SAME
                    // line at col + length.
                    EditRecord::Delete {
                        start_line: *line,
                        start_col: *col,
                        end_line: *line,
                        end_col: col + segments[0].chars().count(),
                        text: text.clone(),
                    }
                } else {
                    // Multi-line insert: the last segment starts at column 0
                    // of its new line, so it ends at its own length.
                    EditRecord::Delete {
                        start_line: *line,
                        start_col: *col,
                        end_line: line + segments.len() - 1,
                        end_col: segments.last().map(|s| s.chars().count()).unwrap_or(0),
                        text: text.clone(),
                    }
                }
            }
            EditRecord::Delete {
                start_line,
                start_col,
                text,
                ..
            } => EditRecord::Insert {
                line: *start_line,
                col: *start_col,
                text: text.clone(),
            },
        }
    }
}

/// A text document open in the editor.
#[derive(Debug, Clone)]
pub struct Document {
    lines: Vec<String>,
    path: Option<PathBuf>,
    line_ending: LineEnding,
    has_bom: bool,
    ends_with_newline: bool,
    modified: bool,
    read_only: bool,
}

impl Document {
    /// New empty in-memory document (no path). Windows-first: CRLF default.
    pub fn new_untitled() -> Self {
        Document {
            lines: vec![String::new()],
            path: None,
            line_ending: LineEnding::Crlf,
            has_bom: false,
            ends_with_newline: true,
            modified: false,
            read_only: false,
        }
    }

    /// New empty document bound to a path that does not exist yet.
    pub fn new_with_path(path: impl Into<PathBuf>) -> Self {
        let mut doc = Self::new_untitled();
        doc.path = Some(path.into());
        doc
    }

    /// Load an existing file. The size guard only flips `read_only`; loading
    /// always succeeds for readable files.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(DocumentError::Io)?;
        let read_only = Self::size_is_read_only(bytes.len() as u64);
        let has_bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
        let text = std::str::from_utf8(&bytes).map_err(|_| DocumentError::NotUtf8)?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);

        let crlf_count = text.matches("\r\n").count();
        let lf_total = text.matches('\n').count();
        let lf_count = lf_total - crlf_count;
        let line_ending = if crlf_count >= lf_count {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        };

        let ends_with_newline = text.ends_with('\n');
        let mut lines: Vec<String> = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
            .collect();
        if ends_with_newline {
            // split('\n') leaves an empty tail for the final terminator.
            lines.pop();
        }
        if lines.is_empty() {
            lines.push(String::new());
        }

        Ok(Document {
            lines,
            path: Some(path.to_path_buf()),
            line_ending,
            has_bom,
            ends_with_newline,
            modified: false,
            read_only,
        })
    }

    /// Pure size classifier so the guard is testable without a 10 MB fixture.
    pub fn size_is_read_only(bytes: u64) -> bool {
        bytes > READ_ONLY_THRESHOLD
    }

    /// Save to a new path, adopting it as the document's path.
    /// BOM and line-ending rules apply unchanged from [`save`].
    pub fn save_as(&mut self, path: impl Into<PathBuf>) -> Result<u64, DocumentError> {
        self.path = Some(path.into());
        self.save()
    }

    // -- Accessors ---------------------------------------------------------

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, line: usize) -> &str {
        &self.lines[line]
    }

    pub fn line_len(&self, line: usize) -> usize {
        self.lines[line].chars().count()
    }

    // Read by the theme milestone (syntax cache key) and status bar polish.
    #[allow(dead_code)]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    // Test helper: force the read-only flag so status-bar tests can
    // verify the read-only marker without creating a 10 MB fixture.
    #[cfg(test)]
    pub fn set_read_only(&mut self) {
        self.read_only = true;
    }

    // Read by the theme milestone (rehighlight on BOM-sensitive content).
    #[allow(dead_code)]
    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    #[allow(dead_code)]
    pub fn has_bom(&self) -> bool {
        self.has_bom
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    // Read by the tab-bar milestone to flag read-only tabs.
    #[allow(dead_code)]
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    // -- Editing -----------------------------------------------------------

    fn check_position(&self, line: usize, col: usize) -> Result<(), DocumentError> {
        if line >= self.lines.len() || col > self.line_len(line) {
            return Err(DocumentError::OutOfBounds { line, col });
        }
        Ok(())
    }

    fn check_range(
        &self,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> Result<(), DocumentError> {
        if start_line > end_line || end_line >= self.lines.len() {
            return Err(DocumentError::OutOfBounds {
                line: end_line,
                col: end_col,
            });
        }
        if start_col > self.line_len(start_line) || end_col > self.line_len(end_line) {
            return Err(DocumentError::OutOfBounds {
                line: start_line,
                col: start_col,
            });
        }
        if start_line == end_line && start_col > end_col {
            return Err(DocumentError::OutOfBounds {
                line: start_line,
                col: start_col,
            });
        }
        Ok(())
    }

    /// Insert text at `line:col`. '\n' inside `text` splits lines; "\r\n" and
    /// stray '\r' are normalized to '\n' (the stored [`LineEnding`] applies at
    /// save time only).
    pub fn insert(
        &mut self,
        line: usize,
        col: usize,
        text: &str,
    ) -> Result<EditRecord, DocumentError> {
        if self.read_only {
            return Err(DocumentError::ReadOnly);
        }
        self.check_position(line, col)?;
        let text = text.replace("\r\n", "\n").replace('\r', "");
        if text.is_empty() {
            return Ok(EditRecord::Insert {
                line,
                col,
                text: String::new(),
            });
        }

        let segments: Vec<&str> = text.split('\n').collect();
        let record = EditRecord::Insert {
            line,
            col,
            text: text.clone(),
        };

        if segments.len() == 1 {
            let col_byte = self.byte_col(line, col);
            self.lines[line].insert_str(col_byte, &text);
        } else {
            let col_byte = self.byte_col(line, col);
            let tail = self.lines[line].split_off(col_byte);
            let head = std::mem::take(&mut self.lines[line]);
            let mut inserted: Vec<String> = segments.iter().map(|s| (*s).to_string()).collect();
            inserted[0].insert_str(0, &head);
            let last = inserted.len() - 1;
            inserted[last].push_str(&tail);
            self.lines.splice(line..=line, inserted.drain(..));
        }
        self.modified = true;
        Ok(record)
    }

    /// Delete the contiguous range from `start_line:start_col` to
    /// `end_line:end_col` (exclusive end). Spanning ranges join lines.
    pub fn delete_range(
        &mut self,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> Result<EditRecord, DocumentError> {
        if self.read_only {
            return Err(DocumentError::ReadOnly);
        }
        self.check_range(start_line, start_col, end_line, end_col)?;

        let deleted = if start_line == end_line {
            let start_byte = self.byte_col(start_line, start_col);
            let end_byte = self.byte_col(end_line, end_col);
            let text = self.lines[start_line][start_byte..end_byte].to_string();
            self.lines[start_line].replace_range(start_byte..end_byte, "");
            text
        } else {
            let start_byte = self.byte_col(start_line, start_col);
            let end_byte = self.byte_col(end_line, end_col);
            let mut parts: Vec<String> = Vec::new();
            parts.push(self.lines[start_line][start_byte..].to_string());
            for mid in &self.lines[start_line + 1..end_line] {
                parts.push(mid.clone());
            }
            parts.push(self.lines[end_line][..end_byte].to_string());
            let text = parts.join("\n");
            let head = self.lines[start_line][..start_byte].to_string();
            let tail = self.lines[end_line][end_byte..].to_string();
            self.lines[start_line] = format!("{head}{tail}");
            self.lines.drain(start_line + 1..=end_line);
            text
        };

        self.modified = true;
        Ok(EditRecord::Delete {
            start_line,
            start_col,
            end_line,
            end_col,
            text: deleted,
        })
    }

    /// Apply a record (typically `record.inverse()` from the undo stack) and
    /// return the record of what was applied.
    pub fn apply(&mut self, record: &EditRecord) -> Result<EditRecord, DocumentError> {
        match record {
            EditRecord::Insert { line, col, text } => self.insert(*line, *col, text),
            EditRecord::Delete {
                start_line,
                start_col,
                end_line,
                end_col,
                ..
            } => self.delete_range(*start_line, *start_col, *end_line, *end_col),
        }
    }

    /// Char index -> byte index within a line (cols are char-based).
    fn byte_col(&self, line: usize, col: usize) -> usize {
        self.lines[line]
            .char_indices()
            .nth(col)
            .map(|(b, _)| b)
            .unwrap_or(self.lines[line].len())
    }

    // -- Persistence ---------------------------------------------------------

    /// Serialize and write to the bound path. Allowed on read-only documents
    /// (writes the unchanged content back); the guard protects edits, not saves.
    pub fn save(&mut self) -> Result<u64, DocumentError> {
        let Some(path) = &self.path else {
            return Err(DocumentError::Untitled);
        };
        let mut out = String::with_capacity(self.byte_len_hint());
        if self.has_bom {
            out.push('\u{feff}');
        }
        out.push_str(&self.lines.join(self.line_ending.as_str()));
        if self.ends_with_newline {
            out.push_str(self.line_ending.as_str());
        }
        let written = out.len() as u64;
        std::fs::write(path, out).map_err(DocumentError::Io)?;
        self.modified = false;
        Ok(written)
    }

    fn byte_len_hint(&self) -> usize {
        let chars: usize = self.lines.iter().map(|l| l.len() + 1).sum();
        chars
            + if self.line_ending == LineEnding::Crlf {
                self.lines.len()
            } else {
                0
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("editormini_{}_{}", name, std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn new_untitled_defaults() {
        let doc = Document::new_untitled();
        assert_eq!(doc.line_count(), 1);
        assert_eq!(doc.line(0), "");
        assert_eq!(doc.line_ending(), LineEnding::Crlf);
        assert!(!doc.has_bom());
        assert!(!doc.is_modified());
        assert!(!doc.is_read_only());
        assert!(doc.path().is_none());
    }

    #[test]
    fn bom_is_detected_and_preserved_on_save() {
        let dir = temp_dir("bom");
        let path = dir.join("bom.txt");
        std::fs::write(&path, "\u{feff}hello\r\nworld\r\n").expect("write fixture");
        let mut doc = Document::from_path(&path).expect("load");
        assert!(doc.has_bom());
        assert_eq!(doc.line(0), "hello");
        let written = doc.save().expect("save");
        let bytes = std::fs::read(&path).expect("reread");
        assert_eq!(written, bytes.len() as u64);
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn crlf_file_round_trips_as_crlf() {
        let dir = temp_dir("crlf");
        let path = dir.join("crlf.txt");
        std::fs::write(&path, "a\r\nb\r\n").expect("write fixture");
        let mut doc = Document::from_path(&path).expect("load");
        assert_eq!(doc.line_ending(), LineEnding::Crlf);
        assert!(doc.ends_with_newline);
        doc.insert(0, 1, "X").expect("insert");
        doc.save().expect("save");
        let raw = std::fs::read_to_string(&path).expect("reread");
        assert_eq!(raw, "aX\r\nb\r\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lf_file_stays_lf() {
        let dir = temp_dir("lf");
        let path = dir.join("lf.txt");
        std::fs::write(&path, "a\nb\n").expect("write fixture");
        let mut doc = Document::from_path(&path).expect("load");
        assert_eq!(doc.line_ending(), LineEnding::Lf);
        doc.insert(1, 1, "!").expect("insert");
        doc.save().expect("save");
        let raw = std::fs::read_to_string(&path).expect("reread");
        assert_eq!(raw, "a\nb!\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mixed_line_endings_normalize_to_dominant() {
        let dir = temp_dir("mixed");
        let path = dir.join("mixed.txt");
        // 2 CRLF vs 1 LF: dominant CRLF.
        std::fs::write(&path, "a\r\nb\r\nc\n").expect("write fixture");
        let mut doc = Document::from_path(&path).expect("load");
        assert_eq!(doc.line_ending(), LineEnding::Crlf);
        doc.save().expect("save");
        let raw = std::fs::read_to_string(&path).expect("reread");
        assert_eq!(raw, "a\r\nb\r\nc\r\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_only_blocks_edits_but_document_loads() {
        let mut doc = Document::new_untitled();
        doc.read_only = true;
        assert!(matches!(
            doc.insert(0, 0, "x"),
            Err(DocumentError::ReadOnly)
        ));
        assert!(matches!(
            doc.delete_range(0, 0, 0, 0),
            Err(DocumentError::ReadOnly)
        ));
        assert!(!doc.is_modified());
    }

    #[test]
    fn size_classifier_uses_threshold() {
        assert!(!Document::size_is_read_only(READ_ONLY_THRESHOLD));
        assert!(Document::size_is_read_only(READ_ONLY_THRESHOLD + 1));
    }

    #[test]
    fn insert_with_newlines_splits_lines() {
        let mut doc = Document::new_untitled();
        doc.insert(0, 0, "one").expect("seed");
        doc.insert(0, 3, "\ntwo\nthree").expect("insert");
        assert_eq!(doc.line_count(), 3);
        assert_eq!(doc.line(0), "one");
        assert_eq!(doc.line(1), "two");
        assert_eq!(doc.line(2), "three");
        assert!(doc.is_modified());
    }

    #[test]
    fn delete_range_across_lines_joins() {
        let mut doc = Document::new_untitled();
        doc.insert(0, 0, "alpha\nbeta\ngamma").expect("seed");
        // Delete from end of "alpha" through start of "gamma" -> join.
        doc.delete_range(0, 5, 2, 0).expect("delete");
        assert_eq!(doc.line_count(), 1);
        assert_eq!(doc.line(0), "alphagamma");
    }

    #[test]
    fn insert_inverse_reproduces_original() {
        let mut doc = Document::new_untitled();
        doc.insert(0, 0, "hello\nworld").expect("seed");
        let before = doc.line_count();

        let record = doc.insert(1, 2, "XX\nYY").expect("edit");
        assert!(doc.line_count() > before);
        doc.apply(&record.inverse()).expect("undo");
        assert_eq!(doc.line_count(), before);
        assert_eq!(doc.line(0), "hello");
        assert_eq!(doc.line(1), "world");
    }

    #[test]
    fn delete_inverse_reinserts_removed_text() {
        let mut doc = Document::new_untitled();
        doc.insert(0, 0, "hello\nworld").expect("seed");
        let record = doc.delete_range(0, 3, 1, 2).expect("delete");
        // "hello" loses "lo", "world" loses "wo" -> "hel" + "rld".
        assert_eq!(doc.line(0), "helrld");
        doc.apply(&record.inverse()).expect("undo");
        assert_eq!(doc.line_count(), 2);
        assert_eq!(doc.line(0), "hello");
        assert_eq!(doc.line(1), "world");
    }

    #[test]
    fn save_round_trip_with_multibyte_chars() {
        let dir = temp_dir("utf8");
        let path = dir.join("utf8.txt");
        let mut doc = Document::new_with_path(&path);
        doc.insert(0, 0, "ñandú 🦤 español\nsegunda línea")
            .expect("seed");
        doc.save().expect("save");
        let doc2 = Document::from_path(&path).expect("reload");
        assert_eq!(doc2.line(0), "ñandú 🦤 español");
        assert_eq!(doc2.line(1), "segunda línea");
        assert_eq!(doc2.line_ending(), LineEnding::Crlf);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn out_of_bounds_positions_error() {
        let mut doc = Document::new_untitled();
        doc.insert(0, 0, "abc").expect("seed");
        assert!(matches!(
            doc.insert(5, 0, "x"),
            Err(DocumentError::OutOfBounds { line: 5, col: 0 })
        ));
        assert!(matches!(
            doc.insert(0, 99, "x"),
            Err(DocumentError::OutOfBounds { line: 0, col: 99 })
        ));
    }

    #[test]
    fn single_line_insert_inverse_reproduces_original() {
        // Regression: the inverse of a single-line insert used to end at the
        // segment LENGTH instead of col + length, so undo deleted nothing.
        let mut doc = Document::new_untitled();
        doc.insert(0, 0, "ac").expect("seed");
        let record = doc.insert(0, 1, "b").expect("insert");
        assert_eq!(doc.line(0), "abc");
        doc.apply(&record.inverse()).expect("undo");
        assert_eq!(doc.line(0), "ac");
    }

    #[test]
    fn empty_file_loads_as_one_empty_line() {
        let dir = temp_dir("empty");
        let path = dir.join("empty.txt");
        std::fs::write(&path, "").expect("write fixture");
        let doc = Document::from_path(&path).expect("load");
        assert_eq!(doc.line_count(), 1);
        assert_eq!(doc.line(0), "");
        assert!(!doc.ends_with_newline);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_without_path_is_untitled_error() {
        let mut doc = Document::new_untitled();
        assert!(matches!(doc.save(), Err(DocumentError::Untitled)));
    }

    #[test]
    fn invalid_utf8_reports_not_utf8() {
        let dir = temp_dir("latin1");
        let path = dir.join("latin1.txt");
        std::fs::write(&path, [0xF1]).expect("write invalid utf8 byte");
        assert!(matches!(
            Document::from_path(&path),
            Err(DocumentError::NotUtf8)
        ));
        let _ = std::fs::remove_file(&path);
    }
}
