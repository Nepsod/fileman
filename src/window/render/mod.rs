mod chrome;
mod dialogs;
mod file_list;

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

