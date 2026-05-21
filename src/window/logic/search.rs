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
    pub(crate) fn activate_search(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
        self.path_edit_active = false;
        self.search_active = true;
        self.search_line_input.update(cx, |input, cx| {
            input.set_text(self.search_query.clone(), cx);
        });
        self.focus_search_line_input(window, cx);
        self.set_status("Search: type to filter, Enter/Escape to finish", cx);
        cx.notify();
    }

    pub(crate) fn clear_search(&mut self, cx: &mut ViewContext<Self>) {
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

    pub(crate) fn focus_search_line_input(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
        self.search_line_input.update(cx, |input, cx| {
            input.set_text(self.search_query.clone(), cx);
        });
        let focus_handle = self.search_line_input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    }

    pub(crate) fn handle_search_input_event(
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

    pub(crate) fn record_search_history(&mut self) {
        const MAX_SEARCH_HISTORY: usize = 10;
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.search_history.retain(|entry| entry != &query);
        self.search_history.insert(0, query);
        self.search_history.truncate(MAX_SEARCH_HISTORY);
    }

    pub(crate) fn schedule_subfolder_search(&mut self, cx: &mut ViewContext<Self>) {
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
                crate::search::find_in_subfolders(&root, &query, show_hidden)
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

    pub(crate) fn set_search_scope(
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

    pub(crate) fn toggle_search_subfolders(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
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

    pub(crate) fn using_subfolder_search(&self) -> bool {
        self.search_active
            && self.search_scope == SearchScope::Subfolders
            && !self.search_query.trim().is_empty()
    }

}
