use nptk::core::signal::{state::StateSignal, Signal};
use std::path::PathBuf;

/// Manages navigation state including path history
pub struct NavigationState {
    /// Path history for back/forward navigation
    path_history: Vec<PathBuf>,
    /// Current position in history
    history_position: usize,
    /// Current path (reactive signal)
    current_path: StateSignal<PathBuf>,
}

impl NavigationState {
    /// Create a new navigation state with an initial path
    pub fn new(initial_path: PathBuf) -> Self {
        let current_path = StateSignal::new(initial_path.clone());
        Self {
            path_history: vec![initial_path],
            history_position: 0,
            current_path,
        }
    }

    /// Get the current path signal
    pub fn current_path(&self) -> &StateSignal<PathBuf> {
        &self.current_path
    }

    /// Navigate to a new path
    pub fn navigate_to(&mut self, path: PathBuf) {
        // Only add to history if it's different from current
        let current = if self.history_position < self.path_history.len() {
            self.path_history[self.history_position].clone()
        } else {
            // Fallback to getting from signal if history is inconsistent
            (*self.current_path.get()).clone()
        };
        
        if current != path {
            // Remove any history after current position
            self.path_history.truncate(self.history_position + 1);
            // Add new path to history
            self.path_history.push(path.clone());
            self.history_position = self.path_history.len() - 1;
            self.current_path.set_value(path);
        }
    }

    /// Navigate back in history
    pub fn go_back(&mut self) -> Option<PathBuf> {
        if self.can_go_back() {
            self.history_position -= 1;
            let path = self.path_history[self.history_position].clone();
            self.current_path.set_value(path.clone());
            Some(path)
        } else {
            None
        }
    }

    /// Navigate forward in history
    pub fn go_forward(&mut self) -> Option<PathBuf> {
        if self.can_go_forward() {
            self.history_position += 1;
            let path = self.path_history[self.history_position].clone();
            self.current_path.set_value(path.clone());
            Some(path)
        } else {
            None
        }
    }

    /// Check if we can go back
    pub fn can_go_back(&self) -> bool {
        self.history_position > 0
    }

    /// Check if we can go forward
    pub fn can_go_forward(&self) -> bool {
        self.history_position < self.path_history.len() - 1
    }

    /// Get the current path
    pub fn get_current_path(&self) -> PathBuf {
        if self.history_position < self.path_history.len() {
            self.path_history[self.history_position].clone()
        } else {
            // Fallback to signal if history is inconsistent
            (*self.current_path.get()).clone()
        }
    }

    /// Get parent directory
    pub fn parent_path(&self) -> Option<PathBuf> {
        let current = if self.history_position < self.path_history.len() {
            self.path_history[self.history_position].clone()
        } else {
            (*self.current_path.get()).clone()
        };
        current.parent().map(PathBuf::from)
    }
}
