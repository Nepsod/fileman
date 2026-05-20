use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct NavigationState {
    path_history: Vec<PathBuf>,
    history_position: usize,
}

impl NavigationState {
    pub fn new(initial_path: PathBuf) -> Self {
        Self {
            path_history: vec![initial_path],
            history_position: 0,
        }
    }

    pub fn current_path(&self) -> PathBuf {
        self.path_history
            .get(self.history_position)
            .cloned()
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    pub fn navigate_to(&mut self, path: PathBuf) {
        let current = self.current_path();
        if current == path {
            return;
        }
        self.path_history.truncate(self.history_position + 1);
        self.path_history.push(path);
        self.history_position = self.path_history.len().saturating_sub(1);
    }

    pub fn go_back(&mut self) -> Option<PathBuf> {
        if !self.can_go_back() {
            return None;
        }
        self.history_position -= 1;
        Some(self.current_path())
    }

    pub fn go_forward(&mut self) -> Option<PathBuf> {
        if !self.can_go_forward() {
            return None;
        }
        self.history_position += 1;
        Some(self.current_path())
    }

    pub fn can_go_back(&self) -> bool {
        self.history_position > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.history_position < self.path_history.len().saturating_sub(1)
    }
}
