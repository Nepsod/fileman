use nptk::std::path::PathBuf;

use crate::operations::rename_path;

#[derive(Debug, Clone)]
enum UndoAction {
    Move { source: PathBuf, dest: PathBuf },
}

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

    pub fn push_move(&mut self, source: PathBuf, dest: PathBuf) {
        self.redo.clear();
        self.undo.push(UndoAction::Move { source, dest });
        while self.undo.len() > self.max_depth {
            self.undo.remove(0);
        }
    }

    pub fn undo_one(&mut self) -> Result<(), String> {
        let Some(UndoAction::Move { source, dest }) = self.undo.pop() else {
            return Err("Nothing to undo".to_string());
        };
        rename_path(dest.clone(), source.clone())?;
        self.redo.push(UndoAction::Move { source, dest });
        Ok(())
    }

    pub fn redo_one(&mut self) -> Result<(), String> {
        let Some(UndoAction::Move { source, dest }) = self.redo.pop() else {
            return Err("Nothing to redo".to_string());
        };
        rename_path(source.clone(), dest.clone())?;
        self.undo.push(UndoAction::Move { source, dest });
        Ok(())
    }
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new(256)
    }
}
