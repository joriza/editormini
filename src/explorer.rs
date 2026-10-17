//! Side file explorer model (milestone 3).
//!
//! A lazy tree of the active document's directory: `INITIAL_DEPTH` levels
//! are expanded on construction; deeper levels are loaded on demand when
//! the user expands a node. The UI works on the flat `rows` list, which is
//! rebuilt after every tree mutation.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Levels expanded when the tree is first built (owner decision: 3).
pub const INITIAL_DEPTH: usize = 3;

/// One visible row in the explorer panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerRow {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    /// 1-based depth: the root's children are depth 1.
    pub depth: usize,
    pub expanded: bool,
    /// Directory exists but has no readable entries.
    pub empty: bool,
}

/// Explorer state: root + flat visible rows + selection.
#[derive(Debug)]
pub struct ExplorerState {
    root: PathBuf,
    pub rows: Vec<ExplorerRow>,
    pub selected: usize,
    expanded: HashSet<PathBuf>,
    empty: HashSet<PathBuf>,
}

impl ExplorerState {
    /// Open the explorer at `root`, expanding the first `INITIAL_DEPTH`
    /// levels. A missing/unreadable root yields an empty row list.
    pub fn open_at(root: PathBuf) -> Self {
        let mut state = Self {
            root,
            rows: Vec::new(),
            selected: 0,
            expanded: HashSet::new(),
            empty: HashSet::new(),
        };
        state.rebuild(true);
        // Pre-populate expanded set with directories at depth < INITIAL_DEPTH
        // so they survive subsequent rebuilds (e.g., after user toggles another dir).
        state.expanded = state
            .rows
            .iter()
            .filter(|r| r.is_dir && r.depth < INITIAL_DEPTH)
            .map(|r| r.path.clone())
            .collect();
        state
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Render-facing accessors for the panel draw (milestone 3 T4/T5);
    /// unused until the renderer consumes them.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    #[allow(dead_code)]
    pub fn row(&self, index: usize) -> Option<&ExplorerRow> {
        self.rows.get(index)
    }

    pub fn selected_row(&self) -> Option<&ExplorerRow> {
        self.rows.get(self.selected)
    }

    /// Clamp and move the selection by `delta` (negative = up).
    pub fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            self.selected = 0;
            return;
        }
        if delta >= 0 {
            self.selected = (self.selected + delta as usize)
                .min(self.rows.len() - 1);
        } else {
            self.selected = self.selected.saturating_sub(delta.unsigned_abs());
        }
    }

    /// Jump the selection to `index` (mouse row clicks in T6).
    #[allow(dead_code)]
    pub fn select(&mut self, index: usize) {
        if !self.rows.is_empty() {
            self.selected = index.min(self.rows.len() - 1);
        }
    }

    /// Expand/collapse the selected directory. No-op on file rows.
    pub fn toggle_selected(&mut self) {
        let Some(row) = self.selected_row().cloned() else {
            return;
        };
        if !row.is_dir || row.empty {
            return;
        }
        if self.expanded.contains(&row.path) {
            self.expanded.remove(&row.path);
        } else {
            self.expanded.insert(row.path.clone());
        }
        self.rebuild(false);
    }

    /// `←` behavior: collapse the selected dir, or the nearest expanded
    /// ancestor of the selected row.
    pub fn collapse_or_up(&mut self) {
        let Some(row) = self.selected_row().cloned() else {
            return;
        };
        if row.is_dir && !row.empty {
            self.toggle_selected();
            return;
        }
        // Walk backwards for the closest expanded directory strictly above
        // the current depth.
        let above = self
            .rows
            .iter()
            .take(self.selected).rfind(|r| r.is_dir && r.depth < row.depth && r.expanded);
        if let Some(parent) = above {
            self.expanded.remove(&parent.path);
            self.rebuild(false);
        }
    }

    /// Replace the root (tab switched to another pathed doc).
    pub fn set_root(&mut self, root: PathBuf) {
        self.root = root;
        self.expanded.clear();
        self.empty.clear();
        self.selected = 0;
        self.rebuild(true);
    }

    fn rebuild(&mut self, initial: bool) {
        self.rows.clear();
        let root = self.root.clone();
        let mut rows: Vec<ExplorerRow> = Vec::new();
        Self::walk(&root, 1, initial, &mut self.expanded, &mut self.empty, &mut rows);
        self.rows = rows;
        if self.rows.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.rows.len() - 1);
        }
    }

    fn walk(
        dir: &Path,
        depth: usize,
        initial: bool,
        expanded: &mut HashSet<PathBuf>,
        empty: &mut HashSet<PathBuf>,
        rows: &mut Vec<ExplorerRow>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            empty.insert(dir.to_path_buf());
            return;
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        // Directories before files, alphabetical within each group.
        paths.sort_by(|a, b| {
            let (a_dir, b_dir) = (a.is_dir(), b.is_dir());
            b_dir.cmp(&a_dir).then_with(|| {
                a.file_name().as_ref().cmp(&b.file_name().as_ref())
            })
        });
        if paths.is_empty() {
            empty.insert(dir.to_path_buf());
        }
        for path in paths {
            let is_dir = path.is_dir();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let expanded_flag = if is_dir {
                if initial {
                    depth < INITIAL_DEPTH
                } else {
                    expanded.contains(&path)
                }
            } else {
                false
            };
            // Track empty dirs (not files — files can't be "empty").
            if !is_dir || expanded_flag {
                // Don't mark as empty unless we know it has no children.
            }
            rows.push(ExplorerRow {
                path: path.clone(),
                name,
                is_dir,
                depth,
                expanded: expanded_flag,
                empty: false,
            });
            if is_dir && expanded_flag {
                Self::walk(&path, depth + 1, initial, expanded, empty, rows);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal fixture: root has two dirs (alpha, bravo) and one file (a.txt).
    /// alpha contains beta/ and foxtrot.txt; beta contains charlie/.
    /// charlie contains delta/ which contains echo.txt.
    /// bravo contains golf.txt.
    fn fixture(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "editormini_explorer_fixture_{suffix}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        // Create the full tree so all parents exist.
        std::fs::create_dir_all(dir.join("alpha/beta/charlie/delta")).unwrap();
        std::fs::create_dir_all(dir.join("bravo")).unwrap();
        for path in [
            "a.txt",
            "alpha/foxtrot.txt",
            "alpha/beta/golf.txt",
            "alpha/beta/charlie/delta/echo.txt",
            "bravo/golf.txt",
        ] {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x").unwrap();
        }
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn initial_tree_expands_three_levels() {
        let dir = fixture("levels");
        let state = ExplorerState::open_at(dir.clone());
        // Walk produces DFS order: dirs first (sorted), then files (sorted).
        // Root children (depth 1): alpha/, bravo/, a.txt
        //   alpha expanded (depth 1 < 3)
        //   Root children (depth 2): beta/, foxtrot.txt
        //     beta expanded (depth 2 < 3)
        //     Root children (depth 3): charlie/, golf.txt
        //       charlie NOT expanded (depth 3 == INITIAL_DEPTH, so depth 3 is NOT < 3)
        //       delta (depth 4) is hidden because charlie is not expanded.
        // bravo expanded (depth 1 < 3)
        //   Root children (depth 2): golf.txt
        // a.txt (file, depth 1)
        let names: Vec<String> = state.rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(
            names,
            vec![
                "alpha", "beta", "charlie", "golf.txt", "foxtrot.txt",
                "bravo", "golf.txt", "a.txt",
            ],
            "unexpected rows: {names:?}"
        );
        assert_eq!(state.rows[0].depth, 1);
        // charlie is at index 2, depth 3, NOT expanded.
        assert!(state.rows[2].is_dir && !state.rows[2].expanded);
        cleanup(&dir);
    }

    #[test]
    fn collapse_and_expand_roundtrip() {
        let dir = fixture("collapse");
        let mut state = ExplorerState::open_at(dir.clone());
        // Verify 'beta' is visible initially.
        assert!(state.rows.iter().any(|r| r.name == "beta"));
        // 'alpha' is at index 0. Collapse it.
        state.select(0);
        state.toggle_selected();
        // After collapsing alpha, its children (beta, foxtrot.txt) must be hidden.
        let names_after_collapse: Vec<String> = state.rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(
            names_after_collapse,
            vec!["alpha", "bravo", "golf.txt", "a.txt"],
            "collapsed alpha leaves only top-level items + expanded siblings: {names_after_collapse:?}"
        );
        // Expand again.
        state.select(0);
        state.toggle_selected();
        assert!(
            state.rows.iter().any(|r| r.name == "beta"),
            "re-expanded alpha must restore children"
        );
        cleanup(&dir);
    }

    #[test]
    fn expand_beyond_initial_depth_on_demand() {
        let dir = fixture("expand");
        let mut state = ExplorerState::open_at(dir.clone());
        // 'charlie' is at index 2, depth 3, NOT expanded initially.
        let charlie_idx = state
            .rows
            .iter()
            .position(|r| r.name == "charlie")
            .expect("charlie must be visible at depth 3");
        // 'delta' must NOT be visible initially.
        assert!(!state.rows.iter().any(|r| r.name == "delta"), "delta hidden initially");
        // Expand charlie.
        state.select(charlie_idx);
        state.toggle_selected();
        // Now 'delta' (child of charlie at depth 4) must appear right after charlie.
        let names_after: Vec<String> = state.rows.iter().map(|r| r.name.clone()).collect();
        assert!(
            names_after.iter().any(|n| n == "delta"),
            "expanding charlie must reveal delta: {names_after:?}"
        );
        cleanup(&dir);
    }
    #[test]
    fn collapse_or_up_goes_to_parent_dir() {
        let dir = fixture("parent");
        let mut state = ExplorerState::open_at(dir.clone());
        // Select golf.txt inside alpha/beta (depth 3).
        let golf_idx = state
            .rows
            .iter()
            .position(|r| r.name == "golf.txt" && r.depth == 3)
            .expect("golf.txt at depth 3");
        state.select(golf_idx);
        state.collapse_or_up();
        // beta should be collapsed, hiding golf.txt at depth 3.
        // bravo/golf.txt at depth 2 should still be visible.
        assert!(
            !state
                .rows
                .iter()
                .any(|r| r.name == "golf.txt" && r.depth == 3),
            "golf.txt at depth 3 must be hidden when beta collapses: {:?}",
            state.rows.iter().map(|r| format!("{}(d{})", r.name, r.depth)).collect::<Vec<_>>()
        );
        // bravo/golf.txt at depth 2 should still be visible.
        assert!(
            state.rows.iter().any(|r| r.name == "golf.txt" && r.depth == 2),
            "bravo/golf.txt at depth 2 should still be visible"
        );
        cleanup(&dir);
    }

    #[test]
    fn missing_root_yields_empty_rows() {
        let dir = std::env::temp_dir().join(format!(
            "editormini_explorer_missing_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let state = ExplorerState::open_at(dir.clone());
        assert!(state.is_empty(), "unreadable root must yield empty rows");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn selection_clamps_after_collapse() {
        let dir = fixture("clamp");
        let mut state = ExplorerState::open_at(dir.clone());
        state.select(state.len() - 1); // last row
        state.select(0); // alpha
        state.toggle_selected(); // collapse alpha
        assert!(state.selected < state.len(), "selection clamped after collapse");
        state.move_selection(999);
        assert_eq!(state.selected, state.len() - 1);
        state.move_selection(-999);
        assert_eq!(state.selected, 0);
        cleanup(&dir);
    }

    #[test]
    fn set_root_resets_state() {
        let dir = fixture("reroot");
        // Start with alpha as root.
        let mut state = ExplorerState::open_at(dir.join("alpha").clone());
        assert!(
            state.rows.iter().any(|r| r.name == "foxtrot.txt"),
            "alpha root must show foxtrot.txt"
        );
        // Switch root to the top-level dir.
        state.set_root(dir.clone());
        assert!(
            state.rows.iter().any(|r| r.name == "a.txt"),
            "reroot must show a.txt"
        );
        assert_eq!(state.selected, 0);
        cleanup(&dir);
    }
}
