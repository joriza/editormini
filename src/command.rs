//! Command registry backing the command palette (milestone 2).
//!
//! One entry per user-actionable command, each labelled through i18n so the
//! palette speaks the UI language. Milestone 4 turns the keymap into a
//! remappable table over this registry.

use crate::i18n::Key;

/// Identifier of a palette-executable command. Every variant maps to an
/// existing `App` action — the palette exposes nothing new.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommandKind {
    Save,
    SaveAll,
    Open,
    CloseTab,
    Quit,
    Undo,
    Redo,
    Find,
    Replace,
    FindAllDocs,
    ReplaceAllDocs,
    DeleteLine,
    MoveLineUp,
    MoveLineDown,
    ToggleOverwrite,
    GotoLine,
    NextTab,
    PrevTab,
    LastDocPanel1,
}

/// A palette entry: what runs and how it is labelled.
#[derive(Debug, Clone, Copy)]
pub struct Command {
    pub kind: CommandKind,
    pub label_key: Key,
}

/// All palette commands in stable display order (File, Edit, Go, View).
pub fn all_commands() -> Vec<Command> {
    use CommandKind::*;
    // NOTE: `Key` is deliberately NOT glob-imported: `CommandKind::GotoLine`
    // and `Key::GotoLine` would collide into an ambiguous name.
    vec![
        Command {
            kind: Save,
            label_key: Key::CmdSave,
        },
        Command {
            kind: SaveAll,
            label_key: Key::CmdSaveAll,
        },
        Command {
            kind: Open,
            label_key: Key::CmdOpen,
        },
        Command {
            kind: CloseTab,
            label_key: Key::CmdCloseTab,
        },
        Command {
            kind: Quit,
            label_key: Key::CmdQuit,
        },
        Command {
            kind: Undo,
            label_key: Key::CmdUndo,
        },
        Command {
            kind: Redo,
            label_key: Key::CmdRedo,
        },
        Command {
            kind: Find,
            label_key: Key::CmdFind,
        },
        Command {
            kind: Replace,
            label_key: Key::CmdReplace,
        },
        Command {
            kind: FindAllDocs,
            label_key: Key::CmdFindAllDocs,
        },
        Command {
            kind: ReplaceAllDocs,
            label_key: Key::CmdReplaceAllDocs,
        },
        Command {
            kind: DeleteLine,
            label_key: Key::CmdDeleteLine,
        },
        Command {
            kind: MoveLineUp,
            label_key: Key::CmdMoveLineUp,
        },
        Command {
            kind: MoveLineDown,
            label_key: Key::CmdMoveLineDown,
        },
        Command {
            kind: ToggleOverwrite,
            label_key: Key::CmdToggleOverwrite,
        },
        Command {
            kind: GotoLine,
            label_key: Key::CmdGotoLine,
        },
        Command {
            kind: NextTab,
            label_key: Key::CmdNextTab,
        },
        Command {
            kind: PrevTab,
            label_key: Key::CmdPrevTab,
        },
        Command {
            kind: LastDocPanel1,
            label_key: Key::CmdLastDocPanel1,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    #[test]
    fn registry_is_non_empty_and_kinds_are_unique() {
        let commands = all_commands();
        assert!(!commands.is_empty(), "registry must list commands");
        let mut kinds: Vec<CommandKind> = commands.iter().map(|c| c.kind).collect();
        let total = kinds.len();
        kinds.sort();
        kinds.dedup();
        assert_eq!(kinds.len(), total, "command kinds must be unique");
    }

    #[test]
    fn every_command_label_resolves_in_both_languages() {
        for command in all_commands() {
            for lang in [Lang::En, Lang::Es] {
                let label = crate::i18n::t(lang, command.label_key);
                assert!(
                    !label.is_empty(),
                    "empty label for {lang:?}/{:?}",
                    command.kind
                );
            }
        }
    }

    #[test]
    fn registry_covers_the_core_file_and_edit_actions() {
        let kinds: Vec<CommandKind> = all_commands().iter().map(|c| c.kind).collect();
        for expected in [
            CommandKind::Save,
            CommandKind::SaveAll,
            CommandKind::Open,
            CommandKind::Quit,
            CommandKind::Undo,
            CommandKind::Redo,
            CommandKind::Find,
            CommandKind::Replace,
            CommandKind::GotoLine,
        ] {
            assert!(kinds.contains(&expected), "missing {expected:?}");
        }
    }
}
