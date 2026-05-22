use crate::window::imports::*;

#[derive(Clone)]
enum FileTileInteraction {
    Entry {
        entry_index: usize,
        name_for_open: String,
        name_for_click: String,
        is_directory: bool,
        context_name: String,
    },
    Search {
        entry_index: usize,
        path: PathBuf,
        selection_key: String,
        is_directory: bool,
    },
}

fn file_list_item(item_id: SharedString, is_selected: bool, is_focused: bool) -> ListItem {
    ListItem::new(item_id)
        .spacing(ListItemSpacing::ExtraDense)
        .toggle_state(is_selected)
        .focused(is_focused)
        .rounded()
}

use crate::icon_label_layout::{
    icon_view_label_layout, IconViewLabelLayout, ICON_LABEL_MAX_LINES_UNSELECTED,
};

fn tile_grid_rows(mut tiles: Vec<AnyElement>, columns: usize, gap: Pixels) -> Vec<AnyElement> {
    let columns = columns.max(1);
    let mut rows = Vec::new();
    while !tiles.is_empty() {
        let chunk_len = columns.min(tiles.len());
        let row_tiles: Vec<_> = tiles.drain(..chunk_len).collect();
        rows.push(
            h_flex()
                .gap(gap)
                .items_start()
                .children(row_tiles)
                .into_any_element(),
        );
    }
    rows
}

fn tile_icon_content(icon_element: AnyElement, icon_pixel_size: f32) -> impl IntoElement {
    div()
        .w(px(icon_pixel_size))
        .h(px(icon_pixel_size))
        .flex()
        .items_center()
        .justify_center()
        .child(icon_element)
}

fn tile_part_shell(
    part_id: impl Into<ElementId>,
    content: impl IntoElement,
    is_selected: bool,
    is_focused: bool,
    shell_padding: Pixels,
    reserve_focus_border: bool,
    click_handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    context_handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Stateful<Div> {
    let colors = cx.theme().colors();
    let mut shell = div()
        .id(part_id)
        .rounded_sm()
        .p(shell_padding)
        .cursor_pointer()
        .on_click(click_handler)
        .on_mouse_down(MouseButton::Right, context_handler)
        .when(is_selected, |this| {
            this.bg(colors.element_selection_background.opacity(0.9))
        })
        .when(!is_selected, |this| {
            this.hover(|style| style.bg(colors.ghost_element_hover))
        });

    if reserve_focus_border {
        shell = shell
            .border_1()
            .border_color(if is_focused {
                colors.border_focused
            } else {
                gpui::transparent_black()
            });
    } else if is_focused {
        shell = shell.border_1().border_color(colors.border_focused);
    }

    shell.child(content)
}

fn icon_tile_icon_shell(
    part_id: impl Into<ElementId>,
    icon_element: AnyElement,
    icon_pixel_size: f32,
    is_selected: bool,
    click_handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    context_handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Stateful<Div> {
    let colors = cx.theme().colors();
    let icon_size = px(icon_pixel_size);
    let mut shell = div()
        .id(part_id)
        .relative()
        .w(icon_size)
        .h(icon_size)
        .flex_shrink_0()
        .cursor_pointer()
        .on_click(click_handler)
        .on_mouse_down(MouseButton::Right, context_handler);

    if is_selected {
        shell = shell.child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .w(icon_size)
                .h(icon_size)
                .rounded_sm()
                .bg(colors.element_selection_background.opacity(0.9)),
        );
    } else {
        shell = shell.hover(|style| style.bg(colors.ghost_element_hover));
    }

    shell.child(
        div()
            .w(icon_size)
            .h(icon_size)
            .flex()
            .items_center()
            .justify_center()
            .child(icon_element),
    )
}

fn table_column_label(text: &'static str) -> Label {
    Label::new(text).size(LabelSize::XSmall).color(Color::Muted)
}

impl FilemanWindow {
    fn render_table_row_columns(
        &self,
        size_string: String,
        type_string: String,
        modified_string: String,
    ) -> impl IntoElement {
        h_flex()
            .gap_2()
            .child(
                div().w(px(TABLE_COLUMN_SIZE_PX)).child(
                    Label::new(size_string)
                        .color(Color::Muted)
                        .size(LabelSize::XSmall)
                        .truncate(),
                ),
            )
            .child(
                div().w(px(TABLE_COLUMN_TYPE_PX)).child(
                    Label::new(type_string)
                        .color(Color::Muted)
                        .size(LabelSize::XSmall)
                        .truncate(),
                ),
            )
            .child(
                div().w(px(TABLE_COLUMN_MODIFIED_PX)).child(
                    Label::new(modified_string)
                        .color(Color::Muted)
                        .size(LabelSize::XSmall)
                        .truncate(),
                ),
            )
    }

    fn render_table_sortable_header(&self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let active_column = self.sort_column;
        let active_order = self.sort_order;
        h_flex()
            .id("fileman-table-header")
            .h(px(TABLE_HEADER_HEIGHT_PX))
            .px_2()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(self.render_table_header_column(
                SortColumn::Name,
                "Name",
                None,
                active_column,
                active_order,
                cx,
            ))
            .child(self.render_table_header_column(
                SortColumn::Size,
                "Size",
                Some(px(TABLE_COLUMN_SIZE_PX)),
                active_column,
                active_order,
                cx,
            ))
            .child(self.render_table_header_column(
                SortColumn::Type,
                "Type",
                Some(px(TABLE_COLUMN_TYPE_PX)),
                active_column,
                active_order,
                cx,
            ))
            .child(self.render_table_header_column(
                SortColumn::Modified,
                "Modified",
                Some(px(TABLE_COLUMN_MODIFIED_PX)),
                active_column,
                active_order,
                cx,
            ))
    }

    fn render_table_header_column(
        &self,
        column: SortColumn,
        label: &'static str,
        width: Option<Pixels>,
        active_column: SortColumn,
        active_order: SortOrder,
        cx: &mut ViewContext<Self>,
    ) -> Stateful<Div> {
        let is_active = column == active_column;
        let sort_icon = match active_order {
            SortOrder::Ascending => IconName::ArrowUp,
            SortOrder::Descending => IconName::ArrowDown,
        };

        let mut header_cell = div()
            .id(SharedString::from(format!("table-header-{label}")))
            .h_full()
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                let order = if this.sort_column == column {
                    Some(match this.sort_order {
                        SortOrder::Ascending => SortOrder::Descending,
                        SortOrder::Descending => SortOrder::Ascending,
                    })
                } else {
                    Some(SortOrder::Ascending)
                };
                this.apply_sort(column, order, cx);
            }))
            .child(table_column_label(label))
            .when(is_active, |this| {
                this.child(
                    Icon::new(sort_icon)
                        .size(IconSize::XSmall)
                        .color(Color::Default),
                )
            });

        if let Some(width) = width {
            header_cell = header_cell.w(width);
        } else {
            header_cell = header_cell.flex_1();
        }

        header_cell
    }

    pub(in crate::window::render) fn file_icon_element(
        presentation: FileIconPresentation,
        icon_pixel_size: f32,
        icon_color: Color,
        cx: &App,
    ) -> AnyElement {
        let icon_size = IconSize::Custom(rems_from_px(icon_pixel_size));

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

    fn dispatch_file_tile_click(
        this: &mut Self,
        event: &ClickEvent,
        interaction: &FileTileInteraction,
        cx: &mut ViewContext<Self>,
    ) {
        match interaction {
            FileTileInteraction::Entry {
                entry_index,
                name_for_open,
                name_for_click,
                is_directory,
                ..
            } => Self::handle_file_item_click(
                this,
                event,
                *entry_index,
                name_for_open,
                *is_directory,
                name_for_click,
                cx,
            ),
            FileTileInteraction::Search {
                entry_index,
                path,
                selection_key,
                is_directory,
            } => Self::handle_search_item_click(
                this,
                event,
                *entry_index,
                path,
                *is_directory,
                selection_key,
                cx,
            ),
        }
    }

    fn dispatch_file_tile_context(
        this: &mut Self,
        event: &MouseDownEvent,
        window: &mut Window,
        interaction: &FileTileInteraction,
        cx: &mut ViewContext<Self>,
    ) {
        match interaction {
            FileTileInteraction::Entry { context_name, .. } => {
                this.prepare_context_selection(context_name, cx);
            }
            FileTileInteraction::Search { selection_key, .. } => {
                if !this.selected_files.contains(selection_key) {
                    this.selected_files.clear();
                    this.selected_files.insert(selection_key.clone());
                }
            }
        }
        this.deploy_context_menu(
            event.position,
            ContextMenuTarget::FileList,
            window,
            cx,
        );
        cx.notify();
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
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) -> AnyElement {
        let name = file_info.get_name().unwrap_or("").to_string();
        let is_directory = file_info.get_file_type() == FileType::Directory;
        let is_in_selection = self.selected_files.contains(&name);
        let is_focused = self.list_focus_index == Some(entry_index);
        let size_string = if is_directory {
            "--".to_string()
        } else {
            format_size(file_info.get_size())
        };
        let modified_string = format_modified(file_info);
        let type_string = format_file_type(file_info);
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
        let icon_element =
            self.render_file_icon(&file_path, view_mode, file_icon, icon_color, cx);
        let item_id = SharedString::from(format!("file-row-{name}"));
        let icon_layout = icon_view_layout(self.icon_size, self.files_panel_width());
        let drag_payload = DraggedFilePaths {
            paths: if is_in_selection && self.selected_files.len() > 1 {
                self.selected_paths()
            } else {
                vec![file_path.clone()]
            },
        };

        let context_name = name.clone();
        let drag_row_id = SharedString::from(format!("file-drag-{name}"));
        let row_height = self.files_list_item_height();

        if matches!(view_mode, ViewMode::Icon | ViewMode::Compact) {
            let tile_secondary_label = (view_mode == ViewMode::Compact).then(|| type_string.clone());
            let directory_drop_path = is_directory.then_some(file_path.clone());
            let tile = self.render_file_tile(
                drag_row_id,
                view_mode,
                icon_element,
                name,
                tile_secondary_label,
                is_in_selection,
                is_focused,
                icon_layout,
                drag_payload,
                directory_drop_path,
                FileTileInteraction::Entry {
                    entry_index,
                    name_for_open,
                    name_for_click,
                    is_directory,
                    context_name,
                },
                window,
                cx,
            );
            return tile.into_any_element();
        }

        let list_item = match view_mode {
            ViewMode::Icon | ViewMode::Compact => unreachable!("handled by render_file_tile"),
            ViewMode::Table => file_list_item(item_id, is_in_selection, is_focused)
                .start_slot(icon_element)
                .child(Label::new(name).truncate().flex_1())
                .end_slot(self.render_table_row_columns(size_string, type_string, modified_string))
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    Self::handle_file_item_click(
                        this,
                        event,
                        entry_index,
                        &name_for_open,
                        is_directory,
                        &name_for_click,
                        cx,
                    );
                }))
                .on_secondary_mouse_down(cx.listener(
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
                ))
                .into_any_element(),
            ViewMode::List => file_list_item(item_id, is_in_selection, is_focused)
                .start_slot(icon_element)
                .child(Label::new(name.clone()))
                .end_slot(Label::new(size_string).color(Color::Muted).size(LabelSize::XSmall))
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    Self::handle_file_item_click(
                        this,
                        event,
                        entry_index,
                        &name_for_open,
                        is_directory,
                        &name_for_click,
                        cx,
                    );
                }))
                .on_secondary_mouse_down(cx.listener(
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
                ))
                .into_any_element(),
        };

        let mut row = div()
            .id(drag_row_id)
            .h(row_height)
            .overflow_hidden()
            .on_drag(drag_payload, |payload: &DraggedFilePaths, _, _, cx| {
                cx.new(|_| payload.clone())
            })
            .child(list_item);
        if is_directory {
            row = self.apply_directory_drop_target(row, file_path, cx);
        }
        row.into_any_element()
    }

    pub(in crate::window::render) fn render_file_tile(
        &self,
        tile_id: SharedString,
        view_mode: ViewMode,
        icon_element: AnyElement,
        primary_label: String,
        secondary_label: Option<String>,
        is_selected: bool,
        is_focused: bool,
        icon_layout: IconViewLayout,
        drag_payload: DraggedFilePaths,
        directory_drop_path: Option<PathBuf>,
        interaction: FileTileInteraction,
        window: &Window,
        cx: &mut ViewContext<Self>,
    ) -> Stateful<Div> {
        let interaction_click_icon = interaction.clone();
        let interaction_click_label = interaction.clone();
        let click_icon = cx.listener(move |this, event: &ClickEvent, _, cx| {
            Self::dispatch_file_tile_click(this, event, &interaction_click_icon, cx);
        });
        let click_label = cx.listener(move |this, event: &ClickEvent, _, cx| {
            Self::dispatch_file_tile_click(this, event, &interaction_click_label, cx);
        });
        let interaction_context_icon = interaction.clone();
        let interaction_context_label = interaction.clone();
        let context_icon = cx.listener(move |this, event: &MouseDownEvent, window, cx| {
            Self::dispatch_file_tile_context(this, event, window, &interaction_context_icon, cx);
        });
        let context_label = cx.listener(move |this, event: &MouseDownEvent, window, cx| {
            Self::dispatch_file_tile_context(
                this,
                event,
                window,
                &interaction_context_label,
                cx,
            );
        });
        let icon_part_id = format!("{tile_id}-icon");
        let label_part_id = format!("{tile_id}-label");
        let (tile_width, tile_height) = match view_mode {
            ViewMode::Icon => (icon_layout.cell_width, icon_layout.cell_height),
            ViewMode::Compact => (COMPACT_TILE_WIDTH_PX, COMPACT_TILE_HEIGHT_PX),
            _ => unreachable!("render_file_tile is only for icon and compact views"),
        };
        let icon_pixel_size = self.file_icon_pixel_size(view_mode);

        let label_max_width_px =
            (icon_layout.cell_width - ICON_VIEW_PADDING_PX * 2.0).max(10.0);
        let label_max_width = px(label_max_width_px);
        let icon_label_line_limit = if is_selected {
            None
        } else {
            Some(ICON_LABEL_MAX_LINES_UNSELECTED)
        };
        let icon_label_layout = (view_mode == ViewMode::Icon && secondary_label.is_none()).then(
            || {
                icon_view_label_layout(
                    &primary_label,
                    label_max_width_px,
                    icon_label_line_limit,
                    window,
                    cx,
                )
            },
        );
        let label_element = if let Some(secondary) = secondary_label {
            v_flex()
                .gap_0p5()
                .child(Label::new(primary_label).size(LabelSize::Small).truncate())
                .child(
                    Label::new(secondary)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .truncate(),
                )
                .into_any_element()
        } else if view_mode == ViewMode::Icon {
            let mut label = Label::new(primary_label)
                .size(LabelSize::XSmall)
                .line_height_style(LineHeightStyle::UiLabel);
            if icon_label_layout.is_some_and(|layout| layout.fits_on_one_line) {
                label = label.single_line();
            }
            label.into_any_element()
        } else {
            Label::new(primary_label)
                .size(LabelSize::Small)
                .truncate()
                .into_any_element()
        };

        let label_shell = {
            let layout = icon_label_layout.unwrap_or(IconViewLabelLayout::fallback(label_max_width_px));
            let label_container = if view_mode == ViewMode::Icon && layout.fits_on_one_line {
                div()
                    .max_w(label_max_width)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_start()
                    .text_center()
            } else if view_mode == ViewMode::Icon {
                div()
                    .w(px(layout.width))
                    .h(px(layout.height))
                    .max_w(label_max_width)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_start()
                    .text_center()
            } else {
                div().flex_1().min_w_0().overflow_hidden()
            };
            tile_part_shell(
                label_part_id,
                label_container.child(label_element),
                is_selected,
                is_focused,
                px(crate::view_mode::ICON_TILE_LABEL_SHELL_PADDING_PX),
                view_mode == ViewMode::Icon,
                click_label,
                context_label,
                cx,
            )
        };

        let drag_payload_for_shell = drag_payload.clone();
        let attach_drag_to_shell = |this: &Self,
                                    mut shell: Stateful<Div>,
                                    cx: &mut ViewContext<Self>| {
            shell = shell.on_drag(drag_payload_for_shell.clone(), |payload: &DraggedFilePaths, _, _, cx| {
                cx.new(|_| payload.clone())
            });
            if let Some(destination) = directory_drop_path.clone() {
                shell = this.apply_directory_drop_target(shell, destination, cx);
            }
            shell
        };

        let mut icon_shell = icon_tile_icon_shell(
            icon_part_id.clone(),
            tile_icon_content(icon_element, icon_pixel_size).into_any_element(),
            icon_pixel_size,
            is_selected,
            click_icon,
            context_icon,
            cx,
        );
        icon_shell = attach_drag_to_shell(self, icon_shell, cx);
        let label_shell = attach_drag_to_shell(self, label_shell, cx);

        match view_mode {
            ViewMode::Icon => {
                let interactive = div()
                    .id(format!("{tile_id}-interactive"))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(ICON_ICON_LABEL_GAP_PX))
                    .pt(px(ICON_VIEW_PADDING_PX))
                    .child(icon_shell)
                    .child(label_shell);
                div()
                    .id(tile_id)
                    .w(px(tile_width))
                    .h(px(tile_height))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_start()
                    .child(interactive)
            }
            ViewMode::Compact => {
                let interactive = h_flex()
                    .id(format!("{tile_id}-interactive"))
                    .items_center()
                    .gap(px(COMPACT_TILE_ICON_LABEL_GAP_PX))
                    .px(px(COMPACT_TILE_HORIZONTAL_PADDING_PX))
                    .child(icon_shell.flex_shrink_0())
                    .child(label_shell.flex_1().min_w_0());
                div()
                    .id(tile_id)
                    .w(px(tile_width))
                    .h(px(tile_height))
                    .flex()
                    .items_center()
                    .justify_start()
                    .overflow_hidden()
                    .child(interactive)
            }
            _ => unreachable!(),
        }
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

    pub(in crate::window::render) fn file_icon_pixel_size(&self, view_mode: ViewMode) -> f32 {
        match view_mode {
            ViewMode::Icon => self.icon_size as f32,
            ViewMode::Compact => COMPACT_TILE_ICON_PX as f32,
            ViewMode::List | ViewMode::Table => 20.0,
        }
    }

    pub(in crate::window::render) fn render_file_icon(
        &self,
        file_path: &Path,
        view_mode: ViewMode,
        file_icon: IconName,
        icon_color: Color,
        cx: &App,
    ) -> AnyElement {
        let icon_pixel_size = self.file_icon_pixel_size(view_mode);
        let cache_pixel_size = self.file_icon_cache_size();
        if let Some(presentation) = self.icon_cache.cached_icon(file_path, cache_pixel_size) {
            return Self::file_icon_element(presentation, icon_pixel_size, icon_color, cx);
        }

        Icon::new(file_icon)
            .size(IconSize::Custom(rems_from_px(icon_pixel_size)))
            .color(icon_color)
            .into_any_element()
    }

    pub(in crate::window::render) fn render_files_area(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        if self.view_mode == ViewMode::Icon {
            self.sync_icon_label_layout_cache(window, cx);
        }
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

        let uses_tile_grid = matches!(view_mode, ViewMode::Icon | ViewMode::Compact);
        let tile_scroll_content: Vec<AnyElement> = if uses_tile_grid
            && !self.loading_directory
            && !search_in_progress
            && item_count > 0
        {
            let columns = match view_mode {
                ViewMode::Icon => {
                    icon_view_layout(self.icon_size, self.files_panel_width()).columns
                }
                ViewMode::Compact => compact_view_layout(self.files_panel_width()).columns,
                _ => 1,
            }
            .max(1);
            let tile_gap = if view_mode == ViewMode::Compact {
                COMPACT_TILE_SPACING_PX
            } else {
                ICON_VIEW_TILE_GAP_PX
            };

            let tiles: Vec<_> = if subfolder_search {
                (0..item_count)
                    .filter_map(|entry_index| {
                        self.search_matches.get(entry_index).map(|search_match| {
                            self.render_search_match(
                                search_match,
                                entry_index,
                                view_mode,
                                window,
                                cx,
                            )
                        })
                    })
                    .collect()
            } else {
                let visible_files: Vec<_> = self.visible_files();
                (0..item_count)
                    .filter_map(|entry_index| {
                        visible_files.get(entry_index).map(|file_info| {
                            self.render_file_entry(
                                file_info,
                                entry_index,
                                view_mode,
                                window,
                                cx,
                            )
                        })
                    })
                    .collect()
            };

            tile_grid_rows(tiles, columns, px(tile_gap))
        } else {
            Vec::new()
        };

        let mut scroll = div()
            .id("files-scroll-area")
            .flex_1()
            .flex()
            .flex_col()
            .min_h_0()
            .relative()
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

        if !uses_tile_grid {
            scroll = scroll
                .drag_over::<DraggedFilePaths>(|style, _, _, cx| drop_target_style(style, cx))
                .drag_over::<ExternalPaths>(|style, _, _, cx| drop_target_style(style, cx));
        }

        if uses_tile_grid {
            let tile_scroll_handle = self.files_scroll_handle.0.borrow().base_handle.clone();
            let tile_gap = if view_mode == ViewMode::Compact {
                px(COMPACT_TILE_SPACING_PX)
            } else {
                px(ICON_VIEW_TILE_GAP_PX)
            };
            scroll.child(
                div()
                    .id("fileman-marquee-layer")
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        self.attach_marquee_handlers(
                            div()
                                .id("fileman-tile-scroll")
                                .overflow_y_scroll()
                                .size_full()
                                .track_scroll(&tile_scroll_handle)
                                .relative()
                                .p(px(ICON_VIEW_PADDING_PX))
                                .flex()
                                .flex_col()
                                .gap(tile_gap)
                                .children(tile_scroll_content)
                                .child(self.render_marquee_overlay(cx, true)),
                            cx,
                        ),
                    ),
            )
        } else {
            let table_header = (view_mode == ViewMode::Table
                && !self.loading_directory
                && item_count > 0)
                .then(|| self.render_table_sortable_header(cx));

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
                                    this.render_search_match_range(range, view_mode, window, cx)
                                } else {
                                    this.render_file_entry_range(range, view_mode, window, cx)
                                }
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.files_scroll_handle),
                    )
                    .vertical_scrollbar_for(&self.files_scroll_handle, window, cx)
                    .child(self.render_marquee_overlay(cx, false)),
                )
        }
    }

    pub(in crate::window::render) fn render_marquee_overlay(
        &self,
        cx: &mut ViewContext<Self>,
        use_list_coordinates: bool,
    ) -> AnyElement {
        let Some(marquee) = self.marquee_drag.as_ref().filter(|marquee| marquee.active) else {
            return div().into_any_element();
        };

        let colors = cx.theme().colors();
        let (left, top, width, height) = self.marquee_overlay_bounds_from_list(
            marquee.origin_list,
            marquee.pointer_list,
            use_list_coordinates,
        );

        div()
            .absolute()
            .left(left)
            .top(top)
            .w(width.max(px(1.0)))
            .h(height.max(px(1.0)))
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
        view_mode: ViewMode,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) -> AnyElement {
        let path = search_match.path.clone();
        let selection_key = Self::selection_key_for_path(&path);
        let is_in_selection = self.selected_files.contains(&selection_key);
        let is_focused = self.list_focus_index == Some(entry_index);
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
        let icon_element = self.render_file_icon(&path, view_mode, file_icon, icon_color, cx);
        let item_id = SharedString::from(format!("search-row-{}", path.display()));
        let subtitle = format!("{} · {}", search_match.parent_label, search_match.name);
        let drag_payload = DraggedFilePaths {
            paths: if is_in_selection && self.selected_files.len() > 1 {
                self.selected_paths()
            } else {
                vec![path.clone()]
            },
        };
        let name = search_match.name.clone();

        let row_height = self.files_list_item_height();
        let drag_row_id = SharedString::from(format!("search-drag-{}", path.display()));
        let icon_layout = icon_view_layout(self.icon_size, self.files_panel_width());

        if matches!(view_mode, ViewMode::Icon | ViewMode::Compact) {
            let directory_drop_path = is_directory.then_some(path.clone());
            let tile = self.render_file_tile(
                drag_row_id,
                view_mode,
                icon_element,
                name,
                Some(subtitle),
                is_in_selection,
                is_focused,
                icon_layout,
                drag_payload,
                directory_drop_path,
                FileTileInteraction::Search {
                    entry_index,
                    path,
                    selection_key,
                    is_directory,
                },
                window,
                cx,
            );
            return tile.into_any_element();
        }

        let path_for_open_table = path.clone();
        let path_for_open_list = path.clone();
        let selection_key_for_click_table = selection_key.clone();
        let selection_key_for_click_list = selection_key.clone();
        let selection_key_for_context_table = selection_key.clone();
        let selection_key_for_context_list = selection_key.clone();
        let list_item = match view_mode {
            ViewMode::Icon | ViewMode::Compact => unreachable!("handled by render_file_tile"),
            ViewMode::Table => file_list_item(item_id, is_in_selection, is_focused)
                .start_slot(icon_element)
                .child(Label::new(name).truncate().flex_1())
                .end_slot(self.render_table_row_columns(
                    "--".to_string(),
                    "--".to_string(),
                    "--".to_string(),
                ))
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    Self::handle_search_item_click(
                        this,
                        event,
                        entry_index,
                        &path_for_open_table,
                        is_directory,
                        &selection_key_for_click_table,
                        cx,
                    );
                }))
                .on_secondary_mouse_down(cx.listener(
                    move |this, event: &MouseDownEvent, window, cx| {
                        if !this.selected_files.contains(&selection_key_for_context_table) {
                            this.selected_files.clear();
                            this.selected_files
                                .insert(selection_key_for_context_table.clone());
                        }
                        this.deploy_context_menu(
                            event.position,
                            ContextMenuTarget::FileList,
                            window,
                            cx,
                        );
                        cx.notify();
                    },
                ))
                .into_any_element(),
            ViewMode::List => file_list_item(item_id, is_in_selection, is_focused)
                .start_slot(icon_element)
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(Label::new(name).truncate())
                        .child(
                            Label::new(subtitle)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .truncate(),
                        ),
                )
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    Self::handle_search_item_click(
                        this,
                        event,
                        entry_index,
                        &path_for_open_list,
                        is_directory,
                        &selection_key_for_click_list,
                        cx,
                    );
                }))
                .on_secondary_mouse_down(cx.listener(
                    move |this, event: &MouseDownEvent, window, cx| {
                        if !this.selected_files.contains(&selection_key_for_context_list) {
                            this.selected_files.clear();
                            this.selected_files
                                .insert(selection_key_for_context_list.clone());
                        }
                        this.deploy_context_menu(
                            event.position,
                            ContextMenuTarget::FileList,
                            window,
                            cx,
                        );
                        cx.notify();
                    },
                ))
                .into_any_element(),
        };

        let mut row = div()
            .id(drag_row_id)
            .h(row_height)
            .overflow_hidden()
            .on_drag(drag_payload, |payload: &DraggedFilePaths, _, _, cx| {
                cx.new(|_| payload.clone())
            })
            .child(list_item);
        if is_directory {
            row = self.apply_directory_drop_target(row, path, cx);
        }
        row.into_any_element()
    }

    pub(in crate::window::render) fn render_search_match_range(
        &mut self,
        range: Range<usize>,
        view_mode: ViewMode,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) -> Vec<AnyElement> {
        self.list_visible_range = Some(range.clone());
        self.refresh_uniform_list_row_height(self.search_matches.len());
        range
            .filter_map(|entry_index| {
                self.search_matches.get(entry_index).map(|search_match| {
                    self.render_search_match(
                        search_match,
                        entry_index,
                        view_mode,
                        window,
                        cx,
                    )
                })
            })
            .collect()
    }

}
