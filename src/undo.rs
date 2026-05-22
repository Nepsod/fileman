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

#[cfg(test)]
mod tests {
    use super::*;
    use nptk::std::fs;

    fn test_directory(label: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "fileman_undo_test_{label}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create undo test directory");
        directory
    }

    #[test]
    fn push_move_clears_redo_stack() {
        let directory = test_directory("redo_clear");
        let first_source = directory.join("first.txt");
        let first_dest = directory.join("first_moved.txt");
        let second_source = directory.join("second.txt");
        let second_dest = directory.join("second_moved.txt");
        fs::write(&first_dest, b"1").unwrap();
        fs::write(&second_dest, b"2").unwrap();

        let mut stack = UndoStack::new(8);
        stack.push_move(first_source.clone(), first_dest.clone());
        stack.undo_one().expect("undo first move");
        stack.push_move(second_source, second_dest);
        assert!(stack.redo_one().is_err());

        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn max_depth_drops_oldest_undo_entry() {
        let directory = test_directory("max_depth");
        let mut stack = UndoStack::new(2);
        for index in 0..3 {
            let source = directory.join(format!("file{index}.txt"));
            let dest = directory.join(format!("moved{index}.txt"));
            fs::write(&dest, format!("{index}").as_bytes()).unwrap();
            stack.push_move(source, dest);
        }

        assert!(stack.undo_one().is_ok());
        assert!(stack.undo_one().is_ok());
        assert!(stack.undo_one().is_err());

        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn undo_and_redo_round_trip_rename() {
        let directory = test_directory("round_trip");
        let source = directory.join("original.txt");
        let dest = directory.join("renamed.txt");
        fs::write(&dest, b"payload").unwrap();

        let mut stack = UndoStack::new(8);
        stack.push_move(source.clone(), dest.clone());
        stack.undo_one().expect("undo rename");
        assert!(source.exists());
        assert!(!dest.exists());

        stack.redo_one().expect("redo rename");
        assert!(!source.exists());
        assert!(dest.exists());

        let _ = fs::remove_dir_all(&directory);
    }
}
