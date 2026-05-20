mod bookmarks;
mod clipboard;
mod config;
mod icons;
mod navigation;
mod operations;
mod sort;
mod tabs;
mod view_mode;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nptk::gpui::{self as gpui, *};
use nptk::gpui_tokio::Tokio;
use nptk::theme::ActiveTheme;
use nptk::ui::{ListItem, prelude::*};
use npio::backend::local::LocalBackend;
use npio::{get_file_for_uri, register_backend, FileInfo, FileType};
use sort::{SortColumn, SortOrder};
use view_mode::ViewMode;

use file_icons::FileIconPresentation;

use crate::clipboard::FileClipboard;
use crate::config::FilemanConfig;
use crate::tabs::TabModel;
use crate::operations::{
    create_directory, create_file, delete_path, duplicate_path, move_to_trash, paste_files,
    rename_path, unique_name_in_parent,
};

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

struct FilemanWindow {
    current_path: PathBuf,
    show_hidden: bool,
    selected_files: HashSet<String>,
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
    path_edit_active: bool,
    tabs: TabModel,
    bookmark_paths: Vec<PathBuf>,
    icon_cache: icons::FileIconCache,
    icon_size: u32,
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
        ActivateSearch,
        ClearSearch,
        FocusPathBar,
        GoHome,
        NewTab,
        CloseTab,
        AddBookmark,
        RemoveBookmark,
        SortByName,
        SortBySize,
        SortByModified,
        ToggleSortOrder,
        ViewList,
        ViewIcon,
        ViewCompact,
        ViewTable,
        Quit
    ]
);

impl FilemanWindow {
    fn new(initial_path: PathBuf, cx: &mut ViewContext<Self>) -> Self {
        let config = FilemanConfig::load_or_create();
        let sort_column = config.sort_column();
        let sort_order = config.sort_order();
        let view_mode = config.view_mode();

        let mut this = Self {
            current_path: initial_path.clone(),
            show_hidden: config.folder_view.show_hidden,
            selected_files: HashSet::new(),
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
            path_edit_active: false,
            tabs: TabModel::new(initial_path.clone()),
            bookmark_paths: bookmarks::load_bookmarks(),
            icon_cache: icons::FileIconCache::new(),
            icon_size: view_mode.icon_size(),
        };

        this.register_menus(cx);
        this.register_keybindings(cx);
        this.navigate_to(initial_path, false, cx);

        cx.on_app_quit(|this, cx| {
            this.persist_window_geometry(cx);
            async move {}
        })
        .detach();

        this
    }

    fn register_menus(&self, cx: &mut ViewContext<Self>) {
        cx.set_menus(vec![
            Menu::new("File").items(vec![
                MenuItem::action("New Folder", CreateFolder),
                MenuItem::action("New File", CreateFile),
                MenuItem::separator(),
                MenuItem::action("Go Up", GoUp),
                MenuItem::action("Refresh", Refresh),
                MenuItem::separator(),
                MenuItem::action("Delete Selected", DeleteSelected),
                MenuItem::separator(),
                MenuItem::action("Quit", Quit),
            ]),
            Menu::new("Edit").items(vec![
                MenuItem::action("Copy", Copy),
                MenuItem::action("Cut", Cut),
                MenuItem::action("Paste", Paste),
                MenuItem::separator(),
                MenuItem::action("Duplicate", Duplicate),
                MenuItem::separator(),
                MenuItem::action("Select All", SelectAll),
                MenuItem::action("Clear Selection", ClearSelection),
                MenuItem::action("Rename", Rename),
            ]),
            Menu::new("View").items(vec![
                MenuItem::action("Toggle Hidden Files", ToggleHidden),
                MenuItem::separator(),
                MenuItem::action("Activate Search", ActivateSearch),
                MenuItem::action("Clear Search", ClearSearch),
                MenuItem::action("Focus Path Bar", FocusPathBar),
                MenuItem::separator(),
                MenuItem::action("Sort by Name", SortByName),
                MenuItem::action("Sort by Size", SortBySize),
                MenuItem::action("Sort by Modified", SortByModified),
                MenuItem::action("Toggle Sort Order", ToggleSortOrder),
                MenuItem::separator(),
                MenuItem::action("List View", ViewList),
                MenuItem::action("Icon View", ViewIcon),
                MenuItem::action("Compact View", ViewCompact),
                MenuItem::action("Table View", ViewTable),
            ]),
            Menu::new("Bookmarks").items(vec![
                MenuItem::action("Add Bookmark", AddBookmark),
                MenuItem::action("Remove Bookmark", RemoveBookmark),
            ]),
        ]);
    }

    fn register_keybindings(&self, cx: &mut ViewContext<Self>) {
        cx.bind_keys([
            KeyBinding::new("f5", Refresh, None),
            KeyBinding::new("backspace", GoUp, None),
            KeyBinding::new("delete", DeleteSelected, None),
            KeyBinding::new("shift-delete", DeletePermanent, None),
            KeyBinding::new("f2", Rename, None),
            KeyBinding::new("ctrl-a", SelectAll, None),
            KeyBinding::new("ctrl-c", Copy, None),
            KeyBinding::new("ctrl-x", Cut, None),
            KeyBinding::new("ctrl-v", Paste, None),
            KeyBinding::new("ctrl-d", Duplicate, None),
            KeyBinding::new("ctrl-f", ActivateSearch, None),
            KeyBinding::new("ctrl-l", FocusPathBar, None),
            KeyBinding::new("ctrl-t", NewTab, None),
            KeyBinding::new("ctrl-w", CloseTab, None),
            KeyBinding::new("escape", ClearSelection, None),
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
        let current = self.current_path.clone();
        self.navigate_to(current, false, cx);
    }

    fn navigate_to(&mut self, path: PathBuf, record_history: bool, cx: &mut ViewContext<Self>) {
        if record_history {
            if let Some(navigation) = self.tabs.active_navigation_mut() {
                navigation.navigate_to(path.clone());
            }
        }
        self.current_path = path.clone();
        self.path_input_text = path.to_string_lossy().to_string();
        self.selected_files.clear();
        self.loading_directory = true;
        self.set_status("Loading…", cx);

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
                this.loading_directory = false;
                match files_result {
                    Ok(Ok(mut files)) => {
                        sort::sort_files(&mut files, this.sort_column, this.sort_order);
                        this.files = files;
                        this.set_status("Ready", cx);
                        this.queue_icon_loads(cx);
                    }
                    _ => {
                        this.files.clear();
                        this.set_status("Failed to load directory", cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();

        cx.notify();
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
        cx.notify();
    }

    fn select_all_visible(&mut self, cx: &mut ViewContext<Self>) {
        self.selected_files = self
            .visible_files()
            .into_iter()
            .filter_map(|file_info| file_info.get_name().map(str::to_string))
            .collect();
        cx.notify();
    }

    fn toggle_selection(&mut self, name: &str, extend: bool, cx: &mut ViewContext<Self>) {
        if extend {
            if !self.selected_files.remove(name) {
                self.selected_files.insert(name.to_string());
            }
        } else {
            self.selected_files.clear();
            self.selected_files.insert(name.to_string());
        }
        cx.notify();
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected_files
            .iter()
            .map(|name| self.current_path.join(name))
            .collect()
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

        let destination = self.current_path.clone();
        let action_label = if is_cut { "Moving" } else { "Copying" };
        self.set_status(format!("{action_label} {} items…", sources.len()), cx);

        cx.spawn(async move |this, cx| {
            let errors = Tokio::spawn(cx, async move {
                paste_files(sources, destination, is_cut)
            })
            .await
            .unwrap_or_default();

            let status = if errors.is_empty() {
                format!("{action_label} complete")
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
        self.selected_files.clear();
        cx.notify();
    }

    fn activate_search(&mut self, cx: &mut ViewContext<Self>) {
        self.path_edit_active = false;
        self.search_active = true;
        self.set_status("Search: type to filter, Enter/Escape to finish", cx);
        cx.notify();
    }

    fn clear_search(&mut self, cx: &mut ViewContext<Self>) {
        self.search_active = false;
        self.search_query.clear();
        self.set_status("Ready", cx);
        cx.notify();
    }

    fn focus_path_bar(&mut self, cx: &mut ViewContext<Self>) {
        self.search_active = false;
        self.search_query.clear();
        self.path_edit_active = true;
        self.path_input_text = self.current_path.to_string_lossy().to_string();
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
            self.path_input_text = self.current_path.to_string_lossy().to_string();
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
                self.set_status("Bookmark added", cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    fn remove_bookmark_for_current(&mut self, cx: &mut ViewContext<Self>) {
        match bookmarks::remove_bookmark(&self.current_path) {
            Ok(()) => {
                self.bookmark_paths = bookmarks::load_bookmarks();
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
        self.icon_size = mode.icon_size();
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

        let Some(icon_service) = file_icons::FileIconService::global(cx).cloned() else {
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

        if self.path_edit_active {
            if event.keystroke.key == "escape" {
                self.path_edit_active = false;
                self.path_input_text = self.current_path.to_string_lossy().to_string();
                self.set_status("Ready", cx);
                cx.notify();
                return;
            }
            if event.keystroke.key == "enter" {
                self.submit_path_bar(cx);
                return;
            }
            if event.keystroke.key == "backspace" {
                self.path_input_text.pop();
                cx.notify();
                return;
            }
            if let Some(character) = event.keystroke.key_char.as_ref() {
                if character.len() == 1
                    && !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.platform
                {
                    self.path_input_text.push_str(character);
                    cx.notify();
                }
            }
            return;
        }

        if !self.search_active {
            return;
        }

        if event.keystroke.key == "escape" {
            self.clear_search(cx);
            return;
        }

        if event.keystroke.key == "backspace" {
            self.search_query.pop();
            cx.notify();
            return;
        }

        if let Some(character) = event.keystroke.key_char.as_ref() {
            if character.len() == 1
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.platform
            {
                self.search_query.push_str(character);
                cx.notify();
            }
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
            .on_action(cx.listener(|this, _action: &ActivateSearch, _, cx| {
                this.activate_search(cx)
            }))
            .on_action(cx.listener(|this, _action: &ClearSearch, _, cx| {
                this.clear_search(cx)
            }))
            .on_action(cx.listener(|this, _action: &FocusPathBar, _, cx| this.focus_path_bar(cx)))
            .on_action(cx.listener(|this, _action: &GoHome, _, cx| this.go_home(cx)))
            .on_action(cx.listener(|this, _action: &NewTab, _, cx| this.new_tab(cx)))
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
            .on_action(cx.listener(|this, _action: &ToggleSortOrder, _, cx| {
                this.toggle_sort_order(cx)
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
                    .child(self.render_main_panel(window, cx)),
            )
            .child(self.render_status_bar(cx))
            .when_some(self.pending_delete.clone(), |root, pending| {
                root.child(Self::render_delete_dialog(pending, cx))
            })
            .when_some(self.pending_rename.clone(), |root, pending| {
                root.child(Self::render_rename_dialog(pending, cx))
            })
    }
}

impl FilemanWindow {
    fn render_sidebar(&mut self, _window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let sidebar_width = self.config.window.splitter_pos;
        let colors = cx.theme().colors().clone();
        let places = quick_access_places();
        let bookmark_paths = self.bookmark_paths.clone();

        v_flex()
            .id("fileman-sidebar")
            .w(px(sidebar_width as f32))
            .h_full()
            .bg(colors.panel_background)
            .border_r_1()
            .border_color(colors.border)
            .p_3()
            .gap_2()
            .overflow_y_scroll()
            .child(Headline::new("Quick Access").size(HeadlineSize::XSmall))
            .child(
                v_flex()
                    .gap_0p5()
                    .children(places.into_iter().map(|(label, path)| {
                        let is_active = self.current_path == path;
                        let path_clone = path.clone();
                        let label_string = label.to_string();

                        ListItem::new(SharedString::from(format!("sidebar-{label_string}")))
                            .toggle_state(is_active)
                            .rounded()
                            .start_slot(
                                Icon::new(quick_access_icon(label))
                                    .size(IconSize::Small)
                                    .color(if is_active {
                                        Color::Selected
                                    } else {
                                        Color::Muted
                                    }),
                            )
                            .child(Label::new(label_string))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.navigate_to(path_clone.clone(), true, cx);
                            }))
                    })),
            )
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

                                ListItem::new(item_id)
                                    .toggle_state(is_active)
                                    .rounded()
                                    .start_slot(
                                        Icon::new(IconName::Star)
                                            .size(IconSize::Small)
                                            .color(if is_active {
                                                Color::Selected
                                            } else {
                                                Color::Muted
                                            }),
                                    )
                                    .child(Label::new(display_name.to_string()).truncate())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if path_clone.is_dir() {
                                            this.navigate_to(path_clone.clone(), true, cx);
                                        } else {
                                            this.set_status("Bookmark path is not a directory", cx);
                                        }
                                    }))
                            },
                        )),
                    )
            })
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
                            IconButton::new(
                                SharedString::from(format!("close-tab-{index}")),
                                IconName::Close,
                            )
                            .icon_size(IconSize::XSmall)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.close_tab_at(index, cx);
                            })),
                        )
                    })
            }))
            .child(
                IconButton::new("new-tab", IconName::Plus)
                    .icon_size(IconSize::Small)
                    .on_click(cx.listener(|this, _, _, cx| this.new_tab(cx))),
            )
    }

    fn render_toolbar(&mut self, _window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let show_hidden = self.show_hidden;
        let colors = cx.theme().colors().clone();
        let center_label = if self.path_edit_active {
            if self.path_input_text.is_empty() {
                "Path: type a directory…".to_string()
            } else {
                format!("Path: {}", self.path_input_text)
            }
        } else if self.search_active {
            if self.search_query.is_empty() {
                "Search: type to filter…".to_string()
            } else {
                format!("Search: {}", self.search_query)
            }
        } else {
            self.path_input_text.clone()
        };
        let center_border = if self.path_edit_active {
            colors.border_focused
        } else if self.search_active {
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
                                IconButton::new("go-back", IconName::ArrowLeft)
                                    .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
                            )
                            .child(
                                IconButton::new("go-forward", IconName::ArrowRight)
                                    .on_click(cx.listener(|this, _, _, cx| this.go_forward(cx))),
                            )
                            .child(
                                IconButton::new("go-up", IconName::ArrowUp)
                                    .on_click(cx.listener(|this, _, _, cx| this.go_up(cx))),
                            )
                            .child(
                                IconButton::new("refresh", IconName::ArrowCircle)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reload_current_directory(cx)
                                    })),
                            )
                            .child(
                                IconButton::new("copy", IconName::Copy)
                                    .on_click(cx.listener(|this, _, _, cx| this.copy_selected(cx))),
                            )
                            .child(
                                IconButton::new("cut", IconName::Scissors)
                                    .on_click(cx.listener(|this, _, _, cx| this.cut_selected(cx))),
                            )
                            .child(
                                IconButton::new("paste", IconName::ToolCopy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.paste_clipboard(cx)
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .bg(colors.elevated_surface_background)
                            .border_1()
                            .border_color(center_border)
                            .rounded_md()
                            .px_3()
                            .py_1()
                            .child(Label::new(center_label).truncate()),
                    )
                    .child(
                        IconButton::new("search", IconName::ToolSearch)
                            .toggle_state(self.search_active)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.search_active {
                                    this.clear_search(cx);
                                } else {
                                    this.activate_search(cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("toggle-hidden", "Hidden")
                            .style(ButtonStyle::Outlined)
                            .toggle_state(show_hidden)
                            .start_icon(Icon::new(if show_hidden {
                                IconName::Eye
                            } else {
                                IconName::EyeOff
                            }))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_hidden(cx))),
                    ),
            )
    }

    fn render_files_area(&mut self, _window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let visible_files = self.visible_files();
        let view_mode = self.view_mode;
        let colors = cx.theme().colors().clone();

        let empty_state = v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_2()
            .child(Icon::new(IconName::Folder).size(IconSize::XLarge).color(Color::Muted))
            .child(
                Label::new("This folder is empty")
                    .color(Color::Muted)
                    .size(LabelSize::Small),
            );

        let loading_state = v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .child(Label::new("Loading…").color(Color::Muted).size(LabelSize::Small));

        let file_rows: Vec<_> = visible_files
            .into_iter()
            .map(|file_info| self.render_file_entry(file_info, view_mode, cx))
            .collect();

        let scroll = div()
            .id("files-scroll-area")
            .flex_1()
            .overflow_y_scroll()
            .when(self.loading_directory, |panel| panel.child(loading_state))
            .when(!self.loading_directory && file_rows.is_empty(), |panel| {
                panel.child(empty_state)
            });

        match view_mode {
            ViewMode::Icon => scroll
                .p_2()
                .flex()
                .flex_wrap()
                .gap_2()
                .children(file_rows),
            ViewMode::List | ViewMode::Compact | ViewMode::Table => {
                let mut panel = scroll.p_2().flex().flex_col().gap_0p5();
                if view_mode == ViewMode::Table && !self.loading_directory && !file_rows.is_empty() {
                    panel = panel.child(
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
                            ),
                    );
                }
                panel.children(file_rows)
            }
        }
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
            FileIconPresentation::SvgPath(path) => svg()
                .external_path(path)
                .size(icon_size.rems())
                .flex_none()
                .text_color(icon_color.color(cx))
                .into_any_element(),
            FileIconPresentation::RasterPath(path) => Icon::from_path(path)
                .size(icon_size)
                .color(icon_color)
                .into_any_element(),
        }
    }

    fn render_file_entry(
        &self,
        file_info: &FileInfo,
        view_mode: ViewMode,
        cx: &mut ViewContext<Self>,
    ) -> AnyElement {
        let name = file_info.get_name().unwrap_or("").to_string();
        let is_directory = file_info.get_file_type() == FileType::Directory;
        let is_selected = self.selected_files.contains(&name);
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

        let click_handler = cx.listener(move |this, event: &ClickEvent, _, cx| {
            Self::handle_file_item_click(this, event, &name_for_open, is_directory, &name_for_click, cx);
        });

        match view_mode {
            ViewMode::Icon => div().w(px(88.0)).child(
                ListItem::new(item_id)
                    .toggle_state(is_selected)
                    .rounded()
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .child(icon_element)
                            .child(Label::new(name).size(LabelSize::XSmall).truncate()),
                    )
                    .on_click(click_handler),
            )
            .into_any_element(),
            ViewMode::Compact => ListItem::new(item_id)
                .toggle_state(is_selected)
                .rounded()
                .start_slot(icon_element)
                .child(Label::new(name).size(LabelSize::Small).truncate())
                .on_click(click_handler)
                .into_any_element(),
            ViewMode::Table => ListItem::new(item_id)
                .toggle_state(is_selected)
                .rounded()
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
                .into_any_element(),
            ViewMode::List => ListItem::new(item_id)
                .toggle_state(is_selected)
                .rounded()
                .start_slot(icon_element)
                .child(Label::new(name))
                .end_slot(Label::new(size_string).color(Color::Muted).size(LabelSize::XSmall))
                .on_click(click_handler)
                .into_any_element(),
        }
    }

    fn handle_file_item_click(
        this: &mut Self,
        event: &ClickEvent,
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
                cx.open_with_system(&full_path);
            }
            return;
        }

        let extend = event.modifiers().shift
            || event.modifiers().control
            || event.modifiers().platform;
        this.toggle_selection(name_for_click, extend, cx);
    }

    fn render_status_bar(&self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let selection_count = self.selected_files.len();
        let item_count = self.visible_files().len();
        let selection_summary = if selection_count == 0 {
            format!("{item_count} items")
        } else {
            format!("{selection_count} selected · {item_count} items")
        };

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
            .child(Label::new(self.status_message.clone()).size(LabelSize::Small).truncate())
            .child(
                Label::new(selection_summary)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
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

fn quick_access_icon(label: &str) -> IconName {
    match label {
        "Home" => IconName::OpenFolder,
        "Desktop" => IconName::Screen,
        "Documents" => IconName::FileDoc,
        "Downloads" => IconName::ArrowDown,
        "Music" => IconName::AudioOn,
        "Pictures" => IconName::Image,
        "Videos" => IconName::File,
        _ => IconName::Folder,
    }
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

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = FilemanConfig::load_or_create();
    let initial_path = initial_path_from_args(&config);
    let window_config = config.clone();

    let backend = Arc::new(LocalBackend::new());
    register_backend(backend);

    nptk::gpui_platform::application().run(move |cx: &mut App| {
        nptk::init(cx);

        cx.open_window(window_options(&window_config, cx), |_, cx| {
            cx.new(|cx| FilemanWindow::new(initial_path.clone(), cx))
        })
        .expect("failed to open file manager window");
    });
}
