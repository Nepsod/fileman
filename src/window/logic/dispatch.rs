use crate::actions::*;
use crate::search::SearchScope;
use crate::sort::{SortColumn, SortOrder};
use crate::view_mode::ViewMode;
use crate::window::FilemanWindow;
use nptk::gpui::{Action, Context, Window};

type ViewContext<'a, T> = Context<'a, T>;

impl FilemanWindow {
    pub(crate) fn dispatch_action(
        &mut self,
        action: &dyn Action,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        if action.partial_eq(&CreateFolder) {
            self.create_folder(cx);
        } else if action.partial_eq(&CreateFile) {
            self.create_file(cx);
        } else if action.partial_eq(&GoBack) {
            self.go_back(cx);
        } else if action.partial_eq(&GoForward) {
            self.go_forward(cx);
        } else if action.partial_eq(&GoUp) {
            self.go_up(cx);
        } else if action.partial_eq(&ToggleHidden) {
            self.toggle_hidden(cx);
        } else if action.partial_eq(&DeleteSelected) {
            self.delete_selected(cx);
        } else if action.partial_eq(&DeletePermanent) {
            self.request_delete(true, cx);
        } else if action.partial_eq(&Refresh) {
            self.reload_volume_mounts();
            self.reload_current_directory(cx);
        } else if action.partial_eq(&SelectAll) {
            self.select_all_visible(cx);
        } else if action.partial_eq(&Rename) {
            self.start_rename_selected(cx);
        } else if action.partial_eq(&crate::actions::Copy) {
            self.copy_selected(cx);
        } else if action.partial_eq(&Cut) {
            self.cut_selected(cx);
        } else if action.partial_eq(&Paste) {
            self.paste_clipboard(cx);
        } else if action.partial_eq(&Duplicate) {
            self.duplicate_selected(cx);
        } else if action.partial_eq(&ClearSelection) {
            self.clear_selection(cx);
        } else if action.partial_eq(&InvertSelection) {
            self.invert_selection(cx);
        } else if action.partial_eq(&ActivateSearch) {
            self.activate_search(window, cx);
        } else if action.partial_eq(&ClearSearch) {
            self.clear_search(cx);
        } else if action.partial_eq(&ToggleSearchSubfolders) {
            self.toggle_search_subfolders(window, cx);
        } else if action.partial_eq(&SetSearchCurrentFolder) {
            self.set_search_scope(SearchScope::CurrentFolder, window, cx);
        } else if action.partial_eq(&SetSearchIncludeSubfolders) {
            self.set_search_scope(SearchScope::Subfolders, window, cx);
        } else if action.partial_eq(&FocusPathBar) {
            self.focus_path_bar(window, cx);
        } else if action.partial_eq(&GoHome) {
            self.go_home(cx);
        } else if action.partial_eq(&NewTab) {
            self.new_tab(cx);
        } else if action.partial_eq(&NewWindow) {
            self.spawn_new_window(cx);
        } else if action.partial_eq(&CloseTab) {
            self.close_tab(cx);
        } else if action.partial_eq(&AddBookmark) {
            self.add_bookmark_for_current(cx);
        } else if action.partial_eq(&RemoveBookmark) {
            self.remove_bookmark_for_current(cx);
        } else if action.partial_eq(&SortByName) {
            self.apply_sort(SortColumn::Name, None, cx);
        } else if action.partial_eq(&SortBySize) {
            self.apply_sort(SortColumn::Size, None, cx);
        } else if action.partial_eq(&SortByModified) {
            self.apply_sort(SortColumn::Modified, None, cx);
        } else if action.partial_eq(&SortByType) {
            self.apply_sort(SortColumn::Type, None, cx);
        } else if action.partial_eq(&ToggleSortOrder) {
            self.toggle_sort_order(cx);
        } else if action.partial_eq(&SortNameAsc) {
            self.apply_sort(SortColumn::Name, Some(SortOrder::Ascending), cx);
        } else if action.partial_eq(&SortNameDesc) {
            self.apply_sort(SortColumn::Name, Some(SortOrder::Descending), cx);
        } else if action.partial_eq(&SortSizeAsc) {
            self.apply_sort(SortColumn::Size, Some(SortOrder::Ascending), cx);
        } else if action.partial_eq(&SortSizeDesc) {
            self.apply_sort(SortColumn::Size, Some(SortOrder::Descending), cx);
        } else if action.partial_eq(&SortModifiedAsc) {
            self.apply_sort(SortColumn::Modified, Some(SortOrder::Ascending), cx);
        } else if action.partial_eq(&SortModifiedDesc) {
            self.apply_sort(SortColumn::Modified, Some(SortOrder::Descending), cx);
        } else if action.partial_eq(&SortTypeAsc) {
            self.apply_sort(SortColumn::Type, Some(SortOrder::Ascending), cx);
        } else if action.partial_eq(&SortTypeDesc) {
            self.apply_sort(SortColumn::Type, Some(SortOrder::Descending), cx);
        } else if action.partial_eq(&Undo) {
            self.undo_last(cx);
        } else if action.partial_eq(&Redo) {
            self.redo_last(cx);
        } else if action.partial_eq(&OpenTerminal) {
            self.open_terminal_here(cx);
        } else if action.partial_eq(&OpenSelection) {
            self.open_primary_selection(cx);
        } else if action.partial_eq(&OpenWithSystem) {
            self.open_selection_with_system(cx);
        } else if action.partial_eq(&ShowProperties) {
            self.show_properties_for_selection(cx);
        } else if action.partial_eq(&ShowSettings) {
            self.open_settings(cx);
        } else if action.partial_eq(&ShowAbout) {
            self.open_about(cx);
        } else if action.partial_eq(&GoToParent) {
            self.go_to_parent_of_selection(cx);
        } else if action.partial_eq(&ZoomIn) {
            self.zoom_icons_in(cx);
        } else if action.partial_eq(&ZoomOut) {
            self.zoom_icons_out(cx);
        } else if action.partial_eq(&ZoomReset) {
            self.zoom_icons_reset(cx);
        } else if action.partial_eq(&ViewList) {
            self.set_view_mode(ViewMode::List, cx);
        } else if action.partial_eq(&ViewIcon) {
            self.set_view_mode(ViewMode::Icon, cx);
        } else if action.partial_eq(&ViewCompact) {
            self.set_view_mode(ViewMode::Compact, cx);
        } else if action.partial_eq(&ViewTable) {
            self.set_view_mode(ViewMode::Table, cx);
        }
    }
}
