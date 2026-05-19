//! Channel-delivered file operations from toolbar and menus.

use nptk::core::model::SortOrder;
use std::path::PathBuf;

/// File operation requests that can be sent from UI to be processed
#[derive(Debug, Clone)]
pub enum FileOperationRequest {
    /// Move selected paths to trash after confirmation.
    Delete(Vec<PathBuf>),
    /// Permanently delete after confirmation (e.g. Shift+Delete from list).
    DeletePermanent(Vec<PathBuf>),
    // CreateDirectory { parent: PathBuf, name: String }, // Unused
    // Rename { from: PathBuf, to: PathBuf }, // Unused
    PromptRename(PathBuf), // Prompt for new name for single file
    PromptCreateDirectory(PathBuf), // Prompt for new directory name in parent
    PromptCreateFile(PathBuf),      // Prompt for new empty file name in parent
    Properties(Vec<PathBuf>),
    Copy(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
    Paste,
    /// Reload entries for the current path (same as list context "refresh" behavior).
    Refresh,
    /// Sort file list by column index and order (same as list context menu).
    Sort(usize, SortOrder),
    /// Select all listed entries (same as Ctrl+A in the file list).
    SelectAll,
    /// Clear file list selection.
    DeselectAll,
    /// Invert file list selection.
    InvertSelection,
    /// Open selected paths (folders navigate, files launch default app).
    OpenSelection,
    Duplicate(Vec<PathBuf>),
    /// Spawn terminal with cwd = current folder.
    OpenTerminalHere,
    /// Show Help → About popup.
    ShowAbout,
    /// Open Settings → Configure Fileman.
    ShowSettings,
    /// Undo last reversible operation (cut/paste move).
    Undo,
    /// Redo last undone operation.
    Redo,
    /// Open a new tab (starts at `$HOME` when available).
    NewTab,
    /// Spawn a new window process.
    NewWindow,
    /// Close the current tab (ignored when only one tab remains).
    CloseTab,
    /// Activate a tab by index.
    SwitchTab(usize),
    /// Close a tab by index.
    CloseTabAt(usize),
    /// Increase icon size in icon/compact views.
    ZoomIn,
    /// Decrease icon size in icon/compact views.
    ZoomOut,
    /// Reset icon size to the default.
    ZoomReset,
}
