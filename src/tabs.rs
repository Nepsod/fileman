use nptk::std::path::PathBuf;

use crate::navigation::NavigationState;

#[derive(Debug, Clone)]
pub struct TabEntry {
    pub navigation: NavigationState,
}

#[derive(Debug, Clone)]
pub struct TabModel {
    pub entries: Vec<TabEntry>,
    pub active: usize,
}

impl TabModel {
    pub fn new(initial: PathBuf) -> Self {
        Self {
            entries: vec![TabEntry {
                navigation: NavigationState::new(initial),
            }],
            active: 0,
        }
    }

    pub fn active_path(&self) -> Option<PathBuf> {
        self.entries
            .get(self.active)
            .map(|entry| entry.navigation.current_path())
    }

    pub fn active_navigation_mut(&mut self) -> Option<&mut NavigationState> {
        self.entries
            .get_mut(self.active)
            .map(|entry| &mut entry.navigation)
    }

    pub fn new_tab(&mut self, path: PathBuf) {
        self.entries.push(TabEntry {
            navigation: NavigationState::new(path),
        });
        self.active = self.entries.len().saturating_sub(1);
    }

    pub fn close_active(&mut self) -> bool {
        self.close_at(self.active)
    }

    pub fn close_at(&mut self, index: usize) -> bool {
        if self.entries.len() <= 1 || index >= self.entries.len() {
            return false;
        }
        self.entries.remove(index);
        if self.active == index {
            self.active = index.min(self.entries.len().saturating_sub(1));
        } else if self.active > index {
            self.active -= 1;
        }
        true
    }

    pub fn set_active(&mut self, index: usize) -> bool {
        if index < self.entries.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    pub fn tab_label(index: usize, path: &PathBuf) -> String {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| format!("Tab {}", index + 1))
    }

    pub fn paths_for_strip(&self) -> Vec<PathBuf> {
        self.entries
            .iter()
            .map(|entry| entry.navigation.current_path())
            .collect()
    }
}
