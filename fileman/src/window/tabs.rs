//! Multi-tab paths for the main browsing area (single [`crate::navigation::NavigationState`] shows `paths[active]`).

use std::path::PathBuf;

/// Lightweight tab model: one folder path per tab.
#[derive(Debug, Clone)]
pub struct TabModel {
    pub paths: Vec<PathBuf>,
    pub active: usize,
}

impl TabModel {
    pub fn new(initial: PathBuf) -> Self {
        Self {
            paths: vec![initial],
            active: 0,
        }
    }

    pub fn active_path(&self) -> Option<PathBuf> {
        self.paths.get(self.active).cloned()
    }

    pub fn new_tab(&mut self, path: PathBuf) {
        self.paths.push(path);
        self.active = self.paths.len().saturating_sub(1);
    }

    pub fn close_active(&mut self) -> bool {
        if self.paths.len() <= 1 {
            return false;
        }
        self.paths.remove(self.active);
        if self.active >= self.paths.len() {
            self.active = self.paths.len().saturating_sub(1);
        }
        true
    }

    pub fn set_active(&mut self, index: usize) -> bool {
        if index < self.paths.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    pub fn replace_active_path(&mut self, path: PathBuf) {
        if let Some(p) = self.paths.get_mut(self.active) {
            *p = path;
        }
    }
}
