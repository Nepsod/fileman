//! Drag-and-drop payload types for intra-app file operations (Milestone 4 extension surface).

use std::path::PathBuf;

/// Paths being dragged from the file list or sidebar.
#[derive(Debug, Clone)]
pub enum FileDnDPayload {
    Paths(Vec<PathBuf>),
}
