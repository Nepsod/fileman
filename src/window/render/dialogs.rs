use crate::window::imports::*;

impl FilemanWindow {
    pub(in crate::window::render) fn render_about_dialog(cx: &mut ViewContext<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let info = crate::about::ABOUT;

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
                        Label::new(format!("Repository: {}", crate::about::REPOSITORY))
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

    pub(in crate::window::render) fn render_delete_dialog(
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
    pub(in crate::window::render) fn render_paste_conflict_dialog(
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

    pub(in crate::window::render) fn render_properties_dialog(
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

    pub(in crate::window::render) fn render_rename_dialog(
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

    pub(in crate::window::render) fn render_settings_dialog(
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

    pub(in crate::window::render) fn render_settings_option(
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

}
