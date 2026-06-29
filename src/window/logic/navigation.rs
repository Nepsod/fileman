use std::cell::RefCell;
use std::sync::Arc;

use crate::window::logic::foreground::log_entity_update;
use crate::window::imports::*;

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
        self.rebuild_visible_file_indices();
        self.invalidate_icon_label_layout_cache();
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
        self.selected_indices.clear();
        self.selection_anchor = None;
        self.list_focus_index = None;
        self.bump_selection_generation();
        self.search_matches.clear();
        self.invalidate_icon_label_layout_cache();
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
            selected_indices: HashSet::new(),
            list_data_generation: 0,
            selection_generation: 0,
            selection_paths_cache: RefCell::new(None),
            icons_in_flight: RefCell::new(HashSet::new()),
            visible_display_names: Vec::new(),
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
            pending_rename_collision: None,
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
            search_generation: 0,
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
            paste_generation: 0,
            files_scroll_handle: UniformListScrollHandle::new(),
            uniform_list_row_height: None,
            marquee_drag: None,
            marquee_cancel_subscription: None,
            marquee_autoscroll_task: None,
            icon_label_layout_cache: Vec::new(),
            icon_label_layout_cache_key: None,
            list_visible_range: None,
            visible_file_indices: Vec::new(),
            tile_visible_index_range: None,
            last_tile_scroll_top_bits: None,
            last_tile_range_fingerprint: None,
            inline_rename: None,
            sidebar_resize_drag: None,
            sidebar_resize_cancel_subscription: None,
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

    pub(crate) fn file_icon_cache_size(&self) -> u32 {
        match self.view_mode {
            ViewMode::Compact => COMPACT_TILE_ICON_PX,
            _ => self.icon_size,
        }
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

                log_entity_update(
                    "store_theme_icon_toolbar",
                    this.update(cx, |this, cx| {
                        this.icon_cache.store_theme_icon(icon_name, toolbar_size, presentation);
                        cx.notify();
                    }),
                );
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

                log_entity_update(
                    "store_sidebar_icon",
                    this.update(cx, |this, cx| {
                        this.icon_cache
                            .store_icon(path, sidebar_size, presentation);
                        cx.notify();
                    }),
                );
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

            log_entity_update(
                "reload_directory",
                this.update(cx, |this, cx| {
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
                            this.rebuild_visible_file_indices();
                            this.invalidate_icon_label_layout_cache();
                            this.prune_selection_to_visible();
                            if show_loading && !this.using_subfolder_search() {
                                this.set_status("Ready", cx);
                            }
                            this.queue_icon_loads(cx);
                        }
                        _ => {
                            if show_loading {
                                this.files.clear();
                                this.rebuild_visible_file_indices();
                                this.set_status("Failed to load directory", cx);
                            }
                        }
                    }
                    cx.notify();
                }),
            );
        })
        .detach();
    }

    pub(crate) fn reload_sidebar_state(&mut self, cx: &mut ViewContext<Self>) {
        self.bookmark_paths = crate::bookmarks::load_bookmarks();
        self.queue_ui_icon_loads(cx);
    }

    pub(crate) fn remove_bookmark_for_current(&mut self, cx: &mut ViewContext<Self>) {
        if !crate::bookmarks::is_bookmarked(&self.current_path, &self.bookmark_paths) {
            self.set_status("Current folder is not bookmarked", cx);
            return;
        }
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

                    log_entity_update(
                        "directory_watch_reload",
                        this.update(cx, |this, cx| {
                            if this.current_path == watch_path {
                                this.reload_directory_entries(false, cx);
                            }
                        }),
                    );
                }

                cx.background_executor().timer(POLL_INTERVAL).await;
            }
        })
        .detach();
    }

    pub(crate) fn set_icon_size(&mut self, size: u32, cx: &mut ViewContext<Self>) {
        self.invalidate_icon_label_layout_cache();
        self.icon_size = clamp_icon_size(size);
        self.config.folder_view.icon_size = Some(self.icon_size);
        self.config.save();
        self.queue_icon_loads(cx);
        cx.notify();
    }

    pub(crate) fn set_view_mode(&mut self, mode: ViewMode, cx: &mut ViewContext<Self>) {
        self.view_mode = mode;
        self.uniform_list_row_height = None;
        self.invalidate_icon_label_layout_cache();
        self.icon_size = self.config.icon_size_for_mode(mode);
        self.config.folder_view.mode = mode.config_value().to_string();
        self.config.save();
        self.files_scroll_handle
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(0.)));
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
                    log_entity_update(
                        "volume_mounts_update",
                        this.update(cx, |this, cx| {
                            this.volume_mounts = mounts;
                            this.queue_ui_icon_loads(cx);
                            cx.notify();
                        }),
                    );
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
        self.rebuild_visible_file_indices();
        self.invalidate_icon_label_layout_cache();
        if self.using_subfolder_search() {
            self.schedule_subfolder_search(cx);
        }
        cx.notify();
    }

    pub(crate) fn queue_icon_loads_for_range(
        &mut self,
        index_range: std::ops::Range<usize>,
        cx: &mut ViewContext<Self>,
    ) {
        const ICON_LOAD_CONCURRENCY: usize = 6;

        let icon_size = self.file_icon_cache_size();
        let mut pending: Vec<(Arc<Path>, FileType)> = Vec::new();
        {
            let mut icons_in_flight = self.icons_in_flight.borrow_mut();
            if self.using_subfolder_search() {
                for entry_index in index_range {
                    let Some(search_match) = self.search_matches.get(entry_index) else {
                        continue;
                    };
                    let path = search_match.path.as_path();
                    let flight_key = (search_match.path.clone(), icon_size);
                    if icons_in_flight.contains(&flight_key)
                        || self.icon_cache.cached_icon(path, icon_size).is_some()
                    {
                        continue;
                    }
                    icons_in_flight.insert(flight_key);
                    let file_type = if search_match.is_directory {
                        FileType::Directory
                    } else {
                        FileType::Regular
                    };
                    pending.push((Arc::from(path), file_type));
                }
            } else {
                for entry_index in index_range {
                    let Some(file_info) = self.visible_file_at(entry_index) else {
                        continue;
                    };
                    let Some(name) = file_info.get_name() else {
                        continue;
                    };
                    if name.is_empty() {
                        continue;
                    }
                    let path = self.current_path.join(name);
                    let flight_key = (path.clone(), icon_size);
                    if icons_in_flight.contains(&flight_key)
                        || self.icon_cache.cached_icon(&path, icon_size).is_some()
                    {
                        continue;
                    }
                    icons_in_flight.insert(flight_key);
                    pending.push((Arc::from(path), file_info.get_file_type()));
                }
            }
        }

        if pending.is_empty() {
            return;
        }

        let use_thumbnails = matches!(self.view_mode, ViewMode::Icon | ViewMode::Compact);

        let Some(icon_service) = nptk::file_icons::FileIconService::global(cx).cloned() else {
            return;
        };

        cx.spawn(async move |this, cx| {
            for chunk in pending.chunks(ICON_LOAD_CONCURRENCY) {
                let mut load_tasks = Vec::with_capacity(chunk.len());
                for (path, file_type) in chunk {
                    let path = Arc::clone(path);
                    let file_type = *file_type;
                    let icon_service = icon_service.clone();
                    load_tasks.push((
                        Arc::clone(&path),
                        Tokio::spawn(cx, async move {
                            crate::icons::FileIconCache::load_icon(
                                &icon_service,
                                path.as_ref().to_path_buf(),
                                icon_size,
                                file_type,
                                use_thumbnails,
                            )
                            .await
                        }),
                    ));
                }

                for (path, task) in load_tasks {
                    let loaded_icon = task.await.ok().flatten();
                    let path_buf = path.as_ref().to_path_buf();
                    log_entity_update(
                        "store_file_icon",
                        this.update(cx, |this, cx| {
                            this.icons_in_flight
                                .borrow_mut()
                                .remove(&(path_buf.clone(), icon_size));
                            if let Some(icon) = loaded_icon {
                                this.icon_cache.store_icon(path_buf, icon_size, icon);
                                cx.notify();
                            }
                        }),
                    );
                }
            }
        })
        .detach();
    }

    pub(crate) fn queue_icon_loads(&mut self, cx: &mut ViewContext<Self>) {
        let item_count = self.visible_file_count();
        if item_count == 0 {
            return;
        }

        let index_range = if self.uses_tile_grid() {
            self.update_tile_visible_index_range(item_count);
            self.tile_visible_index_range(item_count)
        } else if let Some(range) = self.list_visible_range.clone() {
            range
        } else {
            0..item_count
        };
        self.queue_icon_loads_for_range(index_range, cx);
    }

    pub(crate) fn begin_sidebar_resize(
        &mut self,
        pointer_x: Pixels,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        self.sidebar_resize_drag = Some((pointer_x.as_f32(), self.config.window.splitter_pos));
        self.sidebar_resize_cancel_subscription = Some(cx.observe_window_activation(
            window,
            |this, _, cx| {
                if this.sidebar_resize_drag.is_some() {
                    this.finish_sidebar_resize(cx);
                }
            },
        ));
    }

    pub(crate) fn update_sidebar_resize(&mut self, pointer_x: Pixels, cx: &mut ViewContext<Self>) {
        let Some((start_x, start_width)) = self.sidebar_resize_drag else {
            return;
        };
        let delta = pointer_x.as_f32() - start_x;
        let new_width = ((start_width as f32) + delta)
            .round()
            .clamp(
                crate::config::SIDEBAR_MIN_WIDTH as f32,
                crate::config::SIDEBAR_MAX_WIDTH as f32,
            ) as u32;
        if new_width != self.config.window.splitter_pos {
            self.config.window.splitter_pos = new_width;
            self.invalidate_icon_label_layout_cache();
            cx.notify();
        }
    }

    pub(crate) fn finish_sidebar_resize(&mut self, cx: &mut ViewContext<Self>) {
        self.sidebar_resize_cancel_subscription = None;
        if self.sidebar_resize_drag.take().is_some() {
            self.config.save();
            self.persist_window_geometry(cx);
        }
    }

    pub(crate) fn toggle_sort_order(&mut self, cx: &mut ViewContext<Self>) {
        self.sort_order = match self.sort_order {
            SortOrder::Ascending => SortOrder::Descending,
            SortOrder::Descending => SortOrder::Ascending,
        };
        self.apply_sort(self.sort_column, Some(self.sort_order), cx);
    }

    pub(crate) fn zoom_icons_in(&mut self, cx: &mut ViewContext<Self>) {
        if self.view_mode != ViewMode::Icon {
            return;
        }
        let next = self.icon_size.saturating_add(ICON_ZOOM_STEP);
        self.set_icon_size(next, cx);
        self.set_status(format!("Icon size: {} px", self.icon_size), cx);
    }

    pub(crate) fn zoom_icons_out(&mut self, cx: &mut ViewContext<Self>) {
        if self.view_mode != ViewMode::Icon {
            return;
        }
        let next = self.icon_size.saturating_sub(ICON_ZOOM_STEP);
        self.set_icon_size(next, cx);
        self.set_status(format!("Icon size: {} px", self.icon_size), cx);
    }

    pub(crate) fn zoom_icons_reset(&mut self, cx: &mut ViewContext<Self>) {
        if self.view_mode != ViewMode::Icon {
            return;
        }
        self.invalidate_icon_label_layout_cache();
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
