//! Pane layout tree.
//!
//! The root of the UI is a tree of panes from day one, so the future
//! split-screen milestone (split, mouse-resizable divider, synchronized scroll
//! to compare two files) extends this tree instead of refactoring the shell.
//! Milestone 1 only ever constructs `PaneTree::Single`.

/// One pane showing one document (index into the app's document list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub doc: usize,
}

/// Binary split tree over panes. `ratio` is the first child's share of the
/// split axis, in 0..=1000 per-mille; `vertical` means the divider is a
/// vertical line (side-by-side panes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneTree {
    Single(Pane),
    /// Constructed by the split-screen milestone; documented shape until then.
    #[allow(dead_code)]
    Split {
        first: Box<PaneTree>,
        second: Box<PaneTree>,
        ratio: u16,
        vertical: bool,
    },
}

impl PaneTree {
    pub fn single(doc: usize) -> Self {
        PaneTree::Single(Pane { doc })
    }

    /// Split this tree, demoting the current subtree to the first child.
    /// Wired by the split-screen milestone; kept tested until then.
    #[allow(dead_code)]
    pub fn split(self, second: PaneTree, ratio: u16, vertical: bool) -> Self {
        PaneTree::Split {
            first: Box::new(self),
            second: Box::new(second),
            ratio,
            vertical,
        }
    }

    /// Document indices in draw order (left-to-right / top-to-bottom).
    /// Wired by the split-screen milestone; kept tested until then.
    #[allow(dead_code)]
    pub fn doc_indices(&self) -> Vec<usize> {
        match self {
            PaneTree::Single(pane) => vec![pane.doc],
            PaneTree::Split { first, second, .. } => {
                let mut docs = first.doc_indices();
                docs.extend(second.doc_indices());
                docs
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_pane_holds_one_document() {
        let tree = PaneTree::single(0);
        assert_eq!(tree.doc_indices(), vec![0]);
    }

    #[test]
    fn split_extends_the_tree_without_refactor() {
        let tree = PaneTree::single(0).split(PaneTree::single(1), 500, true);
        assert_eq!(tree.doc_indices(), vec![0, 1]);
        let tree = tree.split(PaneTree::single(2), 300, false);
        assert_eq!(tree.doc_indices(), vec![0, 1, 2]);
    }
}
