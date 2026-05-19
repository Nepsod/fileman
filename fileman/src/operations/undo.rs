//! Last-in stack of reversible file moves (used for cut + paste).

use std::path::PathBuf;

use super::rename_path;

/// A single logical move the user can step through with undo/redo.
#[derive(Debug, Clone)]
enum UndoAction {
    Move { source: PathBuf, dest: PathBuf },
}

/// Stack of operations for **Edit → Undo / Redo** (cut/paste moves only in the current session).
pub struct UndoStack {
    undo: Vec<UndoAction>,
    redo: Vec<UndoAction>,
    max_depth: usize,
}

impl UndoStack {
    pub fn new(max_depth: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            max_depth: max_depth.max(1),
        }
    }

    /// Record a successful move (e.g. cut line out of the paste job).
    pub fn push_move(&mut self, source: PathBuf, dest: PathBuf) {
        self.redo.clear();
        self.undo.push(UndoAction::Move { source, dest });
        while self.undo.len() > self.max_depth {
            self.undo.remove(0);
        }
    }

    /// Pop the last move and put the file back to `source`.
    pub fn undo_one(&mut self) -> Result<(), String> {
        let Some(UndoAction::Move { source, dest }) = self.undo.pop() else {
            return Err("Nothing to undo".to_string());
        };
        rename_path(dest.clone(), source.clone())?;
        self.redo.push(UndoAction::Move { source, dest });
        Ok(())
    }

    /// Re-apply the last undone move.
    pub fn redo_one(&mut self) -> Result<(), String> {
        let Some(UndoAction::Move { source, dest }) = self.redo.pop() else {
            return Err("Nothing to redo".to_string());
        };
        rename_path(source.clone(), dest.clone())?;
        self.undo.push(UndoAction::Move { source, dest });
        Ok(())
    }
}
