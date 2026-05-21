use crate::window::imports::*;

impl FilemanWindow {
    pub(in crate::window::render) fn file_icon_element(
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
            _ => crate::ui_icons::presentation_element(presentation, icon_size, icon_color, cx),
        }
    }

    pub(in crate::window::render) fn handle_file_item_click(
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

    pub(in crate::window::render) fn handle_search_item_click(
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

    pub(in crate::window::render) fn render_file_entry(
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

    pub(in crate::window::render) fn render_file_entry_range(
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

    pub(in crate::window::render) fn render_file_icon(
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

    pub(in crate::window::render) fn render_files_area(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
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
            .child(crate::ui_icons::cached_icon_element(
                self.icon_cache
                    .cached_theme_icon(crate::ui_icons::FOLDER, crate::ui_icons::TOOLBAR_ICON_PIXELS),
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

    pub(in crate::window::render) fn render_marquee_overlay(&self, cx: &mut ViewContext<Self>) -> AnyElement {
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

    pub(in crate::window::render) fn render_search_match(
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

    pub(in crate::window::render) fn render_search_match_range(
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

}
