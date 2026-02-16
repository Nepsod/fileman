use std::path::PathBuf;
use nptk::core::model::SortOrder;

/// View mode for the file list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileListViewMode {
    /// List view (icon + text in rows)
    List,
    /// Large icon view (grid layout with icons and labels below)
    Icon,
    /// Compact view (Tiles view: Icon left, Text right, grid layout)
    Compact,
    /// Table view (Details view with columns)
    Table,
}

/// Simple operation request type for use within FileList widget
/// This is converted to the full FileOperationRequest in FileListWrapper
pub enum FileListOperation {
    Delete(Vec<PathBuf>),
    Properties(Vec<PathBuf>),
    PromptRename(PathBuf),
    Copy(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
    Paste,
    /// Sort files by column and order
    Sort(usize, SortOrder),
    /// Refresh the file list
    Refresh,
}

#[derive(Clone)]
pub(crate) struct PendingAction {
    pub paths: Vec<PathBuf>,
    pub app_id: Option<String>,
    pub properties: bool,
    pub delete: bool, // If true, this is a delete action
}
