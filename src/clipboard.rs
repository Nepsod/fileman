use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct FileClipboard {
    paths: Vec<PathBuf>,
    is_cut: bool,
}

impl FileClipboard {
    pub fn set_files(&mut self, paths: Vec<PathBuf>, is_cut: bool) {
        self.paths = paths;
        self.is_cut = is_cut;
    }

    pub fn take_files(&mut self) -> Option<(Vec<PathBuf>, bool)> {
        if self.paths.is_empty() {
            return None;
        }

        let paths = std::mem::take(&mut self.paths);
        let is_cut = self.is_cut;
        self.is_cut = false;
        Some((paths, is_cut))
    }

    pub fn peek(&self) -> Option<(&[PathBuf], bool)> {
        if self.paths.is_empty() {
            None
        } else {
            Some((&self.paths, self.is_cut))
        }
    }

    pub fn clear(&mut self) {
        self.paths.clear();
        self.is_cut = false;
    }
}
