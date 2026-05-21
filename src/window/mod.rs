//! Main window state. `logic/` holds navigation, search, selection, and file ops;
//! `render/` holds GPUI layout and dialogs (`impl Render` stays in `render/mod.rs`).

mod format;
pub(crate) mod imports;
pub mod logic;
pub mod render;

use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use nptk::gpui::{
    Entity, FocusHandle, Pixels, Point, SharedString, Subscription, Task, UniformListScrollHandle,
};
use nptk::std::collections::HashSet;
use npio::FileInfo;
use nptk::ui::ContextMenu;

use crate::clipboard::FileClipboard;
use crate::config::FilemanConfig;
use crate::devices::VolumeMount;
use crate::drag::MarqueeDrag;
use crate::icons;
use crate::properties::PropertiesDialog;
use crate::search::{SearchMatch, SearchScope};
use crate::settings::SettingsDraft;
use crate::sort::{SortColumn, SortOrder};
use crate::tabs::TabModel;
use crate::toolbar_input::ToolbarLineInput;
use crate::undo::UndoStack;
use crate::view_mode::ViewMode;

pub(crate) use format::{
    days_in_month, days_in_year, delete_confirmation_message, format_file_type, format_modified,
    format_size, format_unix_timestamp, path_to_file_uri, quick_access_places,
};

#[derive(Clone)]
pub(crate) struct PendingDelete {
    paths: Vec<PathBuf>,
    permanent: bool,
    use_trash: bool,
}

#[derive(Clone)]
pub(crate) struct PendingRename {
    path: PathBuf,
    new_name: String,
}

#[derive(Clone)]
pub(crate) struct PendingPasteChoice {
    sources: Vec<PathBuf>,
    destination_directory: PathBuf,
    is_cut: bool,
    conflict_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextMenuTarget {
    Background,
    FileList,
}

pub struct FilemanWindow {
    current_path: PathBuf,
    show_hidden: bool,
    selected_files: HashSet<String>,
    selection_anchor: Option<usize>,
    files: Vec<FileInfo>,
    config: FilemanConfig,
    path_input_text: String,
    focus_handle: FocusHandle,
    sort_column: SortColumn,
    sort_order: SortOrder,
    status_message: SharedString,
    pending_delete: Option<PendingDelete>,
    pending_rename: Option<PendingRename>,
    loading_directory: bool,
    view_mode: ViewMode,
    clipboard: FileClipboard,
    search_query: String,
    search_active: bool,
    search_scope: SearchScope,
    search_matches: Vec<SearchMatch>,
    search_in_progress: bool,
    search_history: Vec<String>,
    list_focus_index: Option<usize>,
    path_edit_active: bool,
    directory_watcher: Option<notify::RecommendedWatcher>,
    directory_reload_generation: u64,
    tabs: TabModel,
    bookmark_paths: Vec<PathBuf>,
    volume_mounts: Vec<VolumeMount>,
    icon_cache: crate::icons::FileIconCache,
    icon_size: u32,
    undo_stack: UndoStack,
    context_menu: Option<(Entity<ContextMenu>, Point<Pixels>, Subscription)>,
    pending_properties: Option<PropertiesDialog>,
    pending_settings: Option<SettingsDraft>,
    settings_terminal_focus: bool,
    pending_paste_choice: Option<PendingPasteChoice>,
    paste_cancel: Option<Arc<AtomicBool>>,
    files_scroll_handle: UniformListScrollHandle,
    uniform_list_row_height: Option<Pixels>,
    marquee_drag: Option<MarqueeDrag>,
    marquee_cancel_subscription: Option<Subscription>,
    marquee_autoscroll_task: Option<Task<()>>,
    list_visible_range: Option<Range<usize>>,
    show_about: bool,
    path_line_input: Entity<ToolbarLineInput>,
    search_line_input: Entity<ToolbarLineInput>,
    _toolbar_subscriptions: Vec<Subscription>,
}
