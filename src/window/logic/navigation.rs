use crate::actions::*;
use crate::clipboard::FileClipboard;
use crate::config::FilemanConfig;
use crate::devices::VolumeMount;
use crate::drag::{drop_target_style, DraggedFilePaths, MarqueeDrag};
use crate::jobs::{count_paste_conflicts, ConflictResolution, PasteJobSettings, run_paste_batch};
use crate::location_bar::breadcrumb_segments;
use crate::navigation::NavigationState;
use crate::operations::{
    create_directory, create_file, delete_path, duplicate_path, move_to_trash, PasteResult,
    rename_path, unique_name_in_parent,
};
use crate::properties::PropertiesDialog;
use crate::search::{SearchMatch, SearchScope};
use crate::settings::{SettingsDraft, SettingsField};
use crate::sort::{SortColumn, SortOrder};
use crate::tabs::TabModel;
use crate::toolbar_input::{ToolbarLineInput, ToolbarLineInputEvent};
use crate::ui_icons::ThemeIconButton;
use crate::undo::UndoStack;
use crate::view_mode::{clamp_icon_size, ViewMode, ICON_ZOOM_STEP, MAX_ICON_SIZE, MIN_ICON_SIZE};
use crate::window::format::{
    delete_confirmation_message, format_modified, format_size, path_to_file_uri, quick_access_places,
};
use crate::window::{
    ContextMenuTarget, FilemanWindow, PendingDelete, PendingPasteChoice, PendingRename,
};
use crate::icons;
use nptk::file_icons::FileIconPresentation;
use nptk::gpui::{self as gpui, uniform_list, Entity, ScrollStrategy, Subscription, UniformListScrollHandle, *};
use nptk::gpui_tokio::Tokio;
use nptk::std::collections::HashSet;
use nptk::std::ops::Range;
use nptk::std::path::{Path, PathBuf};
use nptk::std::sync::atomic::{AtomicBool, Ordering};
use nptk::std::sync::mpsc;
use nptk::std::sync::Arc;
use nptk::std::time::Duration;
use nptk::theme::ActiveTheme;
use nptk::ui::{
    Checkbox, ContextMenu, DropdownMenu, DropdownStyle, ListItem, ToggleState, WithScrollbar,
    prelude::*,
};
use npio::{get_file_for_uri, FileInfo, FileType};

type ViewContext<'a, T> = gpui::Context<'a, T>;





impl FilemanWindow {
    pub(crate) fn add_bookmark_for_current(&mut self, cx: &mut ViewContext<Self>) {
        match crate::bookmarks::add_bookmark(&self.current_path) {
            Ok(()) => {
                self.bookmark_paths = crate::bookmarks::load_bookmarks();
                self.queue_ui_icon_loads(cx);
                self.set_status("Bookmark added", cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    pub(crate) fn apply_sort(&mut self, column: SortColumn, order: Option<SortOrder>, cx: &mut ViewContext<Self>) {
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
        crate::sort::sort_files(&mut self.files, self.sort_column, self.sort_order);
        self.set_status(format!("Sorted by {:?} ({:?})", self.sort_column, self.sort_order), cx);
        cx.notify();
    }

    pub(crate) fn close_tab(&mut self, cx: &mut ViewContext<Self>) {
        self.close_tab_at(self.tabs.active, cx);
    }

    pub(crate) fn close_tab_at(&mut self, index: usize, cx: &mut ViewContext<Self>) {
        if !self.tabs.close_at(index) {
            self.set_status("Cannot close the last tab", cx);
            return;
        }
        if let Some(path) = self.tabs.active_path() {
            self.navigate_to(path, false, cx);
        }
    }

    pub(crate) fn focus_path_bar(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
        self.search_active = false;
        self.search_query.clear();
        self.path_edit_active = true;
        self.sync_path_line_input_from_current(cx);
        self.focus_path_line_input(window, cx);
        self.set_status("Path: edit and press Enter to go, Escape to cancel", cx);
        cx.notify();
    }

    pub(crate) fn focus_path_line_input(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
        self.path_line_input.update(cx, |input, cx| {
            input.set_text(self.path_input_text.clone(), cx);
        });
        let focus_handle = self.path_line_input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    }

    pub(crate) fn go_back(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(previous) = self
            .tabs
            .active_navigation_mut()
            .and_then(|navigation| navigation.go_back())
        {
            self.navigate_to(previous, false, cx);
        }
    }

    pub(crate) fn go_forward(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(next) = self
            .tabs
            .active_navigation_mut()
            .and_then(|navigation| navigation.go_forward())
        {
            self.navigate_to(next, false, cx);
        }
    }

    pub(crate) fn go_home(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(home) = dirs::home_dir() {
            self.navigate_to(home, true, cx);
        }
    }

    pub(crate) fn go_up(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(parent) = self.current_path.parent() {
            self.navigate_to(parent.to_path_buf(), true, cx);
        }
    }

    pub(crate) fn handle_path_input_event(
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

    pub(crate) fn navigate_to(&mut self, path: PathBuf, record_history: bool, cx: &mut ViewContext<Self>) {
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

    pub fn new(initial_path: PathBuf, cx: &mut ViewContext<Self>) -> Self {
        crate::toolbar_input::register_keybindings(cx);

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
            bookmark_paths: crate::bookmarks::load_bookmarks(),
            volume_mounts: crate::devices::list_removable_mounts(),
            icon_cache: crate::icons::FileIconCache::new(),
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

    pub fn new_tab(&mut self, cx: &mut ViewContext<Self>) {
        let path = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        self.tabs.new_tab(path.clone());
        self.navigate_to(path, false, cx);
    }

    pub(crate) fn persist_window_geometry(&mut self, cx: &mut App) {
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

    pub(crate) fn queue_icon_loads(&mut self, cx: &mut ViewContext<Self>) {
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
                    crate::icons::FileIconCache::load_icon(
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

    pub(crate) fn queue_ui_icon_loads(&mut self, cx: &mut ViewContext<Self>) {
        let Some(icon_service) = nptk::file_icons::FileIconService::global(cx).cloned() else {
            return;
        };

        let mut theme_names: HashSet<String> = crate::ui_icons::TOOLBAR_THEME_ICONS
            .iter()
            .map(|name| (*name).to_string())
            .collect();

        for (label, _) in quick_access_places() {
            if let Some(name) = crate::ui_icons::quick_access_theme_icon(label) {
                theme_names.insert(name.to_string());
            }
        }

        let mut paths: Vec<PathBuf> = quick_access_places()
            .into_iter()
            .map(|(_, path)| path)
            .collect();
        paths.extend(self.volume_mounts.iter().map(|mount| mount.mount_point.clone()));
        paths.extend(self.bookmark_paths.iter().cloned());

        let toolbar_size = crate::ui_icons::TOOLBAR_ICON_PIXELS;
        let sidebar_size = crate::ui_icons::SIDEBAR_ICON_PIXELS;

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
                    crate::icons::FileIconCache::load_theme_icon(
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
                    crate::icons::FileIconCache::load_path_icon(
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

    pub(crate) fn register_keybindings(&self, cx: &mut ViewContext<Self>) {
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

    pub(crate) fn reload_current_directory(&mut self, cx: &mut ViewContext<Self>) {
        self.reload_directory_entries(true, cx);
        if self.using_subfolder_search() {
            self.schedule_subfolder_search(cx);
        }
    }

    pub(crate) fn reload_directory_entries(&mut self, show_loading: bool, cx: &mut ViewContext<Self>) {
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
                        crate::sort::sort_files(&mut files, this.sort_column, this.sort_order);
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

    pub(crate) fn reload_volume_mounts(&mut self) {
        self.volume_mounts = crate::devices::list_removable_mounts();
    }

    pub(crate) fn remove_bookmark_for_current(&mut self, cx: &mut ViewContext<Self>) {
        match crate::bookmarks::remove_bookmark(&self.current_path) {
            Ok(()) => {
                self.bookmark_paths = crate::bookmarks::load_bookmarks();
                self.queue_ui_icon_loads(cx);
                self.set_status("Bookmark removed", cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    pub(crate) fn restart_directory_watch(&mut self, cx: &mut ViewContext<Self>) {
        self.directory_watcher = None;
        let watch_path = self.current_path.clone();
        let (notify_sender, notify_receiver) = mpsc::channel();
        self.directory_watcher =
            crate::watch::create_directory_watcher(&watch_path, notify_sender);

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

    pub(crate) fn set_icon_size(&mut self, size: u32, cx: &mut ViewContext<Self>) {
        self.icon_size = clamp_icon_size(size);
        self.config.folder_view.icon_size = Some(self.icon_size);
        self.config.save();
        self.queue_icon_loads(cx);
        cx.notify();
    }

    pub(crate) fn set_view_mode(&mut self, mode: ViewMode, cx: &mut ViewContext<Self>) {
        self.view_mode = mode;
        self.icon_size = self.config.icon_size_for_mode(mode);
        self.config.folder_view.mode = mode.config_value().to_string();
        self.config.save();
        self.set_status(format!("View: {}", mode.menu_label()), cx);
        self.queue_icon_loads(cx);
        cx.notify();
    }

    pub(crate) fn spawn_new_window(&mut self, cx: &mut ViewContext<Self>) {
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

    pub(crate) fn start_volume_monitor(&mut self, cx: &mut ViewContext<Self>) {
        let (mounts_sender, mounts_receiver) = mpsc::channel();

        cx.spawn(async move |_, cx| {
            Tokio::spawn(cx, async move {
                crate::devices::run_volume_monitor_loop(|mounts| {
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

    pub(crate) fn submit_path_bar(&mut self, cx: &mut ViewContext<Self>) {
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

    pub(crate) fn switch_tab(&mut self, index: usize, cx: &mut ViewContext<Self>) {
        if self.tabs.set_active(index) {
            if let Some(path) = self.tabs.active_path() {
                self.navigate_to(path, false, cx);
            }
        }
    }

    pub(crate) fn sync_path_line_input_from_current(&mut self, cx: &mut ViewContext<Self>) {
        self.path_input_text = self.current_path.to_string_lossy().to_string();
        self.path_line_input.update(cx, |input, cx| {
            input.set_text(self.path_input_text.clone(), cx);
        });
    }

    pub(crate) fn toggle_hidden(&mut self, cx: &mut ViewContext<Self>) {
        self.show_hidden = !self.show_hidden;
        self.config.folder_view.show_hidden = self.show_hidden;
        self.config.save();
        if self.using_subfolder_search() {
            self.schedule_subfolder_search(cx);
        }
        cx.notify();
    }

    pub(crate) fn toggle_sort_order(&mut self, cx: &mut ViewContext<Self>) {
        self.sort_order = match self.sort_order {
            SortOrder::Ascending => SortOrder::Descending,
            SortOrder::Descending => SortOrder::Ascending,
        };
        self.apply_sort(self.sort_column, Some(self.sort_order), cx);
    }

    pub(crate) fn zoom_icons_in(&mut self, cx: &mut ViewContext<Self>) {
        let next = self.icon_size.saturating_add(ICON_ZOOM_STEP);
        self.set_icon_size(next, cx);
        self.set_status(format!("Icon size: {} px", self.icon_size), cx);
    }

    pub(crate) fn zoom_icons_out(&mut self, cx: &mut ViewContext<Self>) {
        let next = self.icon_size.saturating_sub(ICON_ZOOM_STEP);
        self.set_icon_size(next, cx);
        self.set_status(format!("Icon size: {} px", self.icon_size), cx);
    }

    pub(crate) fn zoom_icons_reset(&mut self, cx: &mut ViewContext<Self>) {
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

}
