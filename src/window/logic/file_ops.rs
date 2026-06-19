use crate::window::logic::foreground::log_entity_update;
use crate::window::imports::*;

impl FilemanWindow {
    pub(crate) fn cancel_active_paste(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(cancel) = &self.paste_cancel {
            cancel.store(true, Ordering::Relaxed);
            self.set_status("Cancelling paste…", cx);
            cx.notify();
        }
    }

    pub(crate) fn cancel_pending_delete(&mut self, cx: &mut ViewContext<Self>) {
        self.pending_delete = None;
        cx.notify();
    }

    pub(crate) fn cancel_pending_paste(&mut self, cx: &mut ViewContext<Self>) {
        self.pending_paste_choice = None;
        self.set_status("Paste cancelled", cx);
        cx.notify();
    }

    pub(crate) fn cancel_pending_rename(&mut self, cx: &mut ViewContext<Self>) {
        self.pending_rename = None;
        self.inline_rename = None;
        self.pending_rename_collision = None;
        cx.notify();
    }

    pub(crate) fn cancel_pending_rename_collision(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(pending) = self.pending_rename_collision.take() {
            self.inline_rename = Some(PendingRename {
                path: pending.source_path,
                new_name: pending.new_name,
            });
        }
        cx.notify();
    }

    pub(crate) fn confirm_pending_rename_collision(&mut self, cx: &mut ViewContext<Self>) {
        let Some(pending) = self.pending_rename_collision.take() else {
            return;
        };
        let source_path = pending.source_path;
        let destination = pending.destination;
        let removed_existing = destination.exists();
        if removed_existing {
            if let Err(error) = remove_path_at(&destination) {
                self.set_status(error, cx);
                cx.notify();
                return;
            }
        }
        match rename_path(source_path.clone(), destination.clone()) {
            Ok(()) => {
                self.undo_stack.push_move(source_path.clone(), destination.clone());
                self.remap_selection_after_rename(&source_path, &destination);
                self.set_status("Renamed item", cx);
                self.reload_current_directory(cx);
            }
            Err(error) => {
                let message = if removed_existing {
                    format!("{error} (the previous item at the destination may have been removed)")
                } else {
                    error
                };
                self.set_status(message, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn confirm_paste_with_resolution(
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

    pub(crate) fn confirm_pending_delete(&mut self, cx: &mut ViewContext<Self>) {
        let Some(pending) = self.pending_delete.take() else {
            return;
        };
        self.perform_delete(pending.paths, pending.permanent, cx);
        cx.notify();
    }
    pub(crate) fn confirm_pending_rename(&mut self, cx: &mut ViewContext<Self>) {
        let Some(pending) = self
            .inline_rename
            .take()
            .or_else(|| self.pending_rename.take())
        else {
            return;
        };

        let new_name = pending.new_name.trim();
        if new_name.is_empty() {
            self.set_status("Name cannot be empty", cx);
            self.inline_rename = Some(pending);
            return;
        }
        if new_name.contains('/') || new_name.contains('\\') {
            self.set_status("Name cannot contain path separators", cx);
            self.inline_rename = Some(pending);
            return;
        }

        let Some(parent) = pending.path.parent() else {
            self.set_status("Invalid path", cx);
            return;
        };

        let destination = parent.join(new_name);
        let same_path = fs::canonicalize(&pending.path)
            .ok()
            .and_then(|source| {
                fs::canonicalize(&destination)
                    .ok()
                    .map(|target| source == target)
            })
            .unwrap_or(pending.path == destination);

        if destination.exists() && !same_path {
            self.pending_rename_collision = Some(PendingRenameCollision {
                source_path: pending.path,
                destination,
                new_name: new_name.to_string(),
            });
            cx.notify();
            return;
        }

        let source_path = pending.path.clone();
        match rename_path(pending.path, destination.clone()) {
            Ok(()) => {
                self.undo_stack.push_move(source_path.clone(), destination.clone());
                self.remap_selection_after_rename(&source_path, &destination);
                self.set_status("Renamed item", cx);
                self.reload_current_directory(cx);
            }
            Err(error) => self.set_status(error, cx),
        }
        cx.notify();
    }

    fn cancel_in_flight_paste(&mut self) {
        if let Some(cancel) = &self.paste_cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    fn start_paste_from_sources(
        &mut self,
        sources: Vec<PathBuf>,
        destination_directory: PathBuf,
        is_cut: bool,
        cx: &mut ViewContext<Self>,
    ) {
        let sources =
            crate::drag::filter_paste_sources(sources, &destination_directory, is_cut);
        if sources.is_empty() {
            self.set_status("Cannot paste into this folder", cx);
            cx.notify();
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

        self.execute_paste(sources, destination_directory, is_cut, PasteJobSettings::default(), cx);
    }

    pub(crate) fn confirm_settings(&mut self, cx: &mut ViewContext<Self>) {
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

    pub(crate) fn copy_selected(&mut self, cx: &mut ViewContext<Self>) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.set_status("Nothing selected to copy", cx);
            return;
        }
        self.clipboard.set_files(paths.clone(), false);
        cx.write_to_clipboard(ClipboardItem::new_file_paths(paths, false));
        self.set_status("Copied to clipboard", cx);
    }

    pub(crate) fn create_file(&mut self, cx: &mut ViewContext<Self>) {
        let destination = unique_name_in_parent(&self.current_path, "New File");
        match create_file(destination) {
            Ok(()) => {
                self.set_status("Created file", cx);
                self.reload_current_directory(cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    pub(crate) fn create_folder(&mut self, cx: &mut ViewContext<Self>) {
        let destination = unique_name_in_parent(&self.current_path, "New Folder");
        match create_directory(destination) {
            Ok(()) => {
                self.set_status("Created folder", cx);
                self.reload_current_directory(cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    pub(crate) fn cut_selected(&mut self, cx: &mut ViewContext<Self>) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.set_status("Nothing selected to cut", cx);
            return;
        }
        self.clipboard.set_files(paths.clone(), true);
        cx.write_to_clipboard(ClipboardItem::new_file_paths(paths, true));
        self.set_status("Cut to clipboard", cx);
    }

    pub(crate) fn delete_selected(&mut self, cx: &mut ViewContext<Self>) {
        self.request_delete(false, cx);
    }

    pub(crate) fn deploy_context_menu(
        &mut self,
        position: Point<Pixels>,
        target: ContextMenuTarget,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        self.dismiss_context_menu();
        let focus_handle = self.focus_handle.clone();
        let has_selection = !self.selected_indices.is_empty();
        let selected_paths = self.selected_paths();
        let open_action_label = crate::open::open_label_for_path(
            selected_paths
                .first()
                .map(|path| path.as_path())
                .unwrap_or(Path::new("")),
        );
        let open_with_handlers = selected_paths
            .first()
            .map(|path| crate::open::handlers_for_path(path))
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
                                    log_entity_update(
                                        "open_with_application",
                                        weak.update(cx, |this, cx| {
                                            this.launch_file_with_application(
                                                application_id.clone(),
                                                path.clone(),
                                                cx,
                                            );
                                        }),
                                    );
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

    pub(crate) fn dismiss_about(&mut self, cx: &mut ViewContext<Self>) {
        self.show_about = false;
        cx.notify();
    }

    pub(crate) fn dismiss_context_menu(&mut self) {
        self.context_menu = None;
    }

    pub(crate) fn dismiss_properties(&mut self, cx: &mut ViewContext<Self>) {
        self.pending_properties = None;
        cx.notify();
    }

    pub(crate) fn dismiss_settings(&mut self, cx: &mut ViewContext<Self>) {
        self.pending_settings = None;
        self.settings_terminal_focus = false;
        cx.notify();
    }

    pub(crate) fn drop_external_files(&mut self, paths: &gpui::ExternalPaths, cx: &mut ViewContext<Self>) {
        let sources = paths.paths().to_vec();
        if sources.is_empty() {
            return;
        }
        self.start_paste_from_sources(sources, self.current_path.clone(), false, cx);
    }

    pub(crate) fn drop_external_into_directory(
        &mut self,
        destination_directory: &Path,
        paths: &gpui::ExternalPaths,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        let sources = crate::drag::filter_sources_for_destination(
            &paths.paths().to_vec(),
            destination_directory,
            false,
        );
        if sources.is_empty() {
            self.set_status("Cannot copy into this folder", cx);
            return;
        }
        self.offer_drop_into_directory(
            sources,
            destination_directory.to_path_buf(),
            window.mouse_position(),
            false,
            window,
            cx,
        );
    }

    pub(crate) fn drop_internal_files(
        &mut self,
        dragged: &DraggedFilePaths,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        let sources = crate::drag::filter_sources_for_destination(
            &dragged.paths,
            &self.current_path,
            true,
        );
        if sources.is_empty() {
            self.set_status("Items are already in this folder", cx);
            return;
        }
        self.offer_drop_into_directory(
            sources,
            self.current_path.clone(),
            window.mouse_position(),
            true,
            window,
            cx,
        );
    }

    pub(crate) fn drop_into_directory(
        &mut self,
        destination_directory: &Path,
        dragged: &DraggedFilePaths,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        let sources =
            crate::drag::filter_sources_for_destination(&dragged.paths, destination_directory, true);
        if sources.is_empty() {
            self.set_status("Cannot move into this folder", cx);
            return;
        }
        self.offer_drop_into_directory(
            sources,
            destination_directory.to_path_buf(),
            window.mouse_position(),
            true,
            window,
            cx,
        );
    }

    pub(crate) fn offer_drop_into_directory(
        &mut self,
        sources: Vec<PathBuf>,
        destination_directory: PathBuf,
        position: Point<Pixels>,
        allow_move: bool,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        if sources.is_empty() {
            return;
        }
        self.deploy_drop_choice_menu(
            position,
            sources,
            destination_directory,
            allow_move,
            window,
            cx,
        );
    }

    pub(crate) fn deploy_drop_choice_menu(
        &mut self,
        position: Point<Pixels>,
        sources: Vec<PathBuf>,
        destination_directory: PathBuf,
        allow_move: bool,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        self.dismiss_context_menu();
        let focus_handle = self.focus_handle.clone();
        let window_weak = cx.weak_entity();

        let context_menu = ContextMenu::build(window, cx, move |menu, _, _| {
            let weak_for_move = window_weak.clone();
            let weak_for_copy = window_weak.clone();
            let weak_for_cancel = window_weak.clone();
            let sources_for_move = sources.clone();
            let sources_for_copy = sources.clone();
            let destination_for_move = destination_directory.clone();
            let destination_for_copy = destination_directory.clone();
            let mut menu = menu.context(focus_handle.clone());
            if allow_move {
                menu = menu.entry("Move here", None, move |_, cx| {
                    log_entity_update(
                        "drop_paste_move",
                        weak_for_move.update(cx, |this, cx| {
                            this.paste_dropped_files(
                                sources_for_move.clone(),
                                destination_for_move.clone(),
                                true,
                                cx,
                            );
                        }),
                    );
                });
            }
            menu = menu.entry("Copy here", None, move |_, cx| {
                    log_entity_update(
                        "drop_paste_copy",
                        weak_for_copy.update(cx, |this, cx| {
                            this.paste_dropped_files(
                                sources_for_copy.clone(),
                                destination_for_copy.clone(),
                                false,
                                cx,
                            );
                        }),
                    );
                });
            if allow_move {
                menu = menu.separator();
            }
            menu.entry("Cancel", None, move |_, cx| {
                    log_entity_update(
                        "drop_paste_cancel",
                        weak_for_cancel.update(cx, |this, cx| {
                            this.dismiss_context_menu();
                            cx.notify();
                        }),
                    );
                })
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

    pub(crate) fn duplicate_selected(&mut self, cx: &mut ViewContext<Self>) {
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

            log_entity_update(
                "paste_dropped_complete",
                this.update(cx, |this, cx| {
                    this.set_status(status, cx);
                    this.reload_current_directory(cx);
                }),
            );
        })
        .detach();
    }

    pub(crate) fn execute_paste(
        &mut self,
        sources: Vec<PathBuf>,
        destination_directory: PathBuf,
        is_cut: bool,
        settings: PasteJobSettings,
        cx: &mut ViewContext<Self>,
    ) {
        let action_label = if is_cut { "Moving" } else { "Copying" };
        self.cancel_in_flight_paste();
        self.paste_generation = self.paste_generation.wrapping_add(1);
        let generation = self.paste_generation;
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
                log_entity_update(
                    "paste_progress",
                    this.update(cx, |this, cx| {
                        this.set_status(
                            format!("{action_label} {current}/{total}: {file_name}"),
                            cx,
                        );
                    }),
                );
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

            let moved_count = combined.recorded_moves.len();
            let status = if combined.cancelled {
                if combined.errors.is_empty() {
                    if moved_count > 0 {
                        format!("Paste cancelled after {moved_count} of {total} items")
                    } else {
                        "Paste cancelled".to_string()
                    }
                } else {
                    format!("Paste cancelled; {}", combined.errors.join("; "))
                }
            } else if combined.errors.is_empty() {
                format!("{action_label} complete")
            } else {
                combined.errors.join("; ")
            };

            log_entity_update(
                "paste_complete",
                this.update(cx, |this, cx| {
                    if generation != this.paste_generation {
                        return;
                    }
                    this.paste_cancel = None;
                    let cut_should_clear =
                        is_cut && cut_clipboard_should_clear_after_paste(&combined);
                    for (source, destination) in combined.recorded_moves {
                        this.undo_stack.push_move(source, destination);
                    }
                    if cut_should_clear {
                        this.clipboard.clear();
                        cx.write_to_clipboard(ClipboardItem::new_file_paths(Vec::new(), false));
                    }
                    this.set_status(status, cx);
                    this.reload_current_directory(cx);
                }),
            );
        })
        .detach();
    }

    pub(crate) fn file_type_for_path(&self, path: &Path) -> FileType {
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

    pub(crate) fn focus_settings_terminal(&mut self, cx: &mut ViewContext<Self>) {
        if self.pending_settings.is_some() {
            self.settings_terminal_focus = true;
            cx.notify();
        }
    }

    pub(crate) fn go_to_parent_of_selection(&mut self, cx: &mut ViewContext<Self>) {
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

    pub(crate) fn handle_rename_dialog_key(&mut self, event: &KeyDownEvent, cx: &mut ViewContext<Self>) {
        let Some(pending) = self
            .inline_rename
            .as_mut()
            .or_else(|| self.pending_rename.as_mut())
        else {
            return;
        };

        if event.keystroke.key == "escape" {
            self.inline_rename = None;
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

    pub(crate) fn handle_settings_dialog_key(&mut self, event: &KeyDownEvent, cx: &mut ViewContext<Self>) {
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

    pub(crate) fn handle_toolbar_input_key(&mut self, event: &KeyDownEvent, cx: &mut ViewContext<Self>) {
        if self.pending_rename_collision.is_some() {
            if event.keystroke.key == "escape" {
                self.cancel_pending_rename_collision(cx);
            }
            return;
        }

        if self.inline_rename.is_some() || self.pending_rename.is_some() {
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

    pub(crate) fn launch_file(&mut self, path: &Path, cx: &mut ViewContext<Self>) {
        let path = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let result = Tokio::spawn(cx, async move { crate::open::launch_path(path).await }).await;
            let message = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(error.to_string()),
            };
            if let Some(message) = message {
                log_entity_update(
                    "launch_file_status",
                    this.update(cx, |this, cx| {
                        this.set_status(message, cx);
                    }),
                );
            }
        })
        .detach();
    }

    pub(crate) fn launch_file_with_application(
        &mut self,
        application_id: String,
        path: PathBuf,
        cx: &mut ViewContext<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result =
                Tokio::spawn(cx, async move { crate::open::launch_with_application(&application_id, path).await })
                    .await;
            let message = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(error.to_string()),
            };
            if let Some(message) = message {
                log_entity_update(
                    "launch_with_application_status",
                    this.update(cx, |this, cx| {
                        this.set_status(message, cx);
                    }),
                );
            }
        })
        .detach();
    }

    pub(crate) fn open_about(&mut self, cx: &mut ViewContext<Self>) {
        self.show_about = true;
        cx.notify();
    }

    pub(crate) fn open_primary_selection(&mut self, cx: &mut ViewContext<Self>) {
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

    pub(crate) fn open_selection_with_system(&mut self, cx: &mut ViewContext<Self>) {
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

    pub(crate) fn open_settings(&mut self, cx: &mut ViewContext<Self>) {
        self.pending_settings = Some(SettingsDraft::from_config(&self.config));
        self.settings_terminal_focus = false;
        cx.notify();
    }

    pub(crate) fn open_terminal_here(&mut self, cx: &mut ViewContext<Self>) {
        let directory = self.current_path.clone();
        let terminal_command = self.config.terminal_command().map(str::to_string);
        match crate::terminal::open_terminal_in_directory(&directory, terminal_command.as_deref()) {
            Ok(()) => self.set_status("Opened terminal", cx),
            Err(error) => self.set_status(error, cx),
        }
    }

    pub(crate) fn paste_clipboard(&mut self, cx: &mut ViewContext<Self>) {
        let job = cx
            .read_from_clipboard()
            .and_then(|clipboard| clipboard.file_paths())
            .or_else(|| self.clipboard.take_files());

        let Some((sources, is_cut)) = job else {
            self.set_status("Clipboard is empty", cx);
            return;
        };

        self.start_paste_from_sources(sources, self.current_path.clone(), is_cut, cx);
    }

    pub(crate) fn paste_dropped_files(
        &mut self,
        sources: Vec<PathBuf>,
        destination_directory: PathBuf,
        is_cut: bool,
        cx: &mut ViewContext<Self>,
    ) {
        if sources.is_empty() {
            return;
        }

        self.start_paste_from_sources(sources, destination_directory, is_cut, cx);
    }

    pub(crate) fn perform_delete(&mut self, paths: Vec<PathBuf>, permanent: bool, cx: &mut ViewContext<Self>) {
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

            log_entity_update(
                "delete_complete",
                this.update(cx, |this, cx| {
                    this.set_status(status, cx);
                    this.reload_current_directory(cx);
                }),
            );
        })
        .detach();
    }

    pub(crate) fn prepare_context_selection(&mut self, visible_index: usize, cx: &mut ViewContext<Self>) {
        if !self.selected_indices.contains(&visible_index) {
            self.selected_indices.clear();
            self.selected_indices.insert(visible_index);
            cx.notify();
        }
    }

    pub(crate) fn redo_last(&mut self, cx: &mut ViewContext<Self>) {
        match self.undo_stack.redo_one() {
            Ok(()) => {
                self.set_status("Redone", cx);
                self.reload_current_directory(cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    pub(crate) fn request_delete(&mut self, permanent: bool, cx: &mut ViewContext<Self>) {
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

    pub(crate) fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected_indices
            .iter()
            .filter_map(|&index| self.path_for_visible_index(index))
            .collect()
    }

    pub(crate) fn set_status(&mut self, message: impl Into<SharedString>, cx: &mut ViewContext<Self>) {
        self.status_message = message.into();
        cx.notify();
    }

    pub(crate) fn show_properties_for_selection(&mut self, cx: &mut ViewContext<Self>) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.set_status("Nothing selected for properties", cx);
            return;
        }
        let mut dialog = match crate::properties::properties_for_paths(&paths) {
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
                    .cached_icon(path, crate::properties::PROPERTIES_ICON_SIZE)
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
                crate::properties::mime_kind_row(&path_for_kind).await
            })
            .await
            .ok()
            .flatten();

            if let Some(kind_row) = kind_row {
                log_entity_update(
                    "properties_kind_row",
                    this.update(cx, |this, cx| {
                        if let Some(dialog) = this.pending_properties.as_mut() {
                            crate::properties::insert_kind_row(dialog, kind_row);
                            cx.notify();
                        }
                    }),
                );
            }

            let icon_size = crate::properties::PROPERTIES_ICON_SIZE;
            let path_for_icon = path.clone();
            let presentation = Tokio::spawn(cx, async move {
                crate::icons::FileIconCache::load_icon(
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
            log_entity_update(
                "properties_icon",
                this.update(cx, |this, cx| {
                    if let Some(dialog) = this.pending_properties.as_mut() {
                        if dialog.icon.is_none() {
                            dialog.icon = Some(presentation.clone());
                        }
                        this.icon_cache
                            .store_icon(path_for_cache, icon_size, presentation);
                        cx.notify();
                    }
                }),
            );
        })
        .detach();
    }

    pub(crate) fn start_rename_selected(&mut self, cx: &mut ViewContext<Self>) {
        let Some(visible_index) = self.selected_indices.iter().copied().next() else {
            self.set_status("Select a single item to rename", cx);
            return;
        };

        if self.selected_indices.len() != 1 {
            self.set_status("Select a single item to rename", cx);
            return;
        }

        let Some(path) = self.path_for_visible_index(visible_index) else {
            self.set_status("Select a single item to rename", cx);
            return;
        };
        let new_name = path
            .file_name()
            .and_then(|segment| segment.to_str())
            .unwrap_or("")
            .to_string();

        self.list_focus_index = Some(visible_index);
        self.inline_rename = Some(PendingRename { path, new_name });
        self.pending_rename = None;
        cx.notify();
    }

    pub(crate) fn toggle_settings_field(&mut self, field: SettingsField, cx: &mut ViewContext<Self>) {
        let Some(draft) = self.pending_settings.as_mut() else {
            return;
        };
        draft.toggle(field);
        cx.notify();
    }

    pub(crate) fn undo_last(&mut self, cx: &mut ViewContext<Self>) {
        match self.undo_stack.undo_one() {
            Ok(()) => {
                self.set_status("Undone", cx);
                self.reload_current_directory(cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

}
