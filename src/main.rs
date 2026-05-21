mod about;
mod bookmarks;
mod clipboard;
mod config;
mod devices;
mod drag;
mod icons;
mod jobs;
mod location_bar;
mod navigation;
mod open;
mod operations;
mod properties;
mod search;
mod settings;
mod sort;
mod tabs;
mod terminal;
mod toolbar_input;
mod ui_icons;
mod undo;
mod view_mode;
mod watch;

use nptk::std::collections::HashSet;
use nptk::std::ops::Range;
use nptk::std::path::{Path, PathBuf};
use nptk::std::sync::atomic::{AtomicBool, Ordering};
use nptk::std::sync::mpsc;
use nptk::std::sync::Arc;
use nptk::std::time::Duration;

use nptk::gpui::{self as gpui, uniform_list, ScrollStrategy, UniformListScrollHandle, *};
use nptk::gpui_tokio::Tokio;
use nptk::theme::ActiveTheme;
use nptk::ui::{
    Checkbox, ContextMenu, DropdownMenu, DropdownStyle, ListItem, ToggleState, WithScrollbar,
    prelude::*,
};
use npio::backend::local::LocalBackend;
use npio::{get_file_for_uri, register_backend, FileInfo, FileType};
use sort::{SortColumn, SortOrder};
use view_mode::ViewMode;

use nptk::file_icons::FileIconPresentation;

use crate::clipboard::FileClipboard;
use crate::config::FilemanConfig;
use crate::devices::VolumeMount;
use crate::drag::{drop_target_style, DraggedFilePaths, MarqueeDrag};
use crate::search::{SearchMatch, SearchScope};
use crate::jobs::{
    count_paste_conflicts, ConflictResolution, PasteJobSettings, run_paste_batch,
};
use crate::location_bar::breadcrumb_segments;
use crate::settings::{SettingsDraft, SettingsField};
use crate::tabs::TabModel;
use crate::toolbar_input::{ToolbarLineInput, ToolbarLineInputEvent};
use crate::ui_icons::ThemeIconButton;
use crate::view_mode::{clamp_icon_size, ICON_ZOOM_STEP, MAX_ICON_SIZE, MIN_ICON_SIZE};
use crate::operations::{
    create_directory, create_file, delete_path, duplicate_path, move_to_trash, PasteResult,
    rename_path, unique_name_in_parent,
};
use crate::properties::PropertiesDialog;
use crate::undo::UndoStack;

type ViewContext<'a, T> = gpui::Context<'a, T>;

#[derive(Clone)]
struct PendingDelete {
    paths: Vec<PathBuf>,
    permanent: bool,
    use_trash: bool,
}

#[derive(Clone)]
struct PendingRename {
    path: PathBuf,
    new_name: String,
}

#[derive(Clone)]
struct PendingPasteChoice {
    sources: Vec<PathBuf>,
    destination_directory: PathBuf,
    is_cut: bool,
    conflict_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextMenuTarget {
    Background,
    FileList,
}

struct FilemanWindow {
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
    icon_cache: icons::FileIconCache,
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

gpui::actions!(
    fileman,
    [
        CreateFolder,
        CreateFile,
        GoBack,
        GoForward,
        GoUp,
        ToggleHidden,
        DeleteSelected,
        DeletePermanent,
        Refresh,
        SelectAll,
        Rename,
        Copy,
        Cut,
        Paste,
        Duplicate,
        ClearSelection,
        InvertSelection,
        ActivateSearch,
        ClearSearch,
        ToggleSearchSubfolders,
        FocusPathBar,
        GoHome,
        NewTab,
        NewWindow,
        CloseTab,
        AddBookmark,
        RemoveBookmark,
        SortByName,
        SortBySize,
        SortByModified,
        SortByType,
        ToggleSortOrder,
        SortNameAsc,
        SortNameDesc,
        SortSizeAsc,
        SortSizeDesc,
        SortModifiedAsc,
        SortModifiedDesc,
        SortTypeAsc,
        SortTypeDesc,
        SetSearchCurrentFolder,
        SetSearchIncludeSubfolders,
        Undo,
        Redo,
        OpenTerminal,
        OpenSelection,
        OpenWithSystem,
        ShowProperties,
        ShowSettings,
        ShowAbout,
        GoToParent,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        ViewList,
        ViewIcon,
        ViewCompact,
        ViewTable,
        Quit
    ]
);

impl FilemanWindow {
    fn new(initial_path: PathBuf, cx: &mut ViewContext<Self>) -> Self {
        toolbar_input::register_keybindings(cx);

        let config = FilemanConfig::load_or_create();
        let sort_column = config.sort_column();
        let sort_order = config.sort_order();
        let view_mode = config.view_mode();
        let icon_size = config.icon_size_for_mode(view_mode);

        let path_line_input =
            cx.new(|cx| ToolbarLineInput::new("Enter path…", cx));
        let search_line_input =
            cx.new(|cx| ToolbarLineInput::new("Search…", cx));

        let path_subscription = cx.subscribe(
            &path_line_input,
            |this, _, event: &ToolbarLineInputEvent, cx| {
                this.handle_path_input_event(event.clone(), cx);
            },
        );
        let search_subscription = cx.subscribe(
            &search_line_input,
            |this, _, event: &ToolbarLineInputEvent, cx| {
                this.handle_search_input_event(event.clone(), cx);
            },
        );

        let mut this = Self {
            current_path: initial_path.clone(),
            show_hidden: config.folder_view.show_hidden,
            selected_files: HashSet::new(),
            selection_anchor: None,
            files: Vec::new(),
            config,
            path_input_text: initial_path.to_string_lossy().to_string(),
            focus_handle: cx.focus_handle(),
            sort_column,
            sort_order,
            status_message: SharedString::from("Ready"),
            pending_delete: None,
            pending_rename: None,
            loading_directory: false,
            view_mode,
            clipboard: FileClipboard::default(),
            search_query: String::new(),
            search_active: false,
            search_scope: SearchScope::CurrentFolder,
            search_matches: Vec::new(),
            search_in_progress: false,
            search_history: Vec::new(),
            list_focus_index: None,
            path_edit_active: false,
            directory_watcher: None,
            directory_reload_generation: 0,
            tabs: TabModel::new(initial_path.clone()),
            bookmark_paths: bookmarks::load_bookmarks(),
            volume_mounts: devices::list_removable_mounts(),
            icon_cache: icons::FileIconCache::new(),
            icon_size,
            undo_stack: UndoStack::default(),
            context_menu: None,
            pending_properties: None,
            pending_settings: None,
            settings_terminal_focus: false,
            pending_paste_choice: None,
            paste_cancel: None,
            files_scroll_handle: UniformListScrollHandle::new(),
            uniform_list_row_height: None,
            marquee_drag: None,
            marquee_cancel_subscription: None,
            marquee_autoscroll_task: None,
            list_visible_range: None,
            show_about: false,
            path_line_input,
            search_line_input,
            _toolbar_subscriptions: vec![path_subscription, search_subscription],
        };

        this.register_menus(cx);
        this.register_keybindings(cx);
        this.navigate_to(initial_path, false, cx);
        this.queue_ui_icon_loads(cx);
        this.start_volume_monitor(cx);

        cx.on_app_quit(|this, cx| {
            this.persist_window_geometry(cx);
            async move {}
        })
        .detach();

        this
    }

    fn register_menus(&self, cx: &mut ViewContext<Self>) {
        let sort_menu = Menu::new("Sort By").items(vec![
            MenuItem::action("Name (Ascending)", SortNameAsc),
            MenuItem::action("Name (Descending)", SortNameDesc),
            MenuItem::action("Size (Ascending)", SortSizeAsc),
            MenuItem::action("Size (Descending)", SortSizeDesc),
            MenuItem::action("Date Modified (Ascending)", SortModifiedAsc),
            MenuItem::action("Date Modified (Descending)", SortModifiedDesc),
            MenuItem::action("Type (Ascending)", SortTypeAsc),
            MenuItem::action("Type (Descending)", SortTypeDesc),
        ]);

        cx.set_menus(vec![
            Menu::new("File").items(vec![
                MenuItem::action("Home", GoHome),
                MenuItem::action("New Tab", NewTab),
                MenuItem::action("New Window", NewWindow),
                MenuItem::action("Close Tab", CloseTab),
                MenuItem::separator(),
                MenuItem::action("New Folder", CreateFolder),
                MenuItem::action("New File", CreateFile),
                MenuItem::action("Open", OpenSelection),
                MenuItem::action("Properties", ShowProperties),
                MenuItem::separator(),
                MenuItem::action("Back", GoBack),
                MenuItem::action("Forward", GoForward),
                MenuItem::action("Up", GoUp),
            ]),
            Menu::new("Edit").items(vec![
                MenuItem::action("Focus Location Bar", FocusPathBar),
                MenuItem::action("Activate Search", ActivateSearch),
                MenuItem::separator(),
                MenuItem::action("Select All", SelectAll),
                MenuItem::action("Deselect All", ClearSelection),
                MenuItem::action("Invert Selection", InvertSelection),
                MenuItem::separator(),
                MenuItem::action("Copy", Copy),
                MenuItem::action("Cut", Cut),
                MenuItem::action("Paste", Paste),
                MenuItem::separator(),
                MenuItem::action("Undo", Undo),
                MenuItem::action("Redo", Redo),
                MenuItem::separator(),
                MenuItem::action("Rename", Rename),
                MenuItem::action("Move to Trash", DeleteSelected),
                MenuItem::action("Delete Permanently", DeletePermanent),
                MenuItem::action("Duplicate", Duplicate),
            ]),
            Menu::new("View").items(vec![
                MenuItem::action("Refresh", Refresh),
                MenuItem::submenu(sort_menu),
                MenuItem::action("Search: Current Folder", SetSearchCurrentFolder),
                MenuItem::action("Search: Include Subfolders", SetSearchIncludeSubfolders),
                MenuItem::action("Show Hidden Files", ToggleHidden),
                MenuItem::separator(),
                MenuItem::action("List View", ViewList),
                MenuItem::action("Icon View", ViewIcon),
                MenuItem::action("Compact View", ViewCompact),
                MenuItem::action("Table View", ViewTable),
                MenuItem::separator(),
                MenuItem::action("Zoom In", ZoomIn),
                MenuItem::action("Zoom Out", ZoomOut),
                MenuItem::action("Zoom Reset", ZoomReset),
            ]),
            Menu::new("Tools").items(vec![MenuItem::action(
                "Open Terminal Here",
                OpenTerminal,
            )]),
            Menu::new("Bookmarks").items(vec![
                MenuItem::action("Add Current Folder", AddBookmark),
                MenuItem::action("Remove Current Folder", RemoveBookmark),
            ]),
            Menu::new("Settings").items(vec![MenuItem::action(
                "Configure Fileman",
                ShowSettings,
            )]),
            Menu::new("Help").items(vec![MenuItem::action("About", ShowAbout)]),
        ]);
    }

    fn register_keybindings(&self, cx: &mut ViewContext<Self>) {
        cx.bind_keys([
            KeyBinding::new("f5", Refresh, None),
            KeyBinding::new("f6", FocusPathBar, None),
            KeyBinding::new("backspace", GoUp, None),
            KeyBinding::new("alt-up", GoUp, None),
            KeyBinding::new("delete", DeleteSelected, None),
            KeyBinding::new("shift-delete", DeletePermanent, None),
            KeyBinding::new("f2", Rename, None),
            KeyBinding::new("ctrl-a", SelectAll, None),
            KeyBinding::new("ctrl-c", Copy, None),
            KeyBinding::new("ctrl-x", Cut, None),
            KeyBinding::new("ctrl-v", Paste, None),
            KeyBinding::new("ctrl-z", Undo, None),
            KeyBinding::new("ctrl-shift-z", Redo, None),
            KeyBinding::new("ctrl-d", Duplicate, None),
            KeyBinding::new("ctrl-f", ActivateSearch, None),
            KeyBinding::new("ctrl-shift-f", ToggleSearchSubfolders, None),
            KeyBinding::new("ctrl-l", FocusPathBar, None),
            KeyBinding::new("ctrl-comma", ShowSettings, None),
            KeyBinding::new("ctrl-equal", ZoomIn, None),
            KeyBinding::new("ctrl-minus", ZoomOut, None),
            KeyBinding::new("ctrl-0", ZoomReset, None),
            KeyBinding::new("ctrl-n", NewWindow, None),
            KeyBinding::new("ctrl-t", NewTab, None),
            KeyBinding::new("ctrl-w", CloseTab, None),
            KeyBinding::new("escape", ClearSelection, None),
            KeyBinding::new("ctrl-shift-enter", OpenWithSystem, None),
        ]);
    }

    fn persist_window_geometry(&mut self, cx: &mut App) {
        if !self.config.window.remember_window_size {
            return;
        }

        for window_handle in cx.windows() {
            let Ok(()) = window_handle.update(cx, |_, window, _| {
                let bounds = window.bounds();
                self.config.window.last_window_width =
                    f32::from(bounds.size.width).round() as u32;
                self.config.window.last_window_height =
                    f32::from(bounds.size.height).round() as u32;
            }) else {
                continue;
            };
            self.config.save();
            break;
        }
    }

    fn set_status(&mut self, message: impl Into<SharedString>, cx: &mut ViewContext<Self>) {
        self.status_message = message.into();
        cx.notify();
    }

    fn reload_current_directory(&mut self, cx: &mut ViewContext<Self>) {
        self.reload_directory_entries(true, cx);
        if self.using_subfolder_search() {
            self.schedule_subfolder_search(cx);
        }
    }

    fn navigate_to(&mut self, path: PathBuf, record_history: bool, cx: &mut ViewContext<Self>) {
        if record_history {
            if let Some(navigation) = self.tabs.active_navigation_mut() {
                navigation.navigate_to(path.clone());
            }
        }
        self.current_path = path.clone();
        self.path_input_text = path.to_string_lossy().to_string();
        if !self.path_edit_active {
            self.path_line_input.update(cx, |input, cx| {
                input.set_text(self.path_input_text.clone(), cx);
            });
        }
        self.selected_files.clear();
        self.selection_anchor = None;
        self.list_focus_index = None;
        self.search_matches.clear();
        self.restart_directory_watch(cx);
        self.reload_directory_entries(true, cx);
        if self.using_subfolder_search() {
            self.schedule_subfolder_search(cx);
        }
        cx.notify();
    }

    fn reload_directory_entries(&mut self, show_loading: bool, cx: &mut ViewContext<Self>) {
        let path = self.current_path.clone();
        self.directory_reload_generation = self.directory_reload_generation.wrapping_add(1);
        let generation = self.directory_reload_generation;
        if show_loading {
            self.loading_directory = true;
            self.set_status("Loading…", cx);
        }

        let path_string = path_to_file_uri(&path);
        cx.spawn(async move |this, cx| {
            let files_result = Tokio::spawn(cx, async move {
                if let Ok(directory) = get_file_for_uri(&path_string) {
                    let mut entries = Vec::new();
                    if let Ok(mut enumerator) = directory
                        .enumerate_children("standard::*,time::modified", None)
                        .await
                    {
                        while let Ok(Some((info, _child))) = enumerator.next_file(None).await {
                            entries.push(info);
                        }
                        let _ = enumerator.close(None).await;
                    }
                    Ok(entries)
                } else {
                    Err(())
                }
            })
            .await;

            let _ = this.update(cx, |this, cx| {
                if generation != this.directory_reload_generation {
                    return;
                }
                if show_loading {
                    this.loading_directory = false;
                }
                match files_result {
                    Ok(Ok(mut files)) => {
                        sort::sort_files(&mut files, this.sort_column, this.sort_order);
                        this.files = files;
                        if show_loading && !this.using_subfolder_search() {
                            this.set_status("Ready", cx);
                        }
                        this.queue_icon_loads(cx);
                    }
                    _ => {
                        if show_loading {
                            this.files.clear();
                            this.set_status("Failed to load directory", cx);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn restart_directory_watch(&mut self, cx: &mut ViewContext<Self>) {
        self.directory_watcher = None;
        let watch_path = self.current_path.clone();
        let (notify_sender, notify_receiver) = mpsc::channel();
        self.directory_watcher =
            watch::create_directory_watcher(&watch_path, notify_sender);

        cx.spawn(async move |this, cx| {
            const DEBOUNCE: Duration = Duration::from_millis(500);
            const POLL_INTERVAL: Duration = Duration::from_millis(200);

            loop {
                let mut saw_event = false;
                while notify_receiver.try_recv().is_ok() {
                    saw_event = true;
                }

                if saw_event {
                    cx.background_executor().timer(DEBOUNCE).await;

                    let mut more_events = false;
                    while notify_receiver.try_recv().is_ok() {
                        more_events = true;
                    }
                    if more_events {
                        continue;
                    }

                    let still_watching = this
                        .update(cx, |this, _| this.current_path == watch_path)
                        .unwrap_or(false);
                    if !still_watching {
                        break;
                    }

                    let _ = this.update(cx, |this, cx| {
                        if this.current_path == watch_path {
                            this.reload_directory_entries(false, cx);
                        }
                    });
                }

                cx.background_executor().timer(POLL_INTERVAL).await;
            }
        })
        .detach();
    }

    fn using_subfolder_search(&self) -> bool {
        self.search_active
            && self.search_scope == SearchScope::Subfolders
            && !self.search_query.trim().is_empty()
    }

    fn schedule_subfolder_search(&mut self, cx: &mut ViewContext<Self>) {
        if !self.using_subfolder_search() {
            self.search_matches.clear();
            self.search_in_progress = false;
            cx.notify();
            return;
        }

        let root = self.current_path.clone();
        let query = self.search_query.clone();
        let show_hidden = self.show_hidden;
        self.search_in_progress = true;
        self.set_status("Searching subfolders…", cx);

        cx.spawn(async move |this, cx| {
            let matches = Tokio::spawn(cx, async move {
                search::find_in_subfolders(&root, &query, show_hidden)
            })
            .await
            .unwrap_or_default();

            let _ = this.update(cx, |this, cx| {
                this.search_in_progress = false;
                this.search_matches = matches;
                let count = this.search_matches.len();
                this.set_status(format!("Found {count} matches in subfolders"), cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle_search_subfolders(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
        if !self.search_active {
            self.activate_search(window, cx);
        }
        self.search_scope = match self.search_scope {
            SearchScope::CurrentFolder => SearchScope::Subfolders,
            SearchScope::Subfolders => SearchScope::CurrentFolder,
        };
        self.search_matches.clear();
        if self.using_subfolder_search() {
            self.schedule_subfolder_search(cx);
        } else {
            self.set_status("Search: current folder only", cx);
            cx.notify();
        }
    }

    fn set_search_scope(
        &mut self,
        scope: SearchScope,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        if !self.search_active {
            self.activate_search(window, cx);
        }
        self.search_scope = scope;
        self.search_matches.clear();
        match scope {
            SearchScope::CurrentFolder => {
                self.set_status("Search: current folder only", cx);
                cx.notify();
            }
            SearchScope::Subfolders => {
                if self.using_subfolder_search() {
                    self.schedule_subfolder_search(cx);
                } else {
                    self.set_status("Search: include subfolders", cx);
                    cx.notify();
                }
            }
        }
    }

    fn selection_key_for_path(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn go_back(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(previous) = self
            .tabs
            .active_navigation_mut()
            .and_then(|navigation| navigation.go_back())
        {
            self.navigate_to(previous, false, cx);
        }
    }

    fn go_forward(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(next) = self
            .tabs
            .active_navigation_mut()
            .and_then(|navigation| navigation.go_forward())
        {
            self.navigate_to(next, false, cx);
        }
    }

    fn go_up(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(parent) = self.current_path.parent() {
            self.navigate_to(parent.to_path_buf(), true, cx);
        }
    }

    fn toggle_hidden(&mut self, cx: &mut ViewContext<Self>) {
        self.show_hidden = !self.show_hidden;
        self.config.folder_view.show_hidden = self.show_hidden;
        self.config.save();
        if self.using_subfolder_search() {
            self.schedule_subfolder_search(cx);
        }
        cx.notify();
    }

    fn select_all_visible(&mut self, cx: &mut ViewContext<Self>) {
        if self.using_subfolder_search() {
            self.selected_files = self
                .search_matches
                .iter()
                .map(|search_match| Self::selection_key_for_path(&search_match.path))
                .collect();
        } else {
            self.selected_files = self
                .visible_files()
                .into_iter()
                .filter_map(|file_info| file_info.get_name().map(str::to_string))
                .collect();
        }
        cx.notify();
    }

    fn visible_entry_names(&self) -> Vec<String> {
        if self.using_subfolder_search() {
            self.search_matches
                .iter()
                .map(|search_match| Self::selection_key_for_path(&search_match.path))
                .collect()
        } else {
            self.visible_files()
                .into_iter()
                .filter_map(|file_info| file_info.get_name().map(str::to_string))
                .collect()
        }
    }

    fn select_visible_range(&mut self, anchor_index: usize, index: usize) {
        let names = self.visible_entry_names();
        if names.is_empty() {
            return;
        }
        let start = anchor_index.min(index).min(names.len() - 1);
        let end = anchor_index.max(index).min(names.len() - 1);
        self.selected_files.clear();
        for name in &names[start..=end] {
            self.selected_files.insert(name.clone());
        }
    }

    fn apply_list_selection_click(
        &mut self,
        entry_index: usize,
        entry_name: &str,
        shift: bool,
        control: bool,
        cx: &mut ViewContext<Self>,
    ) {
        if control {
            if !self.selected_files.remove(entry_name) {
                self.selected_files.insert(entry_name.to_string());
            }
            self.selection_anchor = Some(entry_index);
        } else if shift {
            if let Some(anchor) = self.selection_anchor {
                self.select_visible_range(anchor, entry_index);
            } else {
                self.selected_files.clear();
                self.selected_files.insert(entry_name.to_string());
                self.selection_anchor = Some(entry_index);
            }
        } else {
            self.selected_files.clear();
            self.selected_files.insert(entry_name.to_string());
            self.selection_anchor = Some(entry_index);
        }
        self.list_focus_index = Some(entry_index);
        cx.notify();
    }

    fn move_list_focus(&mut self, delta: isize, extend: bool, cx: &mut ViewContext<Self>) {
        let names = self.visible_entry_names();
        if names.is_empty() {
            return;
        }

        let current_index = self
            .list_focus_index
            .unwrap_or_else(|| names.len().saturating_sub(1));
        let next_index = if delta < 0 {
            current_index.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (current_index + delta as usize).min(names.len() - 1)
        };

        self.list_focus_index = Some(next_index);
        self.files_scroll_handle
            .scroll_to_item(next_index, ScrollStrategy::Nearest);

        if extend {
            let anchor = self.selection_anchor.unwrap_or(current_index);
            self.select_visible_range(anchor, next_index);
        } else {
            self.selected_files.clear();
            self.selected_files.insert(names[next_index].clone());
            self.selection_anchor = Some(next_index);
        }
        cx.notify();
    }

    fn open_focused_or_selection(&mut self, cx: &mut ViewContext<Self>) {
        if !self.selected_files.is_empty() {
            self.open_primary_selection(cx);
            return;
        }

        let names = self.visible_entry_names();
        let Some(index) = self.list_focus_index else {
            return;
        };
        let Some(name) = names.get(index) else {
            return;
        };

        if self.using_subfolder_search() {
            let path = PathBuf::from(name);
            if path.is_dir() {
                self.navigate_to(path, true, cx);
            } else {
                self.launch_file(&path, cx);
            }
            return;
        }

        let full_path = self.current_path.join(name);
        if full_path.is_dir() {
            self.navigate_to(full_path, true, cx);
        } else {
            self.launch_file(&full_path, cx);
        }
    }

    fn launch_file(&mut self, path: &Path, cx: &mut ViewContext<Self>) {
        let path = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let result = Tokio::spawn(cx, async move { open::launch_path(path).await }).await;
            let message = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(error.to_string()),
            };
            if let Some(message) = message {
                let _ = this.update(cx, |this, cx| {
                    this.set_status(message, cx);
                });
            }
        })
        .detach();
    }

    fn launch_file_with_application(
        &mut self,
        application_id: String,
        path: PathBuf,
        cx: &mut ViewContext<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result =
                Tokio::spawn(cx, async move { open::launch_with_application(&application_id, path).await })
                    .await;
            let message = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(error.to_string()),
            };
            if let Some(message) = message {
                let _ = this.update(cx, |this, cx| {
                    this.set_status(message, cx);
                });
            }
        })
        .detach();
    }

    fn spawn_new_window(&mut self, cx: &mut ViewContext<Self>) {
        let current_directory = self.current_path.clone();
        match std::env::current_exe() {
            Ok(executable) => {
                match std::process::Command::new(executable)
                    .arg(current_directory.to_string_lossy().to_string())
                    .spawn()
                {
                    Ok(_) => self.set_status("Opened a new window", cx),
                    Err(error) => self.set_status(format!("New window failed: {error}"), cx),
                }
            }
            Err(error) => self.set_status(format!("New window failed: {error}"), cx),
        }
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        if self.using_subfolder_search() {
            self.selected_files
                .iter()
                .map(PathBuf::from)
                .collect()
        } else {
            self.selected_files
                .iter()
                .map(|name| self.current_path.join(name))
                .collect()
        }
    }

    fn paste_dropped_files(
        &mut self,
        sources: Vec<PathBuf>,
        destination_directory: PathBuf,
        is_cut: bool,
        cx: &mut ViewContext<Self>,
    ) {
        if sources.is_empty() {
            return;
        }

        let conflict_count = count_paste_conflicts(&sources, &destination_directory);
        if conflict_count > 0 {
            self.pending_paste_choice = Some(PendingPasteChoice {
                sources,
                destination_directory,
                is_cut,
                conflict_count,
            });
            cx.notify();
            return;
        }

        self.execute_paste(
            sources,
            destination_directory,
            is_cut,
            PasteJobSettings::default(),
            cx,
        );
    }

    fn drop_external_files(&mut self, paths: &gpui::ExternalPaths, cx: &mut ViewContext<Self>) {
        let sources = paths.paths().to_vec();
        if sources.is_empty() {
            return;
        }
        self.paste_dropped_files(sources, self.current_path.clone(), false, cx);
    }

    fn drop_internal_files(&mut self, dragged: &DraggedFilePaths, cx: &mut ViewContext<Self>) {
        let sources = drag::filter_sources_for_destination(
            &dragged.paths,
            &self.current_path,
            true,
        );
        if sources.is_empty() {
            self.set_status("Items are already in this folder", cx);
            return;
        }
        self.paste_dropped_files(sources, self.current_path.clone(), true, cx);
    }

    fn drop_external_into_directory(
        &mut self,
        destination_directory: &Path,
        paths: &gpui::ExternalPaths,
        cx: &mut ViewContext<Self>,
    ) {
        let sources = drag::filter_sources_for_destination(
            &paths.paths().to_vec(),
            destination_directory,
            false,
        );
        if sources.is_empty() {
            self.set_status("Cannot copy into this folder", cx);
            return;
        }
        self.paste_dropped_files(sources, destination_directory.to_path_buf(), false, cx);
    }

    fn drop_into_directory(
        &mut self,
        destination_directory: &Path,
        dragged: &DraggedFilePaths,
        cx: &mut ViewContext<Self>,
    ) {
        let sources =
            drag::filter_sources_for_destination(&dragged.paths, destination_directory, true);
        if sources.is_empty() {
            self.set_status("Cannot move into this folder", cx);
            return;
        }
        self.paste_dropped_files(sources, destination_directory.to_path_buf(), true, cx);
    }

    fn files_list_item_height(&self) -> Pixels {
        match self.view_mode {
            ViewMode::Compact => px(40.0),
            ViewMode::Table => px(36.0),
            ViewMode::List => px(36.0),
            ViewMode::Icon => px(96.0),
        }
    }

    fn marquee_viewport_bounds(&self) -> Bounds<Pixels> {
        self.files_scroll_handle.0.borrow().base_handle.bounds()
    }

    fn marquee_scroll_offset(&self) -> Point<Pixels> {
        self.files_scroll_handle.0.borrow().base_handle.offset()
    }

    fn refresh_uniform_list_row_height(&mut self, item_count: usize) {
        if item_count == 0 {
            self.uniform_list_row_height = None;
            return;
        }

        let state = self.files_scroll_handle.0.borrow();
        let Some(item_size) = state.last_item_size else {
            return;
        };
        let measured = px(item_size.contents.height.as_f32() / item_count as f32);
        if measured > px(0.) {
            self.uniform_list_row_height = Some(measured);
        }
    }

    fn marquee_list_row_height(&self) -> Pixels {
        self.uniform_list_row_height
            .filter(|height| *height > px(0.))
            .unwrap_or_else(|| self.files_list_item_height())
    }

    fn marquee_local_point(&self, window_point: Point<Pixels>) -> Point<Pixels> {
        let bounds = self.marquee_viewport_bounds();
        point(
            window_point.x - bounds.origin.x,
            window_point.y - bounds.origin.y,
        )
    }

    fn clamp_pixels(value: Pixels, min: Pixels, max: Pixels) -> Pixels {
        px(value.as_f32().clamp(min.as_f32(), max.as_f32()))
    }

    fn marquee_clamp_pointer_to_viewport(&self, pointer: Point<Pixels>) -> Point<Pixels> {
        let bounds = self.marquee_viewport_bounds();
        if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
            return pointer;
        }

        point(
            Self::clamp_pixels(pointer.x, bounds.left(), bounds.right()),
            Self::clamp_pixels(pointer.y, bounds.top(), bounds.bottom()),
        )
    }

    fn marquee_autoscroll_axes(&self, pointer: Point<Pixels>) -> (i8, i8) {
        let bounds = self.marquee_viewport_bounds();
        if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
            return (0, 0);
        }

        let edge = px(drag::MARQUEE_EDGE_THRESHOLD);
        let local_x = pointer.x - bounds.origin.x;
        let local_y = pointer.y - bounds.origin.y;
        let vertical = if local_y < edge {
            -1
        } else if local_y > bounds.size.height - edge {
            1
        } else {
            0
        };
        let horizontal = if local_x < edge {
            -1
        } else if local_x > bounds.size.width - edge {
            1
        } else {
            0
        };
        (vertical, horizontal)
    }

    fn marquee_scroll_by(&self, delta: Point<Pixels>) {
        let scroll_handle = &self.files_scroll_handle.0.borrow().base_handle;
        let mut offset = scroll_handle.offset();
        let max_offset = scroll_handle.max_offset();
        offset.x = Self::clamp_pixels(offset.x + delta.x, -max_offset.x, px(0.));
        offset.y = Self::clamp_pixels(offset.y + delta.y, -max_offset.y, px(0.));
        scroll_handle.set_offset(offset);
    }

    fn tick_marquee_autoscroll(&mut self, cx: &mut ViewContext<Self>) {
        let (vertical, horizontal, extend_selection) = {
            let Some(marquee) = self.marquee_drag.as_ref() else {
                return;
            };
            (
                marquee.autoscroll_vertical,
                marquee.autoscroll_horizontal,
                marquee.extend_selection,
            )
        };

        if vertical == 0 && horizontal == 0 {
            return;
        }

        let step = px(drag::MARQUEE_AUTOSCROLL_STEP);
        self.marquee_scroll_by(point(
            step * horizontal as f32,
            step * -vertical as f32,
        ));

        let bounds = self.marquee_viewport_bounds();
        let pointer = {
            let Some(marquee) = self.marquee_drag.as_ref() else {
                return;
            };
            point(
                if horizontal > 0 {
                    bounds.right()
                } else if horizontal < 0 {
                    bounds.left()
                } else {
                    marquee.pointer.x
                },
                if vertical > 0 {
                    bounds.bottom()
                } else if vertical < 0 {
                    bounds.top()
                } else {
                    marquee.pointer.y
                },
            )
        };

        self.update_marquee_drag(pointer, extend_selection, cx);
    }

    fn start_marquee_autoscroll_task(&mut self, cx: &mut ViewContext<Self>) {
        self.marquee_autoscroll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;

                let continue_task = this
                    .update(cx, |this, cx| {
                        if this.marquee_drag.is_none() {
                            return false;
                        }
                        this.tick_marquee_autoscroll(cx);
                        true
                    })
                    .unwrap_or(false);

                if !continue_task {
                    break;
                }
            }
        }));
    }

    fn marquee_list_point(&self, window_point: Point<Pixels>) -> Point<Pixels> {
        let bounds = self.marquee_viewport_bounds();
        let scroll = self.marquee_scroll_offset();
        point(
            window_point.x - bounds.origin.x - scroll.x,
            -scroll.y + (window_point.y - bounds.origin.y),
        )
    }

    fn marquee_list_index_for_list_y(&self, list_y: Pixels, item_count: usize) -> usize {
        if item_count == 0 {
            return 0;
        }

        let item_height = self.marquee_list_row_height();
        if item_height <= px(0.) {
            return 0;
        }

        let index = (list_y / item_height).floor().max(0.0) as usize;
        index.min(item_count - 1)
    }

    fn attach_marquee_handlers(
        &self,
        layer: Stateful<Div>,
        cx: &mut ViewContext<Self>,
    ) -> Stateful<Div> {
        layer
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if event.button == MouseButton::Left {
                    this.begin_marquee_drag(
                        event.position,
                        event.modifiers.control || event.modifiers.platform,
                        window,
                        cx,
                    );
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.marquee_drag.is_some()
                    && event.pressed_button != Some(MouseButton::Left)
                {
                    this.finish_marquee_drag(cx);
                    return;
                }
                if this.marquee_drag.is_some() {
                    let extend_selection = this
                        .marquee_drag
                        .as_ref()
                        .is_some_and(|marquee| marquee.extend_selection);
                    this.update_marquee_drag(event.position, extend_selection, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.finish_marquee_drag(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.finish_marquee_drag(cx);
                }),
            )
    }

    fn register_marquee_window_listeners(&self, window: &mut Window, cx: &mut ViewContext<Self>) {
        let view = cx.entity();
        let view_for_mouse_up = view.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture {
                let _ = view_for_mouse_up.update(cx, |this, cx| {
                    if this.marquee_drag.is_some() {
                        this.finish_marquee_drag(cx);
                    }
                });
            }
        });
        let view_for_mouse_move = view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }

            if event.pressed_button == Some(MouseButton::Left) {
                let _ = view_for_mouse_move.update(cx, |this, cx| {
                    if let Some(extend_selection) =
                        this.marquee_drag.as_ref().map(|marquee| marquee.extend_selection)
                    {
                        this.update_marquee_drag(window.mouse_position(), extend_selection, cx);
                    }
                });
            } else {
                let _ = view_for_mouse_move.update(cx, |this, cx| {
                    if this.marquee_drag.is_some() {
                        this.finish_marquee_drag(cx);
                    }
                });
            }
        });
    }

    fn begin_marquee_drag(
        &mut self,
        origin: Point<Pixels>,
        extend_selection: bool,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        if self.using_subfolder_search() {
            return;
        }

        if !extend_selection {
            self.clear_selection(cx);
        }

        let clamped_origin = self.marquee_clamp_pointer_to_viewport(origin);
        let origin_list = self.marquee_list_point(clamped_origin);
        let (autoscroll_vertical, autoscroll_horizontal) =
            self.marquee_autoscroll_axes(clamped_origin);
        self.marquee_drag = Some(MarqueeDrag {
            origin,
            pointer: clamped_origin,
            origin_list,
            pointer_list: origin_list,
            extend_selection,
            active: false,
            autoscroll_vertical,
            autoscroll_horizontal,
        });
        self.start_marquee_autoscroll_task(cx);
        self.marquee_cancel_subscription = Some(cx.observe_window_activation(
            window,
            |this, _, cx| {
                if this.marquee_drag.is_some() {
                    this.finish_marquee_drag(cx);
                }
            },
        ));
        cx.notify();
    }

    fn update_marquee_drag(
        &mut self,
        pointer: Point<Pixels>,
        extend_selection: bool,
        cx: &mut ViewContext<Self>,
    ) {
        let clamped_pointer = self.marquee_clamp_pointer_to_viewport(pointer);
        let pointer_list = self.marquee_list_point(clamped_pointer);
        let (autoscroll_vertical, autoscroll_horizontal) =
            self.marquee_autoscroll_axes(clamped_pointer);
        let (origin_list, pointer_list, should_apply) = {
            let Some(marquee) = self.marquee_drag.as_mut() else {
                return;
            };

            marquee.pointer = clamped_pointer;
            marquee.pointer_list = pointer_list;
            marquee.autoscroll_vertical = autoscroll_vertical;
            marquee.autoscroll_horizontal = autoscroll_horizontal;
            if !marquee.active
                && drag::marquee_exceeds_threshold(marquee.origin, clamped_pointer)
            {
                marquee.active = true;
            }

            (
                marquee.origin_list,
                marquee.pointer_list,
                marquee.active,
            )
        };

        if should_apply {
            self.apply_marquee_selection(origin_list, pointer_list, extend_selection);
        }
        cx.notify();
    }

    fn finish_marquee_drag(&mut self, cx: &mut ViewContext<Self>) {
        self.marquee_cancel_subscription.take();
        self.marquee_autoscroll_task.take();
        if self.marquee_drag.is_some() {
            self.marquee_drag = None;
            cx.notify();
        }
    }

    fn apply_marquee_selection(
        &mut self,
        origin_list: Point<Pixels>,
        pointer_list: Point<Pixels>,
        extend_selection: bool,
    ) {
        let names = self.visible_entry_names();
        if names.is_empty() {
            return;
        }

        let (Some(start_index), Some(end_index)) =
            self.marquee_index_range(origin_list, pointer_list, names.len())
        else {
            return;
        };

        if !extend_selection {
            self.selected_files.clear();
        }

        for index in start_index..=end_index {
            if let Some(name) = names.get(index) {
                self.selected_files.insert(name.clone());
            }
        }

        self.selection_anchor = Some(start_index);
        self.list_focus_index = Some(end_index);
    }

    fn icon_grid_columns(&self) -> usize {
        const ICON_CELL_WIDTH: f32 = 88.0;
        const ICON_GRID_GAP: f32 = 8.0;
        let panel_width: f32 = self.marquee_viewport_bounds().size.width.into();
        let panel_width = if panel_width > 0.0 {
            panel_width
        } else {
            let total_width = self.config.window.last_window_width.max(800) as f32;
            let sidebar_width = self.config.window.splitter_pos as f32;
            (total_width - sidebar_width - 64.0).max(200.0)
        };
        (panel_width / (ICON_CELL_WIDTH + ICON_GRID_GAP))
            .floor()
            .max(1.0) as usize
    }

    fn marquee_index_range(
        &self,
        origin_list: Point<Pixels>,
        pointer_list: Point<Pixels>,
        item_count: usize,
    ) -> (Option<usize>, Option<usize>) {
        if item_count == 0 {
            return (None, None);
        }

        let clamp_index = |index: usize| index.min(item_count.saturating_sub(1));

        if self.view_mode == ViewMode::Icon {
            let columns = self.icon_grid_columns().max(1);
            let padding = px(8.0);
            let gap = px(8.0);
            let cell_width = px(88.0) + gap;
            let cell_height = self.files_list_item_height() + gap;

            let index_at = |point: Point<Pixels>| -> usize {
                let row = ((point.y - padding) / cell_height).floor().max(0.0) as usize;
                let column = ((point.x - padding) / cell_width).floor().max(0.0) as usize;
                row * columns + column
            };

            let top_left = Point::new(
                origin_list.x.min(pointer_list.x),
                origin_list.y.min(pointer_list.y),
            );
            let bottom_right = Point::new(
                origin_list.x.max(pointer_list.x),
                origin_list.y.max(pointer_list.y),
            );
            return (
                Some(clamp_index(index_at(top_left))),
                Some(clamp_index(index_at(bottom_right))),
            );
        }

        let start_index = clamp_index(self.marquee_list_index_for_list_y(
            origin_list.y.min(pointer_list.y),
            item_count,
        ));
        let end_index = clamp_index(self.marquee_list_index_for_list_y(
            origin_list.y.max(pointer_list.y),
            item_count,
        ));
        (Some(start_index), Some(end_index))
    }

    fn apply_directory_drop_target(
        &self,
        row: Stateful<Div>,
        destination: PathBuf,
        cx: &mut ViewContext<Self>,
    ) -> Stateful<Div> {
        row.drag_over::<DraggedFilePaths>(|style, _, _, cx| drop_target_style(style, cx))
            .drag_over::<ExternalPaths>(|style, _, _, cx| drop_target_style(style, cx))
            .can_drop({
                let destination = destination.clone();
                move |payload, _, _| {
                    payload
                        .downcast_ref::<DraggedFilePaths>()
                        .is_some_and(|dragged| {
                            drag::is_valid_drop_destination(&dragged.paths, &destination)
                        })
                        || payload
                            .downcast_ref::<ExternalPaths>()
                            .is_some_and(|paths| {
                                drag::is_valid_drop_destination(paths.paths(), &destination)
                            })
                }
            })
            .on_drop(cx.listener({
                let destination = destination.clone();
                move |this, dragged: &DraggedFilePaths, _, cx| {
                    this.drop_into_directory(&destination, dragged, cx);
                }
            }))
            .on_drop(cx.listener({
                let destination = destination.clone();
                move |this, paths: &ExternalPaths, _, cx| {
                    this.drop_external_into_directory(&destination, paths, cx);
                }
            }))
    }

    fn request_delete(&mut self, permanent: bool, cx: &mut ViewContext<Self>) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }

        let needs_confirmation = if permanent {
            self.config.behavior.confirm_delete
        } else if self.config.behavior.use_trash {
            self.config.behavior.confirm_trash
        } else {
            self.config.behavior.confirm_delete
        };

        if needs_confirmation {
            self.pending_delete = Some(PendingDelete {
                paths,
                permanent,
                use_trash: self.config.behavior.use_trash && !permanent,
            });
            cx.notify();
            return;
        }

        self.perform_delete(paths, permanent, cx);
    }

    fn perform_delete(&mut self, paths: Vec<PathBuf>, permanent: bool, cx: &mut ViewContext<Self>) {
        let use_trash = self.config.behavior.use_trash && !permanent;
        self.set_status("Deleting…", cx);

        cx.spawn(async move |this, cx| {
            let errors = Tokio::spawn(cx, async move {
                let mut errors = Vec::new();
                for path in paths {
                    let result = if use_trash {
                        move_to_trash(path)
                    } else {
                        delete_path(path)
                    };
                    if let Err(error) = result {
                        errors.push(error);
                    }
                }
                errors
            })
            .await
            .unwrap_or_default();

            let status = if errors.is_empty() {
                "Delete complete".to_string()
            } else {
                errors.join("; ")
            };

            let _ = this.update(cx, |this, cx| {
                this.set_status(status, cx);
                this.reload_current_directory(cx);
            });
        })
        .detach();
    }

    fn delete_selected(&mut self, cx: &mut ViewContext<Self>) {
        self.request_delete(false, cx);
    }

    fn create_folder(&mut self, cx: &mut ViewContext<Self>) {
        let destination = unique_name_in_parent(&self.current_path, "New Folder");
        match create_directory(destination) {
            Ok(()) => {
                self.set_status("Created folder", cx);
                self.reload_current_directory(cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    fn create_file(&mut self, cx: &mut ViewContext<Self>) {
        let destination = unique_name_in_parent(&self.current_path, "New File");
        match create_file(destination) {
            Ok(()) => {
                self.set_status("Created file", cx);
                self.reload_current_directory(cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    fn copy_selected(&mut self, cx: &mut ViewContext<Self>) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.set_status("Nothing selected to copy", cx);
            return;
        }
        self.clipboard.set_files(paths.clone(), false);
        cx.write_to_clipboard(ClipboardItem::new_file_paths(paths, false));
        self.set_status("Copied to clipboard", cx);
    }

    fn cut_selected(&mut self, cx: &mut ViewContext<Self>) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.set_status("Nothing selected to cut", cx);
            return;
        }
        self.clipboard.set_files(paths.clone(), true);
        cx.write_to_clipboard(ClipboardItem::new_file_paths(paths, true));
        self.set_status("Cut to clipboard", cx);
    }

    fn paste_clipboard(&mut self, cx: &mut ViewContext<Self>) {
        let job = cx
            .read_from_clipboard()
            .and_then(|clipboard| clipboard.file_paths())
            .or_else(|| self.clipboard.take_files());

        let Some((sources, is_cut)) = job else {
            self.set_status("Clipboard is empty", cx);
            return;
        };

        let destination_directory = self.current_path.clone();
        let conflict_count = count_paste_conflicts(&sources, &destination_directory);
        if conflict_count > 0 {
            self.pending_paste_choice = Some(PendingPasteChoice {
                sources,
                destination_directory,
                is_cut,
                conflict_count,
            });
            cx.notify();
            return;
        }

        self.execute_paste(sources, destination_directory, is_cut, PasteJobSettings::default(), cx);
    }

    fn execute_paste(
        &mut self,
        sources: Vec<PathBuf>,
        destination_directory: PathBuf,
        is_cut: bool,
        settings: PasteJobSettings,
        cx: &mut ViewContext<Self>,
    ) {
        let action_label = if is_cut { "Moving" } else { "Copying" };
        let cancel = Arc::new(AtomicBool::new(false));
        self.paste_cancel = Some(cancel.clone());
        self.set_status(format!("{action_label} {} items…", sources.len()), cx);
        cx.notify();

        cx.spawn(async move |this, cx| {
            let total = sources.len() as u32;
            let mut combined = PasteResult::default();
            let paste_destination = destination_directory.clone();
            for (index, source) in sources.into_iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    combined.cancelled = true;
                    break;
                }

                let current = index as u32 + 1;
                let file_name = source
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("…")
                    .to_string();
                let _ = this.update(cx, |this, cx| {
                    this.set_status(
                        format!("{action_label} {current}/{total}: {file_name}"),
                        cx,
                    );
                });
                let paste_destination = paste_destination.clone();
                let cancel_flag = cancel.clone();
                let partial = Tokio::spawn(cx, async move {
                    run_paste_batch(
                        vec![source],
                        paste_destination,
                        is_cut,
                        settings,
                        Some(cancel_flag),
                    )
                })
                .await
                .unwrap_or_default();
                combined.errors.extend(partial.errors);
                combined.recorded_moves.extend(partial.recorded_moves);
                if partial.cancelled {
                    combined.cancelled = true;
                    break;
                }
            }

            let status = if combined.cancelled {
                if combined.errors.is_empty() {
                    "Paste cancelled".to_string()
                } else {
                    format!("Paste cancelled; {}", combined.errors.join("; "))
                }
            } else if combined.errors.is_empty() {
                format!("{action_label} complete")
            } else {
                combined.errors.join("; ")
            };

            let _ = this.update(cx, |this, cx| {
                this.paste_cancel = None;
                for (source, destination) in combined.recorded_moves {
                    this.undo_stack.push_move(source, destination);
                }
                this.set_status(status, cx);
                this.reload_current_directory(cx);
            });
        })
        .detach();
    }

    fn cancel_active_paste(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(cancel) = &self.paste_cancel {
            cancel.store(true, Ordering::Relaxed);
            self.set_status("Cancelling paste…", cx);
            cx.notify();
        }
    }

    fn confirm_paste_with_resolution(
        &mut self,
        resolution: ConflictResolution,
        cx: &mut ViewContext<Self>,
    ) {
        let Some(pending) = self.pending_paste_choice.take() else {
            return;
        };
        let settings = PasteJobSettings { conflict: resolution };
        self.execute_paste(
            pending.sources,
            pending.destination_directory,
            pending.is_cut,
            settings,
            cx,
        );
        cx.notify();
    }

    fn cancel_pending_paste(&mut self, cx: &mut ViewContext<Self>) {
        self.pending_paste_choice = None;
        self.set_status("Paste cancelled", cx);
        cx.notify();
    }

    fn go_to_parent_of_selection(&mut self, cx: &mut ViewContext<Self>) {
        let paths = self.selected_paths();
        if paths.len() != 1 {
            self.set_status("Select a single item to open its parent folder", cx);
            return;
        }
        let Some(parent) = paths[0].parent() else {
            self.set_status("Item has no parent folder", cx);
            return;
        };
        self.navigate_to(parent.to_path_buf(), true, cx);
    }

    fn invert_selection(&mut self, cx: &mut ViewContext<Self>) {
        if self.using_subfolder_search() {
            let keys: Vec<String> = self
                .search_matches
                .iter()
                .map(|search_match| Self::selection_key_for_path(&search_match.path))
                .collect();
            for key in keys {
                if self.selected_files.contains(&key) {
                    self.selected_files.remove(&key);
                } else {
                    self.selected_files.insert(key);
                }
            }
        } else {
            let visible_names: Vec<String> = self
                .visible_files()
                .into_iter()
                .filter_map(|file_info| file_info.get_name().map(str::to_string))
                .collect();
            for name in visible_names {
                if self.selected_files.contains(&name) {
                    self.selected_files.remove(&name);
                } else {
                    self.selected_files.insert(name);
                }
            }
        }
        cx.notify();
    }

    fn reload_volume_mounts(&mut self) {
        self.volume_mounts = devices::list_removable_mounts();
    }

    fn start_volume_monitor(&mut self, cx: &mut ViewContext<Self>) {
        let (mounts_sender, mounts_receiver) = mpsc::channel();

        cx.spawn(async move |_, cx| {
            Tokio::spawn(cx, async move {
                devices::run_volume_monitor_loop(|mounts| {
                    let _ = mounts_sender.send(mounts);
                })
                .await;
            })
            .detach();
        })
        .detach();

        cx.spawn(async move |this, cx| {
            loop {
                while let Ok(mounts) = mounts_receiver.try_recv() {
                    let _ = this.update(cx, |this, cx| {
                        this.volume_mounts = mounts;
                        this.queue_ui_icon_loads(cx);
                        cx.notify();
                    });
                }

                cx.background_executor()
                    .timer(Duration::from_millis(300))
                    .await;
            }
        })
        .detach();
    }

    fn handle_path_input_event(
        &mut self,
        event: ToolbarLineInputEvent,
        cx: &mut ViewContext<Self>,
    ) {
        match event {
            ToolbarLineInputEvent::Changed(text) => {
                self.path_input_text = text;
                cx.notify();
            }
            ToolbarLineInputEvent::Submit => self.submit_path_bar(cx),
            ToolbarLineInputEvent::Cancel => {
                self.path_edit_active = false;
                self.sync_path_line_input_from_current(cx);
                self.set_status("Ready", cx);
            }
        }
    }

    fn handle_search_input_event(
        &mut self,
        event: ToolbarLineInputEvent,
        cx: &mut ViewContext<Self>,
    ) {
        match event {
            ToolbarLineInputEvent::Changed(text) => {
                self.search_query = text;
                self.schedule_subfolder_search(cx);
            }
            ToolbarLineInputEvent::Submit => {
                self.record_search_history();
                cx.notify();
            }
            ToolbarLineInputEvent::Cancel => self.clear_search(cx),
        }
    }

    fn sync_path_line_input_from_current(&mut self, cx: &mut ViewContext<Self>) {
        self.path_input_text = self.current_path.to_string_lossy().to_string();
        self.path_line_input.update(cx, |input, cx| {
            input.set_text(self.path_input_text.clone(), cx);
        });
    }

    fn focus_path_line_input(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
        self.path_line_input.update(cx, |input, cx| {
            input.set_text(self.path_input_text.clone(), cx);
        });
        let focus_handle = self.path_line_input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    }

    fn focus_search_line_input(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
        self.search_line_input.update(cx, |input, cx| {
            input.set_text(self.search_query.clone(), cx);
        });
        let focus_handle = self.search_line_input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    }

    fn set_icon_size(&mut self, size: u32, cx: &mut ViewContext<Self>) {
        self.icon_size = clamp_icon_size(size);
        self.config.folder_view.icon_size = Some(self.icon_size);
        self.config.save();
        self.queue_icon_loads(cx);
        cx.notify();
    }

    fn zoom_icons_in(&mut self, cx: &mut ViewContext<Self>) {
        let next = self.icon_size.saturating_add(ICON_ZOOM_STEP);
        self.set_icon_size(next, cx);
        self.set_status(format!("Icon size: {} px", self.icon_size), cx);
    }

    fn zoom_icons_out(&mut self, cx: &mut ViewContext<Self>) {
        let next = self.icon_size.saturating_sub(ICON_ZOOM_STEP);
        self.set_icon_size(next, cx);
        self.set_status(format!("Icon size: {} px", self.icon_size), cx);
    }

    fn zoom_icons_reset(&mut self, cx: &mut ViewContext<Self>) {
        self.config.folder_view.icon_size = None;
        self.icon_size = self.view_mode.default_icon_size();
        self.config.save();
        self.queue_icon_loads(cx);
        self.set_status(
            format!("Icon size reset to {} px", self.icon_size),
            cx,
        );
        cx.notify();
    }

    fn open_about(&mut self, cx: &mut ViewContext<Self>) {
        self.show_about = true;
        cx.notify();
    }

    fn dismiss_about(&mut self, cx: &mut ViewContext<Self>) {
        self.show_about = false;
        cx.notify();
    }

    fn open_settings(&mut self, cx: &mut ViewContext<Self>) {
        self.pending_settings = Some(SettingsDraft::from_config(&self.config));
        self.settings_terminal_focus = false;
        cx.notify();
    }

    fn dismiss_settings(&mut self, cx: &mut ViewContext<Self>) {
        self.pending_settings = None;
        self.settings_terminal_focus = false;
        cx.notify();
    }

    fn confirm_settings(&mut self, cx: &mut ViewContext<Self>) {
        let Some(draft) = self.pending_settings.take() else {
            return;
        };
        draft.apply_to(&mut self.config);
        self.config.save();
        self.show_hidden = self.config.folder_view.show_hidden;
        self.settings_terminal_focus = false;
        self.icon_size = self.config.icon_size_for_mode(self.view_mode);
        self.queue_icon_loads(cx);
        self.set_status("Settings saved", cx);
        cx.notify();
    }

    fn toggle_settings_field(&mut self, field: SettingsField, cx: &mut ViewContext<Self>) {
        let Some(draft) = self.pending_settings.as_mut() else {
            return;
        };
        draft.toggle(field);
        cx.notify();
    }

    fn focus_settings_terminal(&mut self, cx: &mut ViewContext<Self>) {
        if self.pending_settings.is_some() {
            self.settings_terminal_focus = true;
            cx.notify();
        }
    }

    fn handle_settings_dialog_key(&mut self, event: &KeyDownEvent, cx: &mut ViewContext<Self>) {
        if !self.settings_terminal_focus {
            if event.keystroke.key == "escape" {
                self.dismiss_settings(cx);
            }
            return;
        }

        let Some(draft) = self.pending_settings.as_mut() else {
            return;
        };

        if event.keystroke.key == "escape" {
            self.settings_terminal_focus = false;
            cx.notify();
            return;
        }

        if event.keystroke.key == "backspace" {
            draft.terminal_command.pop();
            cx.notify();
            return;
        }

        if let Some(character) = event.keystroke.key_char.as_ref() {
            if character.len() == 1
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.platform
            {
                draft.terminal_command.push_str(character);
                cx.notify();
            }
        }
    }

    fn undo_last(&mut self, cx: &mut ViewContext<Self>) {
        match self.undo_stack.undo_one() {
            Ok(()) => {
                self.set_status("Undone", cx);
                self.reload_current_directory(cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    fn redo_last(&mut self, cx: &mut ViewContext<Self>) {
        match self.undo_stack.redo_one() {
            Ok(()) => {
                self.set_status("Redone", cx);
                self.reload_current_directory(cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    fn open_terminal_here(&mut self, cx: &mut ViewContext<Self>) {
        let directory = self.current_path.clone();
        let terminal_command = self.config.terminal_command().map(str::to_string);
        match terminal::open_terminal_in_directory(&directory, terminal_command.as_deref()) {
            Ok(()) => self.set_status("Opened terminal", cx),
            Err(error) => self.set_status(error, cx),
        }
    }

    fn show_properties_for_selection(&mut self, cx: &mut ViewContext<Self>) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.set_status("Nothing selected for properties", cx);
            return;
        }
        let mut dialog = match properties::properties_for_paths(&paths) {
            Some(dialog) => dialog,
            None => {
                self.set_status("Could not read properties", cx);
                return;
            }
        };

        if paths.len() == 1 {
            let path = &paths[0];
            if let Some(icon) =
                self.icon_cache
                    .cached_icon(path, properties::PROPERTIES_ICON_SIZE)
            {
                dialog.icon = Some(icon);
            }
        }

        self.pending_properties = Some(dialog);
        cx.notify();

        if paths.len() != 1 {
            return;
        }

        let path = paths[0].clone();
        let file_type = self.file_type_for_path(&path);
        let Some(icon_service) = nptk::file_icons::FileIconService::global(cx).cloned() else {
            return;
        };

        cx.spawn(async move |this, cx| {
            let path_for_kind = path.clone();
            let kind_row = Tokio::spawn(cx, async move {
                properties::mime_kind_row(&path_for_kind).await
            })
            .await
            .ok()
            .flatten();

            if let Some(kind_row) = kind_row {
                let _ = this.update(cx, |this, cx| {
                    if let Some(dialog) = this.pending_properties.as_mut() {
                        properties::insert_kind_row(dialog, kind_row);
                        cx.notify();
                    }
                });
            }

            let icon_size = properties::PROPERTIES_ICON_SIZE;
            let path_for_icon = path.clone();
            let presentation = Tokio::spawn(cx, async move {
                icons::FileIconCache::load_icon(
                    &icon_service,
                    path_for_icon,
                    icon_size,
                    file_type,
                    true,
                )
                .await
            })
            .await
            .ok()
            .flatten();

            let Some(presentation) = presentation else {
                return;
            };

            let path_for_cache = path.clone();
            let _ = this.update(cx, |this, cx| {
                if let Some(dialog) = this.pending_properties.as_mut() {
                    if dialog.icon.is_none() {
                        dialog.icon = Some(presentation.clone());
                    }
                    this.icon_cache
                        .store_icon(path_for_cache, icon_size, presentation);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn file_type_for_path(&self, path: &Path) -> FileType {
        if let Some(name) = path.file_name().and_then(|segment| segment.to_str()) {
            if let Some(file) = self.files.iter().find(|file| file.get_name() == Some(name)) {
                if self.current_path.join(name) == path {
                    return file.get_file_type();
                }
            }
        }

        let Ok(metadata) = nptk::std::fs::symlink_metadata(path) else {
            return FileType::Regular;
        };
        if metadata.is_dir() {
            FileType::Directory
        } else if metadata.file_type().is_symlink() {
            FileType::SymbolicLink
        } else {
            FileType::Regular
        }
    }

    fn dismiss_properties(&mut self, cx: &mut ViewContext<Self>) {
        self.pending_properties = None;
        cx.notify();
    }

    fn dismiss_context_menu(&mut self) {
        self.context_menu = None;
    }

    fn deploy_context_menu(
        &mut self,
        position: Point<Pixels>,
        target: ContextMenuTarget,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        self.dismiss_context_menu();
        let focus_handle = self.focus_handle.clone();
        let has_selection = !self.selected_files.is_empty();
        let selected_paths = self.selected_paths();
        let open_action_label = open::open_label_for_path(
            selected_paths
                .first()
                .map(|path| path.as_path())
                .unwrap_or(Path::new("")),
        );
        let open_with_handlers = selected_paths
            .first()
            .map(|path| open::handlers_for_path(path))
            .unwrap_or_default();
        let window_weak = cx.weak_entity();

        let context_menu = ContextMenu::build(window, cx, move |menu, _, _| {
            let mut menu = menu.context(focus_handle.clone());
            match target {
                ContextMenuTarget::Background => {
                    menu = menu
                        .action("New Folder", CreateFolder.boxed_clone())
                        .action("New File", CreateFile.boxed_clone())
                        .separator()
                        .action("Paste", Paste.boxed_clone())
                        .action("Refresh", Refresh.boxed_clone())
                        .action("Select All", SelectAll.boxed_clone())
                        .separator()
                        .action("Open Terminal Here", OpenTerminal.boxed_clone());
                }
                ContextMenuTarget::FileList => {
                    menu = menu.action(open_action_label.as_str(), OpenSelection.boxed_clone());
                    if !open_with_handlers.is_empty() {
                        let handlers = open_with_handlers.clone();
                        let paths_for_open = selected_paths.clone();
                        let weak = window_weak.clone();
                        menu = menu.submenu("Open With", move |submenu, _, _| {
                            let mut submenu = submenu;
                            for handler in &handlers {
                                let application_id = handler.app_id.clone();
                                let label = handler.label.clone();
                                let path = paths_for_open
                                    .first()
                                    .cloned()
                                    .unwrap_or_default();
                                let weak = weak.clone();
                                submenu = submenu.entry(label, None, move |_, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.launch_file_with_application(
                                            application_id.clone(),
                                            path.clone(),
                                            cx,
                                        );
                                    });
                                });
                            }
                            submenu
                        });
                    }
                    menu = menu
                        .separator()
                        .action("Cut", Cut.boxed_clone())
                        .action("Copy", Copy.boxed_clone())
                        .action("Paste", Paste.boxed_clone())
                        .action("Duplicate", Duplicate.boxed_clone())
                        .separator()
                        .action("Rename", Rename.boxed_clone())
                        .action("Move to Trash", DeleteSelected.boxed_clone())
                        .action("Delete Permanently", DeletePermanent.boxed_clone())
                        .action("Properties", ShowProperties.boxed_clone())
                        .action("Go to Parent Folder", GoToParent.boxed_clone());
                    if has_selection {
                        menu = menu.separator().action(
                            "Open Terminal Here",
                            OpenTerminal.boxed_clone(),
                        );
                    }
                }
            }
            menu
        });

        window.focus(&context_menu.focus_handle(cx), cx);
        let subscription = cx.subscribe_in(
            &context_menu,
            window,
            |this, _, _: &DismissEvent, window, cx| {
                if this.context_menu.as_ref().is_some_and(|(menu, _, _)| {
                    menu.focus_handle(cx).contains_focused(window, cx)
                }) {
                    window.focus(&this.focus_handle, cx);
                }
                this.dismiss_context_menu();
                cx.notify();
            },
        );

        self.context_menu = Some((context_menu, position, subscription));
    }

    fn open_selection_with_system(&mut self, cx: &mut ViewContext<Self>) {
        let paths = self.selected_paths();
        if paths.len() != 1 {
            self.set_status("Select a single item to open with the system handler", cx);
            return;
        }
        let path = paths[0].clone();
        if path.is_dir() {
            self.navigate_to(path, true, cx);
        } else {
            cx.open_with_system(&path);
            self.set_status(format!("Opened {} with the system default", path.display()), cx);
        }
    }

    fn open_primary_selection(&mut self, cx: &mut ViewContext<Self>) {
        let paths = self.selected_paths();
        if paths.len() != 1 {
            self.set_status("Select a single item to open", cx);
            return;
        }
        let path = paths[0].clone();
        if path.is_dir() {
            self.navigate_to(path, true, cx);
        } else {
            self.launch_file(&path, cx);
        }
    }

    fn prepare_context_selection(&mut self, file_name: &str, cx: &mut ViewContext<Self>) {
        if !self.selected_files.contains(file_name) {
            self.selected_files.clear();
            self.selected_files.insert(file_name.to_string());
            cx.notify();
        }
    }

    fn duplicate_selected(&mut self, cx: &mut ViewContext<Self>) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }

        self.set_status("Duplicating…", cx);
        cx.spawn(async move |this, cx| {
            let errors = Tokio::spawn(cx, async move {
                let mut errors = Vec::new();
                for path in paths {
                    if let Err(error) = duplicate_path(path) {
                        errors.push(error);
                    }
                }
                errors
            })
            .await
            .unwrap_or_default();

            let status = if errors.is_empty() {
                "Duplicate complete".to_string()
            } else {
                errors.join("; ")
            };

            let _ = this.update(cx, |this, cx| {
                this.set_status(status, cx);
                this.reload_current_directory(cx);
            });
        })
        .detach();
    }

    fn clear_selection(&mut self, cx: &mut ViewContext<Self>) {
        if self.search_active {
            self.search_active = false;
            self.search_query.clear();
        }
        if self.path_edit_active {
            self.path_edit_active = false;
            self.path_input_text = self.current_path.to_string_lossy().to_string();
        }
        if self.pending_delete.is_some() {
            self.pending_delete = None;
        }
        if self.pending_rename.is_some() {
            self.pending_rename = None;
        }
        if self.pending_properties.is_some() {
            self.pending_properties = None;
        }
        if self.pending_settings.is_some() {
            self.pending_settings = None;
            self.settings_terminal_focus = false;
        }
        if self.pending_paste_choice.is_some() {
            self.pending_paste_choice = None;
        }
        if self.show_about {
            self.show_about = false;
        }
        self.dismiss_context_menu();
        self.selected_files.clear();
        self.selection_anchor = None;
        self.list_focus_index = None;
        cx.notify();
    }

    fn record_search_history(&mut self) {
        const MAX_SEARCH_HISTORY: usize = 10;
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.search_history.retain(|entry| entry != &query);
        self.search_history.insert(0, query);
        self.search_history.truncate(MAX_SEARCH_HISTORY);
    }

    fn activate_search(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
        self.path_edit_active = false;
        self.search_active = true;
        self.search_line_input.update(cx, |input, cx| {
            input.set_text(self.search_query.clone(), cx);
        });
        self.focus_search_line_input(window, cx);
        self.set_status("Search: type to filter, Enter/Escape to finish", cx);
        cx.notify();
    }

    fn clear_search(&mut self, cx: &mut ViewContext<Self>) {
        self.search_active = false;
        self.search_query.clear();
        self.search_matches.clear();
        self.search_in_progress = false;
        self.search_line_input.update(cx, |input, cx| {
            input.set_text("", cx);
        });
        self.set_status("Ready", cx);
        cx.notify();
    }

    fn focus_path_bar(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
        self.search_active = false;
        self.search_query.clear();
        self.path_edit_active = true;
        self.sync_path_line_input_from_current(cx);
        self.focus_path_line_input(window, cx);
        self.set_status("Path: edit and press Enter to go, Escape to cancel", cx);
        cx.notify();
    }

    fn submit_path_bar(&mut self, cx: &mut ViewContext<Self>) {
        let path = PathBuf::from(self.path_input_text.trim());
        self.path_edit_active = false;
        if path.is_dir() {
            self.navigate_to(path, true, cx);
        } else {
            self.set_status("Path is not a directory", cx);
            self.sync_path_line_input_from_current(cx);
            cx.notify();
        }
    }

    fn go_home(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(home) = dirs::home_dir() {
            self.navigate_to(home, true, cx);
        }
    }

    fn new_tab(&mut self, cx: &mut ViewContext<Self>) {
        let path = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        self.tabs.new_tab(path.clone());
        self.navigate_to(path, false, cx);
    }

    fn close_tab(&mut self, cx: &mut ViewContext<Self>) {
        self.close_tab_at(self.tabs.active, cx);
    }

    fn close_tab_at(&mut self, index: usize, cx: &mut ViewContext<Self>) {
        if !self.tabs.close_at(index) {
            self.set_status("Cannot close the last tab", cx);
            return;
        }
        if let Some(path) = self.tabs.active_path() {
            self.navigate_to(path, false, cx);
        }
    }

    fn switch_tab(&mut self, index: usize, cx: &mut ViewContext<Self>) {
        if self.tabs.set_active(index) {
            if let Some(path) = self.tabs.active_path() {
                self.navigate_to(path, false, cx);
            }
        }
    }

    fn add_bookmark_for_current(&mut self, cx: &mut ViewContext<Self>) {
        match bookmarks::add_bookmark(&self.current_path) {
            Ok(()) => {
                self.bookmark_paths = bookmarks::load_bookmarks();
                self.queue_ui_icon_loads(cx);
                self.set_status("Bookmark added", cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    fn remove_bookmark_for_current(&mut self, cx: &mut ViewContext<Self>) {
        match bookmarks::remove_bookmark(&self.current_path) {
            Ok(()) => {
                self.bookmark_paths = bookmarks::load_bookmarks();
                self.queue_ui_icon_loads(cx);
                self.set_status("Bookmark removed", cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    fn apply_sort(&mut self, column: SortColumn, order: Option<SortOrder>, cx: &mut ViewContext<Self>) {
        self.sort_column = column;
        if let Some(order) = order {
            self.sort_order = order;
        }
        self.config.folder_view.sort_column = match column {
            SortColumn::Name => "name".to_string(),
            SortColumn::Size => "size".to_string(),
            SortColumn::Type => "type".to_string(),
            SortColumn::Modified => "modified".to_string(),
        };
        self.config.folder_view.sort_order = match self.sort_order {
            SortOrder::Ascending => "ascending".to_string(),
            SortOrder::Descending => "descending".to_string(),
        };
        self.config.save();
        sort::sort_files(&mut self.files, self.sort_column, self.sort_order);
        self.set_status(format!("Sorted by {:?} ({:?})", self.sort_column, self.sort_order), cx);
        cx.notify();
    }

    fn toggle_sort_order(&mut self, cx: &mut ViewContext<Self>) {
        self.sort_order = match self.sort_order {
            SortOrder::Ascending => SortOrder::Descending,
            SortOrder::Descending => SortOrder::Ascending,
        };
        self.apply_sort(self.sort_column, Some(self.sort_order), cx);
    }

    fn set_view_mode(&mut self, mode: ViewMode, cx: &mut ViewContext<Self>) {
        self.view_mode = mode;
        self.icon_size = self.config.icon_size_for_mode(mode);
        self.config.folder_view.mode = mode.config_value().to_string();
        self.config.save();
        self.set_status(format!("View: {}", mode.menu_label()), cx);
        self.queue_icon_loads(cx);
        cx.notify();
    }

    fn queue_icon_loads(&mut self, cx: &mut ViewContext<Self>) {
        let icon_size = self.icon_size;
        let mut pending: Vec<(PathBuf, FileType)> = Vec::new();

        for file_info in &self.files {
            let Some(name) = file_info.get_name() else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let path = self.current_path.join(name);
            if self.icon_cache.cached_icon(&path, icon_size).is_some() {
                continue;
            }
            pending.push((path, file_info.get_file_type()));
        }

        if pending.is_empty() {
            return;
        }

        let use_thumbnails = matches!(self.view_mode, ViewMode::Icon | ViewMode::Compact);

        let Some(icon_service) = nptk::file_icons::FileIconService::global(cx).cloned() else {
            return;
        };

        cx.spawn(async move |this, cx| {
            for (path, file_type) in pending {
                let path_for_load = path.clone();
                let icon_service = icon_service.clone();
                let image = Tokio::spawn(cx, async move {
                    icons::FileIconCache::load_icon(
                        &icon_service,
                        path_for_load,
                        icon_size,
                        file_type,
                        use_thumbnails,
                    )
                    .await
                })
                .await
                .ok()
                .flatten();

                let Some(icon) = image else {
                    continue;
                };

                let _ = this.update(cx, |this, cx| {
                    this.icon_cache.store_icon(path, icon_size, icon);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn queue_ui_icon_loads(&mut self, cx: &mut ViewContext<Self>) {
        let Some(icon_service) = nptk::file_icons::FileIconService::global(cx).cloned() else {
            return;
        };

        let mut theme_names: HashSet<String> = ui_icons::TOOLBAR_THEME_ICONS
            .iter()
            .map(|name| (*name).to_string())
            .collect();

        for (label, _) in quick_access_places() {
            if let Some(name) = ui_icons::quick_access_theme_icon(label) {
                theme_names.insert(name.to_string());
            }
        }

        let mut paths: Vec<PathBuf> = quick_access_places()
            .into_iter()
            .map(|(_, path)| path)
            .collect();
        paths.extend(self.volume_mounts.iter().map(|mount| mount.mount_point.clone()));
        paths.extend(self.bookmark_paths.iter().cloned());

        let toolbar_size = ui_icons::TOOLBAR_ICON_PIXELS;
        let sidebar_size = ui_icons::SIDEBAR_ICON_PIXELS;

        cx.spawn(async move |this, cx| {
            for icon_name in theme_names {
                if this
                    .read_with(cx, |this, _| {
                        this.icon_cache
                            .cached_theme_icon(&icon_name, toolbar_size)
                            .is_some()
                    })
                    .unwrap_or(false)
                {
                    continue;
                }

                let icon_service = icon_service.clone();
                let icon_name_for_load = icon_name.clone();
                let presentation = Tokio::spawn(cx, async move {
                    icons::FileIconCache::load_theme_icon(
                        &icon_service,
                        &icon_name_for_load,
                        toolbar_size,
                    )
                    .await
                })
                .await
                .ok()
                .flatten();

                let Some(presentation) = presentation else {
                    continue;
                };

                let _ = this.update(cx, |this, cx| {
                    this.icon_cache.store_theme_icon(icon_name, toolbar_size, presentation);
                    cx.notify();
                });
            }

            for path in paths {
                if this
                    .read_with(cx, |this, _| {
                        this.icon_cache.cached_icon(&path, sidebar_size).is_some()
                    })
                    .unwrap_or(false)
                {
                    continue;
                }

                let icon_service = icon_service.clone();
                let path_for_load = path.clone();
                let presentation = Tokio::spawn(cx, async move {
                    icons::FileIconCache::load_path_icon(
                        &icon_service,
                        path_for_load,
                        sidebar_size,
                    )
                    .await
                })
                .await
                .ok()
                .flatten();

                let Some(presentation) = presentation else {
                    continue;
                };

                let _ = this.update(cx, |this, cx| {
                    this.icon_cache
                        .store_icon(path, sidebar_size, presentation);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn start_rename_selected(&mut self, cx: &mut ViewContext<Self>) {
        let Some(name) = self.selected_files.iter().next().cloned() else {
            self.set_status("Select a single item to rename", cx);
            return;
        };

        if self.selected_files.len() != 1 {
            self.set_status("Select a single item to rename", cx);
            return;
        }

        let path = self.current_path.join(&name);
        self.pending_rename = Some(PendingRename {
            path,
            new_name: name,
        });
        cx.notify();
    }

    fn cancel_pending_rename(&mut self, cx: &mut ViewContext<Self>) {
        self.pending_rename = None;
        cx.notify();
    }

    fn confirm_pending_rename(&mut self, cx: &mut ViewContext<Self>) {
        let Some(pending) = self.pending_rename.take() else {
            return;
        };

        let new_name = pending.new_name.trim();
        if new_name.is_empty() {
            self.set_status("Name cannot be empty", cx);
            self.pending_rename = Some(pending);
            return;
        }
        if new_name.contains('/') || new_name.contains('\\') {
            self.set_status("Name cannot contain path separators", cx);
            self.pending_rename = Some(pending);
            return;
        }

        let Some(parent) = pending.path.parent() else {
            self.set_status("Invalid path", cx);
            return;
        };

        let destination = parent.join(new_name);
        match rename_path(pending.path, destination) {
            Ok(()) => {
                self.set_status("Renamed item", cx);
                self.reload_current_directory(cx);
            }
            Err(error) => self.set_status(error, cx),
        }
        cx.notify();
    }

    fn handle_rename_dialog_key(&mut self, event: &KeyDownEvent, cx: &mut ViewContext<Self>) {
        let Some(pending) = self.pending_rename.as_mut() else {
            return;
        };

        if event.keystroke.key == "escape" {
            self.pending_rename = None;
            cx.notify();
            return;
        }

        if event.keystroke.key == "enter" {
            self.confirm_pending_rename(cx);
            return;
        }

        if event.keystroke.key == "backspace" {
            pending.new_name.pop();
            cx.notify();
            return;
        }

        if let Some(character) = event.keystroke.key_char.as_ref() {
            if character.len() == 1
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.platform
            {
                pending.new_name.push_str(character);
                cx.notify();
            }
        }
    }

    fn handle_toolbar_input_key(&mut self, event: &KeyDownEvent, cx: &mut ViewContext<Self>) {
        if self.pending_rename.is_some() {
            self.handle_rename_dialog_key(event, cx);
            return;
        }

        if self.pending_settings.is_some() {
            self.handle_settings_dialog_key(event, cx);
            return;
        }

        if self.path_edit_active || self.search_active {
            return;
        }

        if self.paste_cancel.is_some() && event.keystroke.key == "escape" {
            self.cancel_active_paste(cx);
            return;
        }

        if self.pending_delete.is_some()
            || self.pending_properties.is_some()
            || self.pending_paste_choice.is_some()
            || self.show_about
        {
            if event.keystroke.key == "escape" {
                if self.pending_delete.is_some() {
                    self.cancel_pending_delete(cx);
                } else if self.pending_properties.is_some() {
                    self.pending_properties = None;
                    cx.notify();
                } else if self.pending_paste_choice.is_some() {
                    self.cancel_pending_paste(cx);
                } else if self.show_about {
                    self.show_about = false;
                    cx.notify();
                }
            }
            return;
        }

        self.handle_file_list_key(event, cx);
    }

    fn handle_file_list_key(&mut self, event: &KeyDownEvent, cx: &mut ViewContext<Self>) {
        let key = event.keystroke.key.as_str();
        let shift = event.keystroke.modifiers.shift;
        let control = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;

        match key {
            "up" | "arrowup" => self.move_list_focus(-1, shift, cx),
            "down" | "arrowdown" => self.move_list_focus(1, shift, cx),
            "home" => {
                let names = self.visible_entry_names();
                if names.is_empty() {
                    return;
                }
                if shift {
                    self.select_visible_range(self.selection_anchor.unwrap_or(0), 0);
                } else {
                    self.selected_files.clear();
                    self.selected_files.insert(names[0].clone());
                    self.selection_anchor = Some(0);
                }
                self.list_focus_index = Some(0);
                self.files_scroll_handle
                    .scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
            "end" => {
                let names = self.visible_entry_names();
                if names.is_empty() {
                    return;
                }
                let last = names.len() - 1;
                if shift {
                    self.select_visible_range(self.selection_anchor.unwrap_or(0), last);
                } else {
                    self.selected_files.clear();
                    self.selected_files.insert(names[last].clone());
                    self.selection_anchor = Some(last);
                }
                self.list_focus_index = Some(last);
                self.files_scroll_handle
                    .scroll_to_item(last, ScrollStrategy::Bottom);
                cx.notify();
            }
            "enter" => self.open_focused_or_selection(cx),
            "space" if !control => self.open_focused_or_selection(cx),
            _ => {}
        }
    }


    fn visible_files(&self) -> Vec<&FileInfo> {
        let search = self.search_query.to_ascii_lowercase();
        self.files
            .iter()
            .filter(|file_info| {
                let name = file_info.get_name().unwrap_or("");
                if name.is_empty() {
                    return false;
                }
                if !self.show_hidden && name.starts_with('.') {
                    return false;
                }
                if !search.is_empty() && !name.to_ascii_lowercase().contains(&search) {
                    return false;
                }
                true
            })
            .collect()
    }

    fn cancel_pending_delete(&mut self, cx: &mut ViewContext<Self>) {
        self.pending_delete = None;
        cx.notify();
    }

    fn confirm_pending_delete(&mut self, cx: &mut ViewContext<Self>) {
        let Some(pending) = self.pending_delete.take() else {
            return;
        };
        self.perform_delete(pending.paths, pending.permanent, cx);
        cx.notify();
    }
}

impl Render for FilemanWindow {
    fn render(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();

        if self.marquee_drag.is_some() {
            self.register_marquee_window_listeners(window, cx);
        }

        div()
            .id("fileman-root")
            .relative()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_this, _action: &Quit, _, cx| cx.quit()))
            .on_action(cx.listener(|this, _action: &GoBack, _, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _action: &GoForward, _, cx| this.go_forward(cx)))
            .on_action(cx.listener(|this, _action: &GoUp, _, cx| this.go_up(cx)))
            .on_action(cx.listener(|this, _action: &ToggleHidden, _, cx| {
                this.toggle_hidden(cx)
            }))
            .on_action(cx.listener(|this, _action: &DeleteSelected, _, cx| {
                this.delete_selected(cx)
            }))
            .on_action(cx.listener(|this, _action: &DeletePermanent, _, cx| {
                this.request_delete(true, cx)
            }))
            .on_action(cx.listener(|this, _action: &Refresh, _, cx| {
                this.reload_volume_mounts();
                this.reload_current_directory(cx)
            }))
            .on_action(cx.listener(|this, _action: &SelectAll, _, cx| {
                this.select_all_visible(cx)
            }))
            .on_action(cx.listener(|this, _action: &CreateFolder, _, cx| {
                this.create_folder(cx)
            }))
            .on_action(cx.listener(|this, _action: &CreateFile, _, cx| {
                this.create_file(cx)
            }))
            .on_action(cx.listener(|this, _action: &Rename, _, cx| {
                this.start_rename_selected(cx)
            }))
            .on_action(cx.listener(|this, _action: &Copy, _, cx| this.copy_selected(cx)))
            .on_action(cx.listener(|this, _action: &Cut, _, cx| this.cut_selected(cx)))
            .on_action(cx.listener(|this, _action: &Paste, _, cx| this.paste_clipboard(cx)))
            .on_action(cx.listener(|this, _action: &Duplicate, _, cx| {
                this.duplicate_selected(cx)
            }))
            .on_action(cx.listener(|this, _action: &ClearSelection, _, cx| {
                this.clear_selection(cx)
            }))
            .on_action(cx.listener(|this, _action: &InvertSelection, _, cx| {
                this.invert_selection(cx)
            }))
            .on_action(cx.listener(|this, _action: &ActivateSearch, window, cx| {
                this.activate_search(window, cx)
            }))
            .on_action(cx.listener(|this, _action: &ClearSearch, _, cx| {
                this.clear_search(cx)
            }))
            .on_action(cx.listener(|this, _action: &ToggleSearchSubfolders, window, cx| {
                this.toggle_search_subfolders(window, cx)
            }))
            .on_action(cx.listener(|this, _action: &SetSearchCurrentFolder, window, cx| {
                this.set_search_scope(SearchScope::CurrentFolder, window, cx)
            }))
            .on_action(cx.listener(|this, _action: &SetSearchIncludeSubfolders, window, cx| {
                this.set_search_scope(SearchScope::Subfolders, window, cx)
            }))
            .on_action(cx.listener(|this, _action: &FocusPathBar, window, cx| {
                this.focus_path_bar(window, cx)
            }))
            .on_action(cx.listener(|this, _action: &GoHome, _, cx| this.go_home(cx)))
            .on_action(cx.listener(|this, _action: &Undo, _, cx| this.undo_last(cx)))
            .on_action(cx.listener(|this, _action: &Redo, _, cx| this.redo_last(cx)))
            .on_action(cx.listener(|this, _action: &OpenTerminal, _, cx| {
                this.open_terminal_here(cx)
            }))
            .on_action(cx.listener(|this, _action: &OpenSelection, _, cx| {
                this.open_primary_selection(cx)
            }))
            .on_action(cx.listener(|this, _action: &OpenWithSystem, _, cx| {
                this.open_selection_with_system(cx)
            }))
            .on_action(cx.listener(|this, _action: &ShowProperties, _, cx| {
                this.show_properties_for_selection(cx)
            }))
            .on_action(cx.listener(|this, _action: &ShowSettings, _, cx| {
                this.open_settings(cx)
            }))
            .on_action(cx.listener(|this, _action: &ShowAbout, _, cx| this.open_about(cx)))
            .on_action(cx.listener(|this, _action: &GoToParent, _, cx| {
                this.go_to_parent_of_selection(cx)
            }))
            .on_action(cx.listener(|this, _action: &ZoomIn, _, cx| this.zoom_icons_in(cx)))
            .on_action(cx.listener(|this, _action: &ZoomOut, _, cx| this.zoom_icons_out(cx)))
            .on_action(cx.listener(|this, _action: &ZoomReset, _, cx| {
                this.zoom_icons_reset(cx)
            }))
            .on_action(cx.listener(|this, _action: &NewTab, _, cx| this.new_tab(cx)))
            .on_action(cx.listener(|this, _action: &NewWindow, _, cx| {
                this.spawn_new_window(cx)
            }))
            .on_action(cx.listener(|this, _action: &CloseTab, _, cx| this.close_tab(cx)))
            .on_action(cx.listener(|this, _action: &AddBookmark, _, cx| {
                this.add_bookmark_for_current(cx)
            }))
            .on_action(cx.listener(|this, _action: &RemoveBookmark, _, cx| {
                this.remove_bookmark_for_current(cx)
            }))
            .on_action(cx.listener(|this, _action: &SortByName, _, cx| {
                this.apply_sort(SortColumn::Name, None, cx)
            }))
            .on_action(cx.listener(|this, _action: &SortBySize, _, cx| {
                this.apply_sort(SortColumn::Size, None, cx)
            }))
            .on_action(cx.listener(|this, _action: &SortByModified, _, cx| {
                this.apply_sort(SortColumn::Modified, None, cx)
            }))
            .on_action(cx.listener(|this, _action: &SortByType, _, cx| {
                this.apply_sort(SortColumn::Type, None, cx)
            }))
            .on_action(cx.listener(|this, _action: &ToggleSortOrder, _, cx| {
                this.toggle_sort_order(cx)
            }))
            .on_action(cx.listener(|this, _action: &SortNameAsc, _, cx| {
                this.apply_sort(SortColumn::Name, Some(SortOrder::Ascending), cx)
            }))
            .on_action(cx.listener(|this, _action: &SortNameDesc, _, cx| {
                this.apply_sort(SortColumn::Name, Some(SortOrder::Descending), cx)
            }))
            .on_action(cx.listener(|this, _action: &SortSizeAsc, _, cx| {
                this.apply_sort(SortColumn::Size, Some(SortOrder::Ascending), cx)
            }))
            .on_action(cx.listener(|this, _action: &SortSizeDesc, _, cx| {
                this.apply_sort(SortColumn::Size, Some(SortOrder::Descending), cx)
            }))
            .on_action(cx.listener(|this, _action: &SortModifiedAsc, _, cx| {
                this.apply_sort(SortColumn::Modified, Some(SortOrder::Ascending), cx)
            }))
            .on_action(cx.listener(|this, _action: &SortModifiedDesc, _, cx| {
                this.apply_sort(SortColumn::Modified, Some(SortOrder::Descending), cx)
            }))
            .on_action(cx.listener(|this, _action: &SortTypeAsc, _, cx| {
                this.apply_sort(SortColumn::Type, Some(SortOrder::Ascending), cx)
            }))
            .on_action(cx.listener(|this, _action: &SortTypeDesc, _, cx| {
                this.apply_sort(SortColumn::Type, Some(SortOrder::Descending), cx)
            }))
            .on_action(cx.listener(|this, _action: &ViewList, _, cx| {
                this.set_view_mode(ViewMode::List, cx)
            }))
            .on_action(cx.listener(|this, _action: &ViewIcon, _, cx| {
                this.set_view_mode(ViewMode::Icon, cx)
            }))
            .on_action(cx.listener(|this, _action: &ViewCompact, _, cx| {
                this.set_view_mode(ViewMode::Compact, cx)
            }))
            .on_action(cx.listener(|this, _action: &ViewTable, _, cx| {
                this.set_view_mode(ViewMode::Table, cx)
            }))
            .on_key_down(cx.listener(|this, event, _, cx| {
                this.handle_toolbar_input_key(event, cx)
            }))
            .flex()
            .flex_col()
            .h_full()
            .w_full()
            .bg(colors.background)
            .text_color(colors.text)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(window, cx))
                    .child(self.render_main_panel(window, cx))
            )
            .child(self.render_status_bar(window, cx))
            .when_some(self.pending_delete.clone(), |root, pending| {
                root.child(Self::render_delete_dialog(pending, cx))
            })
            .when_some(self.pending_rename.clone(), |root, pending| {
                root.child(Self::render_rename_dialog(pending, cx))
            })
            .when_some(self.pending_properties.clone(), |root, dialog| {
                root.child(Self::render_properties_dialog(dialog, cx))
            })
            .when_some(self.pending_settings.clone(), |root, draft| {
                root.child(self.render_settings_dialog(draft, cx))
            })
            .when_some(self.pending_paste_choice.clone(), |root, pending| {
                root.child(Self::render_paste_conflict_dialog(pending, cx))
            })
            .when(self.show_about, |root| root.child(Self::render_about_dialog(cx)))
            .children(self.context_menu.as_ref().map(|(menu, position, _)| {
                deferred(
                    anchored()
                        .position(*position)
                        .anchor(gpui::Anchor::TopLeft)
                        .child(menu.clone()),
                )
                .with_priority(1)
            }))
    }
}

impl FilemanWindow {
    fn render_sidebar(&mut self, _window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let sidebar_width = self.config.window.splitter_pos;
        let colors = cx.theme().colors().clone();
        let places = quick_access_places();
        let bookmark_paths = self.bookmark_paths.clone();
        let volume_mounts = self.volume_mounts.clone();

        let sidebar = v_flex()
            .id("fileman-sidebar")
            .w(px(sidebar_width as f32))
            .h_full()
            .bg(colors.panel_background)
            .border_r_1()
            .border_color(colors.border)
            .p_3()
            .gap_2()
            .overflow_y_scroll()
            .drag_over::<DraggedFilePaths>(|style, _, _, cx| drop_target_style(style, cx))
            .drag_over::<ExternalPaths>(|style, _, _, cx| drop_target_style(style, cx))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.drop_external_files(paths, cx);
            }))
            .on_drop(cx.listener(|this, dragged: &DraggedFilePaths, _, cx| {
                this.drop_internal_files(dragged, cx);
            }))
            .child(Headline::new("Quick Access").size(HeadlineSize::XSmall))
            .child(
                v_flex()
                    .gap_0p5()
                    .children(places.into_iter().map(|(label, path)| {
                        let is_active = self.current_path == path;
                        let path_clone = path.clone();
                        let label_string = label.to_string();

                        self.apply_directory_drop_target(
                            div()
                                .id(SharedString::from(format!("sidebar-drop-{label_string}")))
                                .child(
                                ListItem::new(SharedString::from(format!("sidebar-{label_string}")))
                                    .toggle_state(is_active)
                                    .rounded()
                                    .start_slot(ui_icons::cached_icon_element(
                                        ui_icons::quick_access_theme_icon(label)
                                            .and_then(|name| {
                                                self.icon_cache.cached_theme_icon(
                                                    name,
                                                    ui_icons::SIDEBAR_ICON_PIXELS,
                                                )
                                            })
                                            .or_else(|| {
                                                self.icon_cache.cached_icon(
                                                    &path,
                                                    ui_icons::SIDEBAR_ICON_PIXELS,
                                                )
                                            }),
                                        IconSize::Small,
                                        if is_active {
                                            Color::Selected
                                        } else {
                                            Color::Muted
                                        },
                                        cx,
                                    ))
                                    .child(Label::new(label_string))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.navigate_to(path_clone.clone(), true, cx);
                                    })),
                            ),
                            path.clone(),
                            cx,
                        )
                    })),
            )
            .when(!volume_mounts.is_empty(), |sidebar| {
                sidebar
                    .child(Headline::new("Devices").size(HeadlineSize::XSmall))
                    .child(
                        v_flex().gap_0p5().children(volume_mounts.into_iter().map(
                            |mount| {
                                let is_active = mount.mount_point == self.current_path;
                                let path = mount.mount_point.clone();
                                let navigate_path = path.clone();
                                let label = mount.label.clone();
                                let item_id = SharedString::from(format!(
                                    "device-{}",
                                    mount.mount_point.display()
                                ));

                                self.apply_directory_drop_target(
                                    div()
                                        .id(item_id.clone())
                                        .child(
                                        ListItem::new(item_id)
                                            .toggle_state(is_active)
                                            .rounded()
                                            .start_slot(ui_icons::cached_icon_element(
                                                self.icon_cache.cached_icon(
                                                    &path,
                                                    ui_icons::SIDEBAR_ICON_PIXELS,
                                                ),
                                                IconSize::Small,
                                                if is_active {
                                                    Color::Selected
                                                } else {
                                                    Color::Muted
                                                },
                                                cx,
                                            ))
                                            .child(Label::new(label).truncate())
                                            .on_click(cx.listener({
                                                let navigate_path = navigate_path.clone();
                                                move |this, _, _, cx| {
                                                    this.navigate_to(navigate_path.clone(), true, cx);
                                                }
                                            })),
                                    ),
                                    path,
                                    cx,
                                )
                            },
                        )),
                    )
            })
            .when(!bookmark_paths.is_empty(), |sidebar| {
                sidebar
                    .child(Headline::new("Bookmarks").size(HeadlineSize::XSmall))
                    .child(
                        v_flex().gap_0p5().children(bookmark_paths.into_iter().map(
                            |path| {
                                let display_name = path
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .map(str::to_string)
                                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                                let is_active = path == self.current_path;
                                let path_clone = path.clone();
                                let item_id =
                                    SharedString::from(format!("bookmark-{}", path.display()));

                                self.apply_directory_drop_target(
                                    div()
                                        .id(item_id.clone())
                                        .child(
                                        ListItem::new(item_id)
                                            .toggle_state(is_active)
                                            .rounded()
                                            .start_slot(ui_icons::cached_icon_element(
                                                self.icon_cache.cached_icon(
                                                    &path,
                                                    ui_icons::SIDEBAR_ICON_PIXELS,
                                                ),
                                                IconSize::Small,
                                                if is_active {
                                                    Color::Selected
                                                } else {
                                                    Color::Muted
                                                },
                                                cx,
                                            ))
                                            .child(Label::new(display_name.to_string()).truncate())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if path_clone.is_dir() {
                                                    this.navigate_to(path_clone.clone(), true, cx);
                                                } else {
                                                    this.set_status(
                                                        "Bookmark path is not a directory",
                                                        cx,
                                                    );
                                                }
                                            })),
                                    ),
                                    path.clone(),
                                    cx,
                                )
                            },
                        )),
                    )
            });

        sidebar
    }

    fn render_main_panel(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .child(self.render_tab_strip(cx))
            .child(self.render_toolbar(window, cx))
            .child(self.render_files_area(window, cx))
    }

    fn render_tab_strip(&self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let tab_paths = self.tabs.paths_for_strip();
        let tab_count = tab_paths.len();
        let active_tab = self.tabs.active;
        let show_close = tab_count > 1;

        h_flex()
            .id("fileman-tab-strip")
            .h(px(36.0))
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .px_2()
            .bg(colors.panel_background)
            .border_b_1()
            .border_color(colors.border)
            .children(tab_paths.into_iter().enumerate().map(|(index, path)| {
                let label = TabModel::tab_label(index, &path);
                let is_active = index == active_tab;
                let tab_id = SharedString::from(format!("tab-{index}"));

                ListItem::new(tab_id)
                    .toggle_state(is_active)
                    .rounded()
                    .child(
                        Label::new(label)
                            .size(LabelSize::Small)
                            .color(if is_active {
                                Color::Selected
                            } else {
                                Color::Default
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.switch_tab(index, cx);
                    }))
                    .when(show_close, |tab| {
                        tab.end_slot(
                            ThemeIconButton::new(SharedString::from(format!("close-tab-{index}")))
                                .cached(self.icon_cache.cached_theme_icon(
                                    ui_icons::TAB_CLOSE,
                                    ui_icons::TOOLBAR_ICON_PIXELS,
                                ))
                                .icon_size(IconSize::XSmall)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.close_tab_at(index, cx);
                            })),
                        )
                    })
            }))
            .child(
                ThemeIconButton::new("new-tab")
                    .cached(self.icon_cache.cached_theme_icon(
                        ui_icons::TAB_NEW,
                        ui_icons::TOOLBAR_ICON_PIXELS,
                    ))
                    .icon_size(IconSize::Small)
                    .on_click(cx.listener(|this, _, _, cx| this.new_tab(cx))),
            )
    }

    fn render_toolbar_view_menu(&self, window: &mut Window, cx: &mut ViewContext<Self>) -> DropdownMenu {
        DropdownMenu::new(
            "toolbar-view-mode",
            self.view_mode.menu_label(),
            ContextMenu::build(window, cx, |menu, _, _| {
                menu.action("List View", ViewList.boxed_clone())
                    .action("Icon View", ViewIcon.boxed_clone())
                    .action("Compact View", ViewCompact.boxed_clone())
                    .action("Table View", ViewTable.boxed_clone())
            }),
        )
        .style(DropdownStyle::Outlined)
        .trigger_size(ButtonSize::Compact)
    }

    fn render_toolbar(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let show_hidden = self.show_hidden;
        let colors = cx.theme().colors().clone();
        let path_edit_active = self.path_edit_active;
        let search_active = self.search_active;
        let center_border = if path_edit_active || search_active {
            colors.border_focused
        } else {
            colors.border_variant
        };

        v_flex()
            .id("fileman-toolbar")
            .flex_shrink_0()
            .border_b_1()
            .border_color(colors.border)
            .child(
                h_flex()
                    .h(px(52.0))
                    .items_center()
                    .justify_between()
                    .px_3()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                ThemeIconButton::new("go-back")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        ui_icons::GO_BACK,
                                        ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
                            )
                            .child(
                                ThemeIconButton::new("go-forward")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        ui_icons::GO_FORWARD,
                                        ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| this.go_forward(cx))),
                            )
                            .child(
                                ThemeIconButton::new("go-up")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        ui_icons::GO_UP,
                                        ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| this.go_up(cx))),
                            )
                            .child(
                                ThemeIconButton::new("refresh")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        ui_icons::REFRESH,
                                        ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reload_current_directory(cx)
                                    })),
                            )
                            .child(
                                ThemeIconButton::new("copy")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        ui_icons::COPY,
                                        ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| this.copy_selected(cx))),
                            )
                            .child(
                                ThemeIconButton::new("cut")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        ui_icons::CUT,
                                        ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| this.cut_selected(cx))),
                            )
                            .child(
                                ThemeIconButton::new("paste")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        ui_icons::PASTE,
                                        ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.paste_clipboard(cx)
                                    })),
                            )
                            .child(
                                ThemeIconButton::new("toolbar-delete")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        ui_icons::DELETE,
                                        ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.delete_selected(cx)
                                    })),
                            )
                            .child(
                                ThemeIconButton::new("toolbar-properties")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        ui_icons::PROPERTIES,
                                        ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_properties_for_selection(cx)
                                    })),
                            )
                            .child(self.render_toolbar_view_menu(window, cx)),
                    )
                    .child(self.render_location_bar(window, cx, center_border))
                    .child(
                        ThemeIconButton::new("search")
                            .cached(self.icon_cache.cached_theme_icon(
                                ui_icons::SEARCH,
                                ui_icons::TOOLBAR_ICON_PIXELS,
                            ))
                            .toggle_state(self.search_active)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.search_active {
                                    this.clear_search(cx);
                                } else {
                                    this.activate_search(window, cx);
                                }
                            })),
                    )
                    .child({
                        let mut hidden_button = Button::new("toggle-hidden", "Hidden")
                            .style(ButtonStyle::Outlined)
                            .toggle_state(show_hidden)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_hidden(cx)));
                        if let Some(hidden_icon) = ui_icons::cached_theme_icon(
                            self.icon_cache.cached_theme_icon(
                                if show_hidden {
                                    ui_icons::SHOW_HIDDEN
                                } else {
                                    ui_icons::HIDE_HIDDEN
                                },
                                ui_icons::TOOLBAR_ICON_PIXELS,
                            ),
                            IconSize::Small,
                            Color::Default,
                        ) {
                            hidden_button = hidden_button.start_icon(hidden_icon);
                        }
                        hidden_button
                    }),
            )
    }

    fn render_location_bar(
        &self,
        _window: &mut Window,
        cx: &mut ViewContext<Self>,
        border_color: Hsla,
    ) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let search_history = self.search_history.clone();
        let search_active = self.search_active;

        v_flex()
            .flex_1()
            .min_w_0()
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .bg(colors.elevated_surface_background)
                    .border_1()
                    .border_color(border_color)
                    .rounded_md()
                    .px_2()
                    .py_1()
                    .gap_1()
            .when(self.path_edit_active, |bar| {
                bar.child(self.path_line_input.clone())
            })
            .when(self.search_active, |bar| {
                let scope_label = match self.search_scope {
                    SearchScope::CurrentFolder => "folder",
                    SearchScope::Subfolders => "tree",
                };
                bar.child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .items_center()
                        .gap_1()
                        .child(
                            Label::new(format!("({scope_label})"))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .flex_none(),
                        )
                        .child(self.search_line_input.clone()),
                )
            })
            .when(!self.path_edit_active && !self.search_active, |bar| {
                let segments = breadcrumb_segments(&self.current_path);
                let segment_count = segments.len();
                bar.children(segments.into_iter().enumerate().map(|(index, segment)| {
                    let path = segment.path.clone();
                    let show_separator = index + 1 < segment_count;
                    let breadcrumb_button = if segment.clickable {
                        Button::new(
                            SharedString::from(format!("breadcrumb-{}", segment.path.display())),
                            segment.label,
                        )
                        .style(ButtonStyle::Transparent)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.navigate_to(path.clone(), true, cx);
                        }))
                    } else {
                        Button::new(
                            SharedString::from(format!(
                                "breadcrumb-current-{}",
                                segment.path.display()
                            )),
                            segment.label,
                        )
                        .style(ButtonStyle::Transparent)
                    };
                    if show_separator {
                        h_flex()
                            .items_center()
                            .gap_0p5()
                            .child(breadcrumb_button)
                            .child(Label::new("/").color(Color::Muted).size(LabelSize::XSmall))
                            .into_any_element()
                    } else {
                        breadcrumb_button.into_any_element()
                    }
                }))
            }),
            )
            .when(search_active && !search_history.is_empty(), |column| {
                column.child(
                    h_flex()
                        .w_full()
                        .flex_wrap()
                        .gap_1()
                        .children(search_history.into_iter().enumerate().map(
                            |(index, query)| {
                                let query_for_click = query.clone();
                                Button::new(
                                    SharedString::from(format!("search-history-{index}")),
                                    query,
                                )
                                .style(ButtonStyle::Transparent)
                                .size(ButtonSize::Compact)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let query = query_for_click.clone();
                                    this.search_query = query.clone();
                                    this.search_line_input.update(cx, |input, cx| {
                                        input.set_text(query, cx);
                                    });
                                    this.schedule_subfolder_search(cx);
                                    cx.notify();
                                }))
                            },
                        )),
                )
            })
    }

    fn render_file_entry_range(
        &mut self,
        range: Range<usize>,
        view_mode: ViewMode,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) -> Vec<AnyElement> {
        self.list_visible_range = Some(range.clone());
        self.refresh_uniform_list_row_height(self.visible_files().len());
        let visible_files = self.visible_files();
        range
            .filter_map(|entry_index| {
                visible_files.get(entry_index).map(|file_info| {
                    self.render_file_entry(file_info, entry_index, view_mode, window, cx)
                })
            })
            .collect()
    }

    fn render_search_match_range(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) -> Vec<AnyElement> {
        self.list_visible_range = Some(range.clone());
        self.refresh_uniform_list_row_height(self.search_matches.len());
        range
            .filter_map(|entry_index| {
                self.search_matches.get(entry_index).map(|search_match| {
                    self.render_search_match(search_match, entry_index, window, cx)
                })
            })
            .collect()
    }

    fn render_files_area(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let subfolder_search = self.using_subfolder_search();
        let view_mode = self.view_mode;
        let colors = cx.theme().colors().clone();
        let search_in_progress = self.search_in_progress;
        let item_count = if subfolder_search {
            self.search_matches.len()
        } else {
            self.visible_files().len()
        };
        let empty_state = v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_2()
            .child(ui_icons::cached_icon_element(
                self.icon_cache
                    .cached_theme_icon(ui_icons::FOLDER, ui_icons::TOOLBAR_ICON_PIXELS),
                IconSize::XLarge,
                Color::Muted,
                cx,
            ))
            .child(
                Label::new(if subfolder_search {
                    "No matches in subfolders"
                } else {
                    "This folder is empty"
                })
                    .color(Color::Muted)
                    .size(LabelSize::Small),
            );

        let loading_state = v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .child(
                Label::new(if search_in_progress {
                    "Searching subfolders…"
                } else {
                    "Loading…"
                })
                .color(Color::Muted)
                .size(LabelSize::Small),
            );

        let icon_rows: Vec<_> = if view_mode == ViewMode::Icon
            && !self.loading_directory
            && !search_in_progress
            && item_count > 0
        {
            self.visible_files()
                .into_iter()
                .enumerate()
                .map(|(index, file_info)| {
                    self.render_file_entry(file_info, index, view_mode, window, cx)
                })
                .collect()
        } else {
            Vec::new()
        };

        let scroll = div()
            .id("files-scroll-area")
            .flex_1()
            .flex()
            .flex_col()
            .min_h_0()
            .relative()
            .drag_over::<DraggedFilePaths>(|style, _, _, cx| drop_target_style(style, cx))
            .drag_over::<ExternalPaths>(|style, _, _, cx| drop_target_style(style, cx))
            .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                this.drop_external_files(paths, cx);
            }))
            .on_drop(cx.listener(|this, dragged: &DraggedFilePaths, _, cx| {
                this.drop_internal_files(dragged, cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.deploy_context_menu(
                        event.position,
                        ContextMenuTarget::Background,
                        window,
                        cx,
                    );
                    cx.notify();
                }),
            )
            .when(self.loading_directory || search_in_progress, |panel| panel.child(loading_state))
            .when(
                !self.loading_directory && !search_in_progress && item_count == 0,
                |panel| panel.child(empty_state),
            );

        if view_mode == ViewMode::Icon {
            let icon_scroll_handle = self.files_scroll_handle.0.borrow().base_handle.clone();
            scroll.child(
                self.attach_marquee_handlers(
                    div()
                        .id("fileman-marquee-layer")
                        .flex_1()
                        .min_h_0()
                        .relative(),
                    cx,
                )
                .child(
                    div()
                        .id("fileman-icon-scroll")
                        .overflow_y_scroll()
                        .size_full()
                        .track_scroll(&icon_scroll_handle)
                        .p_2()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .children(icon_rows),
                )
                .child(self.render_marquee_overlay(cx)),
            )
        } else {
            let table_header = (view_mode == ViewMode::Table
                && !self.loading_directory
                && item_count > 0)
                .then(|| {
                    h_flex()
                        .px_2()
                        .py_1()
                        .gap_2()
                        .border_b_1()
                        .border_color(colors.border_variant)
                        .child(
                            Label::new("Name")
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .flex_1(),
                        )
                        .child(
                            div().w(px(72.0)).child(
                                Label::new("Size")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                        )
                        .child(
                            div().w(px(120.0)).child(
                                Label::new("Modified")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                        )
                });

            scroll
                .when_some(table_header, |panel, header| panel.child(header))
                .child(
                    self.attach_marquee_handlers(
                        div()
                            .id("fileman-marquee-layer")
                            .flex_1()
                            .min_h_0()
                            .relative(),
                        cx,
                    )
                    .child(
                        uniform_list(
                            "fileman-file-list",
                            item_count,
                            cx.processor(move |this, range: Range<usize>, window, cx| {
                                if subfolder_search {
                                    this.render_search_match_range(range, window, cx)
                                } else {
                                    this.render_file_entry_range(range, view_mode, window, cx)
                                }
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.files_scroll_handle),
                    )
                    .vertical_scrollbar_for(&self.files_scroll_handle, window, cx)
                    .child(self.render_marquee_overlay(cx)),
                )
        }
    }

    fn render_marquee_overlay(&self, cx: &mut ViewContext<Self>) -> AnyElement {
        let Some(marquee) = self.marquee_drag.as_ref().filter(|marquee| marquee.active) else {
            return div().into_any_element();
        };

        let colors = cx.theme().colors();
        let origin = self.marquee_local_point(marquee.origin);
        let pointer = self.marquee_local_point(marquee.pointer);
        let left = origin.x.min(pointer.x);
        let top = origin.y.min(pointer.y);
        let width = (pointer.x - origin.x).abs();
        let height = (pointer.y - origin.y).abs();

        div()
            .absolute()
            .left(left)
            .top(top)
            .w(width)
            .h(height)
            .border_1()
            .border_dashed()
            .border_color(colors.border_selected)
            .bg(colors.element_selection_background.opacity(0.35))
            .into_any_element()
    }

    fn render_file_icon(
        &self,
        file_path: &Path,
        view_mode: ViewMode,
        file_icon: IconName,
        icon_color: Color,
        cx: &App,
    ) -> AnyElement {
        if let Some(presentation) = self.icon_cache.cached_icon(file_path, self.icon_size) {
            return Self::file_icon_element(presentation, view_mode, icon_color, cx);
        }

        let icon_size = match view_mode {
            ViewMode::Icon => IconSize::XLarge,
            ViewMode::Compact => IconSize::XSmall,
            ViewMode::List | ViewMode::Table => IconSize::Small,
        };

        Icon::new(file_icon)
            .size(icon_size)
            .color(icon_color)
            .into_any_element()
    }

    fn file_icon_element(
        presentation: FileIconPresentation,
        view_mode: ViewMode,
        icon_color: Color,
        cx: &App,
    ) -> AnyElement {
        let icon_size = match view_mode {
            ViewMode::Icon => IconSize::XLarge,
            ViewMode::Compact => IconSize::XSmall,
            ViewMode::List | ViewMode::Table => IconSize::Small,
        };

        match presentation {
            FileIconPresentation::RenderImage(image) => img(ImageSource::Render(image))
                .size(icon_size.rems())
                .into_any_element(),
            _ => ui_icons::presentation_element(presentation, icon_size, icon_color, cx),
        }
    }

    fn render_file_entry(
        &self,
        file_info: &FileInfo,
        entry_index: usize,
        view_mode: ViewMode,
        _window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) -> AnyElement {
        let name = file_info.get_name().unwrap_or("").to_string();
        let is_directory = file_info.get_file_type() == FileType::Directory;
        let is_selected = self.selected_files.contains(&name)
            || self.list_focus_index == Some(entry_index);
        let size_string = if is_directory {
            "--".to_string()
        } else {
            format_size(file_info.get_size())
        };
        let modified_string = format_modified(file_info);
        let name_for_click = name.clone();
        let name_for_open = name.clone();
        let file_path = self.current_path.join(&name);
        let file_icon = if is_directory {
            IconName::Folder
        } else {
            IconName::File
        };
        let icon_color = if is_directory {
            Color::Accent
        } else {
            Color::Default
        };
        let icon_element = self.render_file_icon(&file_path, view_mode, file_icon, icon_color, cx);
        let item_id = SharedString::from(format!("file-row-{name}"));
        let drag_payload = DraggedFilePaths {
            paths: if is_selected && self.selected_files.len() > 1 {
                self.selected_paths()
            } else {
                vec![file_path.clone()]
            },
        };

        let click_handler = cx.listener(move |this, event: &ClickEvent, _, cx| {
            Self::handle_file_item_click(
                this,
                event,
                entry_index,
                &name_for_open,
                is_directory,
                &name_for_click,
                cx,
            );
        });
        let context_name = name.clone();
        let context_handler = cx.listener(
            move |this, event: &MouseDownEvent, window, cx| {
                this.prepare_context_selection(&context_name, cx);
                this.deploy_context_menu(
                    event.position,
                    ContextMenuTarget::FileList,
                    window,
                    cx,
                );
                cx.notify();
            },
        );
        let drag_row_id = SharedString::from(format!("file-drag-{name}"));
        let row_height = self.files_list_item_height();

        let list_item = match view_mode {
            ViewMode::Icon => ListItem::new(item_id)
                .toggle_state(is_selected)
                .rounded()
                .child(
                    v_flex()
                        .items_center()
                        .gap_1()
                        .child(icon_element)
                        .child(Label::new(name).size(LabelSize::XSmall).truncate()),
                )
                .on_click(click_handler)
                .on_secondary_mouse_down(context_handler)
                .into_any_element(),
            ViewMode::Compact => ListItem::new(item_id)
                .toggle_state(is_selected)
                .rounded()
                .height(row_height)
                .start_slot(icon_element)
                .child(Label::new(name).size(LabelSize::Small).truncate())
                .on_click(click_handler)
                .on_secondary_mouse_down(context_handler)
                .into_any_element(),
            ViewMode::Table => ListItem::new(item_id)
                .toggle_state(is_selected)
                .rounded()
                .height(row_height)
                .start_slot(icon_element)
                .child(Label::new(name).truncate().flex_1())
                .end_slot(
                    h_flex()
                        .gap_2()
                        .child(
                            div().w(px(72.0)).child(
                                Label::new(size_string)
                                    .color(Color::Muted)
                                    .size(LabelSize::XSmall),
                            ),
                        )
                        .child(
                            div().w(px(120.0)).child(
                                Label::new(modified_string)
                                    .color(Color::Muted)
                                    .size(LabelSize::XSmall),
                            ),
                        ),
                )
                .on_click(click_handler)
                .on_secondary_mouse_down(context_handler)
                .into_any_element(),
            ViewMode::List => ListItem::new(item_id)
                .toggle_state(is_selected)
                .rounded()
                .height(row_height)
                .start_slot(icon_element)
                .child(Label::new(name))
                .end_slot(Label::new(size_string).color(Color::Muted).size(LabelSize::XSmall))
                .on_click(click_handler)
                .on_secondary_mouse_down(context_handler)
                .into_any_element(),
        };

        let mut row = div()
            .id(drag_row_id)
            .h(row_height)
            .on_drag(drag_payload, |payload: &DraggedFilePaths, _, _, cx| {
                cx.new(|_| payload.clone())
            })
            .child(list_item);
        if view_mode == ViewMode::Icon {
            row = row.w(px(88.0));
        }
        if is_directory {
            row = self.apply_directory_drop_target(row, file_path, cx);
        }
        row.into_any_element()
    }

    fn render_search_match(
        &self,
        search_match: &SearchMatch,
        entry_index: usize,
        _window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) -> AnyElement {
        let path = search_match.path.clone();
        let selection_key = Self::selection_key_for_path(&path);
        let is_selected = self.selected_files.contains(&selection_key)
            || self.list_focus_index == Some(entry_index);
        let is_directory = search_match.is_directory;
        let file_icon = if is_directory {
            IconName::Folder
        } else {
            IconName::File
        };
        let icon_color = if is_directory {
            Color::Accent
        } else {
            Color::Default
        };
        let icon_element =
            self.render_file_icon(&path, ViewMode::List, file_icon, icon_color, cx);
        let item_id = SharedString::from(format!("search-row-{}", path.display()));
        let subtitle = format!("{} · {}", search_match.parent_label, search_match.name);
        let drag_payload = DraggedFilePaths {
            paths: if is_selected && self.selected_files.len() > 1 {
                self.selected_paths()
            } else {
                vec![path.clone()]
            },
        };
        let path_for_open = path.clone();
        let selection_key_for_click = selection_key.clone();
        let selection_key_for_context = selection_key.clone();

        let click_handler = cx.listener(move |this, event: &ClickEvent, _, cx| {
            Self::handle_search_item_click(
                this,
                event,
                entry_index,
                &path_for_open,
                is_directory,
                &selection_key_for_click,
                cx,
            );
        });
        let context_handler = cx.listener(move |this, event: &MouseDownEvent, window, cx| {
            if !this.selected_files.contains(&selection_key_for_context) {
                this.selected_files.clear();
                this.selected_files
                    .insert(selection_key_for_context.clone());
            }
            this.deploy_context_menu(
                event.position,
                ContextMenuTarget::FileList,
                window,
                cx,
            );
            cx.notify();
        });

        let row_height = self.marquee_list_row_height();
        let mut row = div()
            .id(SharedString::from(format!("search-drag-{}", path.display())))
            .h(row_height)
            .on_drag(drag_payload, |payload: &DraggedFilePaths, _, _, cx| {
                cx.new(|_| payload.clone())
            })
            .child(
                ListItem::new(item_id)
                    .toggle_state(is_selected)
                    .rounded()
                    .height(row_height)
                    .start_slot(icon_element)
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(Label::new(search_match.name.clone()).truncate())
                            .child(
                                Label::new(subtitle)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted)
                                    .truncate(),
                            ),
                    )
                    .on_click(click_handler)
                    .on_secondary_mouse_down(context_handler),
            );
        if is_directory {
            row = self.apply_directory_drop_target(row, path, cx);
        }
        row.into_any_element()
    }

    fn handle_search_item_click(
        this: &mut Self,
        event: &ClickEvent,
        entry_index: usize,
        path: &Path,
        is_directory: bool,
        selection_key: &str,
        cx: &mut ViewContext<Self>,
    ) {
        if event.click_count() == 2 {
            if is_directory {
                this.navigate_to(path.to_path_buf(), true, cx);
            } else {
                this.launch_file(path, cx);
            }
            return;
        }

        let modifiers = event.modifiers();
        this.apply_list_selection_click(
            entry_index,
            selection_key,
            modifiers.shift,
            modifiers.control || modifiers.platform,
            cx,
        );
    }

    fn handle_file_item_click(
        this: &mut Self,
        event: &ClickEvent,
        entry_index: usize,
        name_for_open: &str,
        is_directory: bool,
        name_for_click: &str,
        cx: &mut ViewContext<Self>,
    ) {
        if event.click_count() == 2 {
            let full_path = this.current_path.join(name_for_open);
            if is_directory {
                this.navigate_to(full_path, true, cx);
            } else {
                this.launch_file(&full_path, cx);
            }
            return;
        }

        let modifiers = event.modifiers();
        this.apply_list_selection_click(
            entry_index,
            name_for_click,
            modifiers.shift,
            modifiers.control || modifiers.platform,
            cx,
        );
    }

    fn render_status_bar(
        &mut self,
        _window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let selection_count = self.selected_files.len();
        let item_count = if self.using_subfolder_search() {
            self.search_matches.len()
        } else {
            self.visible_files().len()
        };
        let selection_summary = if selection_count == 0 {
            format!("{item_count} items")
        } else {
            format!("{selection_count} selected · {item_count} items")
        };
        let paste_in_progress = self.paste_cancel.is_some();

        h_flex()
            .id("fileman-status-bar")
            .h(px(28.0))
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(colors.border)
            .px_3()
            .bg(colors.panel_background)
            .gap_2()
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(
                        Label::new(self.status_message.clone())
                            .size(LabelSize::Small)
                            .truncate(),
                    )
                    .when(paste_in_progress, |row| {
                        row.child(
                            Button::new("paste-job-cancel", "Cancel")
                                .style(ButtonStyle::Outlined)
                                .size(ButtonSize::Compact)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel_active_paste(cx);
                                })),
                        )
                    }),
            )
            .child(
                Label::new(selection_summary)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
    }

    fn render_paste_conflict_dialog(
        pending: PendingPasteChoice,
        cx: &mut ViewContext<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let action = if pending.is_cut { "move" } else { "copy" };
        let message = if pending.conflict_count == 1 {
            format!(
                "1 item already exists in this folder. How should Fileman {action} the conflicting items?"
            )
        } else {
            format!(
                "{} items already exist in this folder. How should Fileman {action} the conflicting items?",
                pending.conflict_count
            )
        };

        div()
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.45))
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(460.0))
                    .gap_3()
                    .p_4()
                    .bg(colors.elevated_surface_background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .child(Headline::new("File already exists").size(HeadlineSize::Small))
                    .child(Label::new(message))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                Button::new("paste-skip-all", "Skip existing items")
                                    .style(ButtonStyle::Outlined)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_paste_with_resolution(
                                            ConflictResolution::Skip,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new("paste-overwrite-all", "Replace existing items")
                                    .style(ButtonStyle::Outlined)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_paste_with_resolution(
                                            ConflictResolution::Overwrite,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new("paste-keep-both-all", "Keep both (rename new items)")
                                    .style(ButtonStyle::Filled)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_paste_with_resolution(
                                            ConflictResolution::KeepBoth,
                                            cx,
                                        );
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .child(
                                Button::new("paste-cancel", "Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_pending_paste(cx);
                                    })),
                            ),
                    ),
            )
    }

    fn render_about_dialog(cx: &mut ViewContext<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let info = about::ABOUT;

        div()
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.45))
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(420.0))
                    .gap_2()
                    .p_4()
                    .bg(colors.elevated_surface_background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .child(
                        Headline::new(format!("{} {}", info.name, info.version))
                            .size(HeadlineSize::Small),
                    )
                    .child(Label::new(info.description).size(LabelSize::Small))
                    .child(Label::new(format!("Authors: {}", info.authors)).size(LabelSize::Small))
                    .child(Label::new(format!("License: {}", info.license)).size(LabelSize::Small))
                    .child(
                        Label::new(format!("Repository: {}", about::REPOSITORY))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .pt_2()
                            .child(
                                Button::new("about-close", "Close")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.dismiss_about(cx);
                                    })),
                            ),
                    ),
            )
    }

    fn render_settings_option(
        id: &'static str,
        label: &'static str,
        checked: bool,
        field: SettingsField,
        cx: &mut ViewContext<Self>,
    ) -> impl IntoElement {
        let toggle_state = if checked {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        };

        h_flex()
            .items_center()
            .gap_2()
            .child(
                Checkbox::new(id, toggle_state).on_click(cx.listener(
                    move |this, _state, _, cx| {
                        this.toggle_settings_field(field, cx);
                    },
                )),
            )
            .child(Label::new(label).size(LabelSize::Small))
    }

    fn render_settings_dialog(
        &self,
        draft: SettingsDraft,
        cx: &mut ViewContext<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let terminal_focused = self.settings_terminal_focus;
        let terminal_display = if draft.terminal_command.is_empty() {
            "(default: $TERMINAL or system fallback)".to_string()
        } else {
            draft.terminal_command.clone()
        };
        let terminal_border = if terminal_focused {
            colors.border_focused
        } else {
            colors.border
        };

        div()
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.45))
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(520.0))
                    .max_h(px(560.0))
                    .gap_3()
                    .p_4()
                    .bg(colors.elevated_surface_background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .child(Headline::new("Configure Fileman").size(HeadlineSize::Small))
                    .child(Headline::new("Display").size(HeadlineSize::XSmall))
                    .child(Self::render_settings_option(
                        "settings-show-hidden",
                        "Show hidden files",
                        draft.show_hidden,
                        SettingsField::ShowHidden,
                        cx,
                    ))
                    .child(Headline::new("Behavior").size(HeadlineSize::XSmall))
                    .child(Self::render_settings_option(
                        "settings-confirm-delete",
                        "Confirm before permanent delete",
                        draft.confirm_delete,
                        SettingsField::ConfirmDelete,
                        cx,
                    ))
                    .child(Self::render_settings_option(
                        "settings-confirm-trash",
                        "Confirm before moving to trash",
                        draft.confirm_trash,
                        SettingsField::ConfirmTrash,
                        cx,
                    ))
                    .child(Self::render_settings_option(
                        "settings-use-trash",
                        "Use trash (Recycle bin)",
                        draft.use_trash,
                        SettingsField::UseTrash,
                        cx,
                    ))
                    .child(Headline::new("System").size(HeadlineSize::XSmall))
                    .child(
                        Label::new("Terminal command (optional)")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .bg(colors.background)
                            .border_1()
                            .border_color(terminal_border)
                            .rounded_md()
                            .px_3()
                            .py_2()
                            .flex_1()
                            .child(Label::new(terminal_display).truncate())
                            .child(
                                Button::new("settings-terminal-edit", "Edit")
                                    .style(ButtonStyle::Outlined)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.focus_settings_terminal(cx);
                                    })),
                            ),
                    )
                    .when(terminal_focused, |panel| {
                        panel.child(
                            Label::new("Type terminal command, Escape to finish editing")
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    })
                    .child(Headline::new("Window").size(HeadlineSize::XSmall))
                    .child(Self::render_settings_option(
                        "settings-remember-size",
                        "Remember window size on exit",
                        draft.remember_window_size,
                        SettingsField::RememberWindowSize,
                        cx,
                    ))
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("settings-cancel", "Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.dismiss_settings(cx);
                                    })),
                            )
                            .child(
                                Button::new("settings-ok", "OK")
                                    .style(ButtonStyle::Filled)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_settings(cx);
                                    })),
                            ),
                    ),
            )
    }

    fn render_properties_dialog(
        dialog: PropertiesDialog,
        cx: &mut ViewContext<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let icon_color = Color::Muted;
        let properties_icon = dialog.icon.clone();

        div()
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.45))
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(480.0))
                    .max_h(px(420.0))
                    .gap_3()
                    .p_4()
                    .bg(colors.elevated_surface_background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .child(Headline::new(dialog.title).size(HeadlineSize::Small))
                    .when_some(properties_icon, |column, icon| {
                        column.child(
                            h_flex()
                                .justify_center()
                                .pb_2()
                                .child(Self::file_icon_element(
                                    icon,
                                    ViewMode::Icon,
                                    icon_color,
                                    cx,
                                )),
                        )
                    })
                    .child(
                        v_flex()
                            .gap_2()
                            .children(dialog.rows.into_iter().map(|row| {
                                h_flex()
                                    .gap_3()
                                    .child(
                                        div().w(px(96.0)).child(
                                            Label::new(row.label)
                                                .size(LabelSize::XSmall)
                                                .color(Color::Muted),
                                        ),
                                    )
                                    .child(Label::new(row.value).truncate())
                            })),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .child(
                                Button::new("properties-close", "Close")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.dismiss_properties(cx);
                                    })),
                            ),
                    ),
            )
    }

    fn render_rename_dialog(
        pending: PendingRename,
        cx: &mut ViewContext<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let original_name = pending
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<unnamed>");

        div()
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.45))
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(420.0))
                    .gap_3()
                    .p_4()
                    .bg(colors.elevated_surface_background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .child(Headline::new("Rename").size(HeadlineSize::Small))
                    .child(Label::new(format!("Rename \"{original_name}\" to:")))
                    .child(
                        h_flex()
                            .items_center()
                            .bg(colors.background)
                            .border_1()
                            .border_color(colors.border_focused)
                            .rounded_md()
                            .px_3()
                            .py_2()
                            .child(Label::new(pending.new_name.clone())),
                    )
                    .child(
                        Label::new("Type a new name, Enter to confirm, Escape to cancel")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("rename-cancel", "Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_pending_rename(cx);
                                    })),
                            )
                            .child(
                                Button::new("rename-confirm", "Rename")
                                    .style(ButtonStyle::Filled)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_pending_rename(cx);
                                    })),
                            ),
                    ),
            )
    }

    fn render_delete_dialog(
        pending: PendingDelete,
        cx: &mut ViewContext<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let message = delete_confirmation_message(&pending.paths, pending.permanent);
        let confirm_label = if pending.permanent {
            "Delete permanently"
        } else if pending.use_trash {
            "Move to Trash"
        } else {
            "Delete"
        };

        div()
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.45))
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(420.0))
                    .gap_3()
                    .p_4()
                    .bg(colors.elevated_surface_background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .child(
                        Headline::new(if pending.permanent {
                            "Confirm permanent delete"
                        } else {
                            "Confirm delete"
                        })
                        .size(HeadlineSize::Small),
                    )
                    .child(Label::new(message))
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("delete-cancel", "Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_pending_delete(cx);
                                    })),
                            )
                            .child(
                                Button::new("delete-confirm", confirm_label)
                                    .style(ButtonStyle::Filled)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_pending_delete(cx);
                                    })),
                            ),
                    ),
            )
    }
}

fn delete_confirmation_message(paths: &[PathBuf], permanent: bool) -> String {
    if paths.len() == 1 {
        let name = paths[0]
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .unwrap_or("<unnamed>");
        if permanent {
            format!("Permanently delete \"{name}\"? This cannot be undone.")
        } else {
            format!("Move \"{name}\" to the trash?")
        }
    } else if permanent {
        format!(
            "Permanently delete {} selected items? This cannot be undone.",
            paths.len()
        )
    } else {
        format!("Move {} selected items to the trash?", paths.len())
    }
}

fn quick_access_places() -> Vec<(&'static str, PathBuf)> {
    let mut places = vec![("Root", PathBuf::from("/"))];
    if let Some(home) = dirs::home_dir() {
        places.push(("Home", home.clone()));
        places.push(("Desktop", home.join("Desktop")));
        places.push(("Documents", home.join("Documents")));
        places.push(("Downloads", home.join("Downloads")));
        places.push(("Music", home.join("Music")));
        places.push(("Pictures", home.join("Pictures")));
        places.push(("Videos", home.join("Videos")));
    }
    places
}

fn path_to_file_uri(path: &Path) -> String {
    let absolute = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf());
    let path_string = absolute.to_string_lossy();
    if path_string.starts_with('/') {
        format!("file://{path_string}")
    } else {
        format!("file:///{path_string}")
    }
}

fn format_modified(file_info: &FileInfo) -> String {
    let timestamp = match file_info.get_attribute("time::modified") {
        Some(npio::FileAttributeType::Uint64(value)) => *value,
        Some(npio::FileAttributeType::Int64(value)) => *value as u64,
        _ => return "--".to_string(),
    };

    if timestamp == 0 {
        return "--".to_string();
    }

    format_unix_timestamp(timestamp)
}

fn format_unix_timestamp(secs: u64) -> String {
    let days = secs / 86_400;
    let remainder = secs % 86_400;
    let hour = remainder / 3600;
    let minute = (remainder % 3600) / 60;

    let mut year = 1970i32;
    let mut day_count = days as i32;
    while day_count >= days_in_year(year) {
        day_count -= days_in_year(year);
        year += 1;
    }

    let mut month = 1i32;
    while day_count >= days_in_month(year, month) {
        day_count -= days_in_month(year, month);
        month += 1;
    }

    format!("{year:04}-{month:02}-{:02} {hour:02}:{minute:02}", day_count + 1)
}

fn days_in_year(year: i32) -> i32 {
    if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
        366
    } else {
        365
    }
}

fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if days_in_year(year) == 366 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

fn format_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn initial_path_from_args(config: &FilemanConfig) -> PathBuf {
    std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| config.folder_view.default_path.clone())
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn window_options(config: &FilemanConfig, cx: &App) -> WindowOptions {
    let size = size(
        px(config.window.last_window_width as f32),
        px(config.window.last_window_height as f32),
    );
    let bounds = Bounds::centered(None, size, cx);

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(config.window.window_title.clone().into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn with_active_fileman(
    cx: &mut App,
    f: impl FnOnce(&mut FilemanWindow, &mut Window, &mut Context<FilemanWindow>),
) {
    let handle = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next());
    let Some(handle) = handle else {
        return;
    };

    let _ = handle.update(cx, |root, window, cx| {
        if let Ok(view) = root.downcast::<FilemanWindow>() {
            view.update(cx, |fileman, cx| f(fileman, window, cx));
        }
    });
}

fn register_app_menu_handlers(cx: &mut App) {
    cx.on_action(|_: &CreateFolder, cx| {
        with_active_fileman(cx, |this, _, cx| this.create_folder(cx));
    });
    cx.on_action(|_: &CreateFile, cx| {
        with_active_fileman(cx, |this, _, cx| this.create_file(cx));
    });
    cx.on_action(|_: &GoBack, cx| {
        with_active_fileman(cx, |this, _, cx| this.go_back(cx));
    });
    cx.on_action(|_: &GoForward, cx| {
        with_active_fileman(cx, |this, _, cx| this.go_forward(cx));
    });
    cx.on_action(|_: &GoUp, cx| {
        with_active_fileman(cx, |this, _, cx| this.go_up(cx));
    });
    cx.on_action(|_: &ToggleHidden, cx| {
        with_active_fileman(cx, |this, _, cx| this.toggle_hidden(cx));
    });
    cx.on_action(|_: &DeleteSelected, cx| {
        with_active_fileman(cx, |this, _, cx| this.delete_selected(cx));
    });
    cx.on_action(|_: &DeletePermanent, cx| {
        with_active_fileman(cx, |this, _, cx| this.request_delete(true, cx));
    });
    cx.on_action(|_: &Refresh, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.reload_volume_mounts();
            this.reload_current_directory(cx);
        });
    });
    cx.on_action(|_: &SelectAll, cx| {
        with_active_fileman(cx, |this, _, cx| this.select_all_visible(cx));
    });
    cx.on_action(|_: &Rename, cx| {
        with_active_fileman(cx, |this, _, cx| this.start_rename_selected(cx));
    });
    cx.on_action(|_: &Copy, cx| {
        with_active_fileman(cx, |this, _, cx| this.copy_selected(cx));
    });
    cx.on_action(|_: &Cut, cx| {
        with_active_fileman(cx, |this, _, cx| this.cut_selected(cx));
    });
    cx.on_action(|_: &Paste, cx| {
        with_active_fileman(cx, |this, _, cx| this.paste_clipboard(cx));
    });
    cx.on_action(|_: &Duplicate, cx| {
        with_active_fileman(cx, |this, _, cx| this.duplicate_selected(cx));
    });
    cx.on_action(|_: &ClearSelection, cx| {
        with_active_fileman(cx, |this, _, cx| this.clear_selection(cx));
    });
    cx.on_action(|_: &InvertSelection, cx| {
        with_active_fileman(cx, |this, _, cx| this.invert_selection(cx));
    });
    cx.on_action(|_: &ActivateSearch, cx| {
        with_active_fileman(cx, |this, window, cx| this.activate_search(window, cx));
    });
    cx.on_action(|_: &ClearSearch, cx| {
        with_active_fileman(cx, |this, _, cx| this.clear_search(cx));
    });
    cx.on_action(|_: &ToggleSearchSubfolders, cx| {
        with_active_fileman(cx, |this, window, cx| this.toggle_search_subfolders(window, cx));
    });
    cx.on_action(|_: &SetSearchCurrentFolder, cx| {
        with_active_fileman(cx, |this, window, cx| {
            this.set_search_scope(SearchScope::CurrentFolder, window, cx);
        });
    });
    cx.on_action(|_: &SetSearchIncludeSubfolders, cx| {
        with_active_fileman(cx, |this, window, cx| {
            this.set_search_scope(SearchScope::Subfolders, window, cx);
        });
    });
    cx.on_action(|_: &FocusPathBar, cx| {
        with_active_fileman(cx, |this, window, cx| this.focus_path_bar(window, cx));
    });
    cx.on_action(|_: &GoHome, cx| {
        with_active_fileman(cx, |this, _, cx| this.go_home(cx));
    });
    cx.on_action(|_: &NewTab, cx| {
        with_active_fileman(cx, |this, _, cx| this.new_tab(cx));
    });
    cx.on_action(|_: &NewWindow, cx| {
        with_active_fileman(cx, |this, _, cx| this.spawn_new_window(cx));
    });
    cx.on_action(|_: &CloseTab, cx| {
        with_active_fileman(cx, |this, _, cx| this.close_tab(cx));
    });
    cx.on_action(|_: &AddBookmark, cx| {
        with_active_fileman(cx, |this, _, cx| this.add_bookmark_for_current(cx));
    });
    cx.on_action(|_: &RemoveBookmark, cx| {
        with_active_fileman(cx, |this, _, cx| this.remove_bookmark_for_current(cx));
    });
    cx.on_action(|_: &SortByName, cx| {
        with_active_fileman(cx, |this, _, cx| this.apply_sort(SortColumn::Name, None, cx));
    });
    cx.on_action(|_: &SortBySize, cx| {
        with_active_fileman(cx, |this, _, cx| this.apply_sort(SortColumn::Size, None, cx));
    });
    cx.on_action(|_: &SortByModified, cx| {
        with_active_fileman(cx, |this, _, cx| this.apply_sort(SortColumn::Modified, None, cx));
    });
    cx.on_action(|_: &SortByType, cx| {
        with_active_fileman(cx, |this, _, cx| this.apply_sort(SortColumn::Type, None, cx));
    });
    cx.on_action(|_: &ToggleSortOrder, cx| {
        with_active_fileman(cx, |this, _, cx| this.toggle_sort_order(cx));
    });
    cx.on_action(|_: &SortNameAsc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Name, Some(SortOrder::Ascending), cx);
        });
    });
    cx.on_action(|_: &SortNameDesc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Name, Some(SortOrder::Descending), cx);
        });
    });
    cx.on_action(|_: &SortSizeAsc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Size, Some(SortOrder::Ascending), cx);
        });
    });
    cx.on_action(|_: &SortSizeDesc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Size, Some(SortOrder::Descending), cx);
        });
    });
    cx.on_action(|_: &SortModifiedAsc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Modified, Some(SortOrder::Ascending), cx);
        });
    });
    cx.on_action(|_: &SortModifiedDesc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Modified, Some(SortOrder::Descending), cx);
        });
    });
    cx.on_action(|_: &SortTypeAsc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Type, Some(SortOrder::Ascending), cx);
        });
    });
    cx.on_action(|_: &SortTypeDesc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Type, Some(SortOrder::Descending), cx);
        });
    });
    cx.on_action(|_: &Undo, cx| {
        with_active_fileman(cx, |this, _, cx| this.undo_last(cx));
    });
    cx.on_action(|_: &Redo, cx| {
        with_active_fileman(cx, |this, _, cx| this.redo_last(cx));
    });
    cx.on_action(|_: &OpenTerminal, cx| {
        with_active_fileman(cx, |this, _, cx| this.open_terminal_here(cx));
    });
    cx.on_action(|_: &OpenSelection, cx| {
        with_active_fileman(cx, |this, _, cx| this.open_primary_selection(cx));
    });
    cx.on_action(|_: &OpenWithSystem, cx| {
        with_active_fileman(cx, |this, _, cx| this.open_selection_with_system(cx));
    });
    cx.on_action(|_: &ShowProperties, cx| {
        with_active_fileman(cx, |this, _, cx| this.show_properties_for_selection(cx));
    });
    cx.on_action(|_: &ShowSettings, cx| {
        with_active_fileman(cx, |this, _, cx| this.open_settings(cx));
    });
    cx.on_action(|_: &ShowAbout, cx| {
        with_active_fileman(cx, |this, _, cx| this.open_about(cx));
    });
    cx.on_action(|_: &GoToParent, cx| {
        with_active_fileman(cx, |this, _, cx| this.go_to_parent_of_selection(cx));
    });
    cx.on_action(|_: &ZoomIn, cx| {
        with_active_fileman(cx, |this, _, cx| this.zoom_icons_in(cx));
    });
    cx.on_action(|_: &ZoomOut, cx| {
        with_active_fileman(cx, |this, _, cx| this.zoom_icons_out(cx));
    });
    cx.on_action(|_: &ZoomReset, cx| {
        with_active_fileman(cx, |this, _, cx| this.zoom_icons_reset(cx));
    });
    cx.on_action(|_: &ViewList, cx| {
        with_active_fileman(cx, |this, _, cx| this.set_view_mode(ViewMode::List, cx));
    });
    cx.on_action(|_: &ViewIcon, cx| {
        with_active_fileman(cx, |this, _, cx| this.set_view_mode(ViewMode::Icon, cx));
    });
    cx.on_action(|_: &ViewCompact, cx| {
        with_active_fileman(cx, |this, _, cx| this.set_view_mode(ViewMode::Compact, cx));
    });
    cx.on_action(|_: &ViewTable, cx| {
        with_active_fileman(cx, |this, _, cx| this.set_view_mode(ViewMode::Table, cx));
    });
    cx.on_action(|_: &Quit, cx| cx.quit());
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = FilemanConfig::load_or_create();
    let initial_path = initial_path_from_args(&config);
    let window_config = config.clone();

    let backend = Arc::new(LocalBackend::new());
    register_backend(backend);

    nptk::gpui_platform::application().run(move |cx: &mut App| {
        nptk::init(cx);
        register_app_menu_handlers(cx);
        cx.activate(true);

        cx.open_window(window_options(&window_config, cx), |_, cx| {
            cx.new(|cx| FilemanWindow::new(initial_path.clone(), cx))
        })
        .expect("failed to open file manager window");
    });
}
