use std::path::PathBuf;
use nptk::core::model::SortOrder;

/// Search scope for the location bar filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchScope {
    /// Filter only entries in the current folder.
    CurrentFolder,
    /// Search in current folder and all subfolders (recursive).
    FolderAndSubfolders,
}

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
    Properties(Vec<PathBuf>),
    PromptRename(PathBuf),
    Copy(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
    Paste,
    /// Sort files by column and order
    Sort(usize, SortOrder),
    /// Refresh the file list
    Refresh,
    /// Open current selection (Enter): folders navigate, files launch default app.
    Open,
    /// Open explicit paths (e.g. context menu).
    OpenPaths(Vec<PathBuf>),
    /// Duplicate selected paths (files sync, folders async in app wrapper).
    Duplicate(Vec<PathBuf>),
    /// Clear selection (menubar / wrapper).
    DeselectAll,
    /// Invert selection against current listed entries.
    InvertSelection,
    /// Move to trash after confirmation (Delete key, context menu, …).
    DeleteToTrash(Vec<PathBuf>),
    /// Permanently delete after confirmation (Shift+Delete).
    DeletePermanent(Vec<PathBuf>),
    /// Go to parent directory (Backspace, Alt+Up).
    NavigateUp,
    /// Prompt to create a new folder in the current directory (empty-area context).
    PromptNewFolder,
    /// Prompt to create a new file in the current directory (empty-area context).
    PromptNewFile,
}

#[derive(Clone)]
pub(crate) struct PendingAction {
    pub paths: Vec<PathBuf>,
    pub app_id: Option<String>,
    pub properties: bool,
    pub delete: bool, // If true, this is a trash action
    pub delete_permanent: bool, // If true, this is a permanent delete action
}
