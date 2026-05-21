use crate::window::imports::*;

impl FilemanWindow {
    pub(in crate::window::render) fn render_location_bar(
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

    pub(in crate::window::render) fn render_main_panel(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .child(self.render_tab_strip(cx))
            .child(self.render_toolbar(window, cx))
            .child(self.render_files_area(window, cx))
    }

    pub(in crate::window::render) fn render_sidebar(&mut self, _window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
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
                                    .start_slot(crate::ui_icons::cached_icon_element(
                                        crate::ui_icons::quick_access_theme_icon(label)
                                            .and_then(|name| {
                                                self.icon_cache.cached_theme_icon(
                                                    name,
                                                    crate::ui_icons::SIDEBAR_ICON_PIXELS,
                                                )
                                            })
                                            .or_else(|| {
                                                self.icon_cache.cached_icon(
                                                    &path,
                                                    crate::ui_icons::SIDEBAR_ICON_PIXELS,
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
                                            .start_slot(crate::ui_icons::cached_icon_element(
                                                self.icon_cache.cached_icon(
                                                    &path,
                                                    crate::ui_icons::SIDEBAR_ICON_PIXELS,
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
                                            .start_slot(crate::ui_icons::cached_icon_element(
                                                self.icon_cache.cached_icon(
                                                    &path,
                                                    crate::ui_icons::SIDEBAR_ICON_PIXELS,
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

    pub(in crate::window::render) fn render_status_bar(
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

    pub(in crate::window::render) fn render_tab_strip(&self, cx: &mut ViewContext<Self>) -> impl IntoElement {
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
                                    crate::ui_icons::TAB_CLOSE,
                                    crate::ui_icons::TOOLBAR_ICON_PIXELS,
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
                        crate::ui_icons::TAB_NEW,
                        crate::ui_icons::TOOLBAR_ICON_PIXELS,
                    ))
                    .icon_size(IconSize::Small)
                    .on_click(cx.listener(|this, _, _, cx| this.new_tab(cx))),
            )
    }

    pub(in crate::window::render) fn render_toolbar(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
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
                                        crate::ui_icons::GO_BACK,
                                        crate::ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
                            )
                            .child(
                                ThemeIconButton::new("go-forward")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        crate::ui_icons::GO_FORWARD,
                                        crate::ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| this.go_forward(cx))),
                            )
                            .child(
                                ThemeIconButton::new("go-up")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        crate::ui_icons::GO_UP,
                                        crate::ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| this.go_up(cx))),
                            )
                            .child(
                                ThemeIconButton::new("refresh")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        crate::ui_icons::REFRESH,
                                        crate::ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reload_current_directory(cx)
                                    })),
                            )
                            .child(
                                ThemeIconButton::new("copy")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        crate::ui_icons::COPY,
                                        crate::ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| this.copy_selected(cx))),
                            )
                            .child(
                                ThemeIconButton::new("cut")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        crate::ui_icons::CUT,
                                        crate::ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| this.cut_selected(cx))),
                            )
                            .child(
                                ThemeIconButton::new("paste")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        crate::ui_icons::PASTE,
                                        crate::ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.paste_clipboard(cx)
                                    })),
                            )
                            .child(
                                ThemeIconButton::new("toolbar-delete")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        crate::ui_icons::DELETE,
                                        crate::ui_icons::TOOLBAR_ICON_PIXELS,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.delete_selected(cx)
                                    })),
                            )
                            .child(
                                ThemeIconButton::new("toolbar-properties")
                                    .cached(self.icon_cache.cached_theme_icon(
                                        crate::ui_icons::PROPERTIES,
                                        crate::ui_icons::TOOLBAR_ICON_PIXELS,
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
                                crate::ui_icons::SEARCH,
                                crate::ui_icons::TOOLBAR_ICON_PIXELS,
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
                        if let Some(hidden_icon) = crate::ui_icons::cached_theme_icon(
                            self.icon_cache.cached_theme_icon(
                                if show_hidden {
                                    crate::ui_icons::SHOW_HIDDEN
                                } else {
                                    crate::ui_icons::HIDE_HIDDEN
                                },
                                crate::ui_icons::TOOLBAR_ICON_PIXELS,
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

    pub(in crate::window::render) fn render_toolbar_view_menu(&self, window: &mut Window, cx: &mut ViewContext<Self>) -> DropdownMenu {
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

}
