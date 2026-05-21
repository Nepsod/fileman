use crate::actions::*;
use crate::search::SearchScope;
use crate::sort::{SortColumn, SortOrder};
use crate::view_mode::ViewMode;
use crate::window::FilemanWindow;
use nptk::gpui::{App, Context, Menu, MenuItem, Window};

type ViewContext<'a, T> = Context<'a, T>;

impl FilemanWindow {
    pub(crate) fn register_menus(&self, cx: &mut ViewContext<Self>) {
        let sort_menu = Menu::new("Sort By").items(vec![
            MenuItem::action("Name (Ascending)", SortNameAsc),
            MenuItem::action("Name (Descending)", SortNameDesc),
            MenuItem::action("Size (Ascending)", SortSizeAsc),
            MenuItem::action("Size (Descending)", SortSizeDesc),
            MenuItem::action("Date Modified (Ascending)", SortModifiedAsc),
            MenuItem::action("Date Modified (Descending)", SortModifiedDesc),
            MenuItem::action("Type (Ascending)", SortTypeAsc),
            MenuItem::action("Type (Descending)", SortTypeDesc),
        ]);

        cx.set_menus(vec![
            Menu::new("File").items(vec![
                MenuItem::action("Home", GoHome),
                MenuItem::action("New Tab", NewTab),
                MenuItem::action("New Window", NewWindow),
                MenuItem::action("Close Tab", CloseTab),
                MenuItem::separator(),
                MenuItem::action("New Folder", CreateFolder),
                MenuItem::action("New File", CreateFile),
                MenuItem::action("Open", OpenSelection),
                MenuItem::action("Properties", ShowProperties),
                MenuItem::separator(),
                MenuItem::action("Back", GoBack),
                MenuItem::action("Forward", GoForward),
                MenuItem::action("Up", GoUp),
            ]),
            Menu::new("Edit").items(vec![
                MenuItem::action("Focus Location Bar", FocusPathBar),
                MenuItem::action("Activate Search", ActivateSearch),
                MenuItem::separator(),
                MenuItem::action("Select All", SelectAll),
                MenuItem::action("Deselect All", ClearSelection),
                MenuItem::action("Invert Selection", InvertSelection),
                MenuItem::separator(),
                MenuItem::action("Copy", crate::actions::Copy),
                MenuItem::action("Cut", Cut),
                MenuItem::action("Paste", Paste),
                MenuItem::separator(),
                MenuItem::action("Undo", Undo),
                MenuItem::action("Redo", Redo),
                MenuItem::separator(),
                MenuItem::action("Rename", Rename),
                MenuItem::action("Move to Trash", DeleteSelected),
                MenuItem::action("Delete Permanently", DeletePermanent),
                MenuItem::action("Duplicate", Duplicate),
            ]),
            Menu::new("View").items(vec![
                MenuItem::action("Refresh", Refresh),
                MenuItem::submenu(sort_menu),
                MenuItem::action("Search: Current Folder", SetSearchCurrentFolder),
                MenuItem::action("Search: Include Subfolders", SetSearchIncludeSubfolders),
                MenuItem::action("Show Hidden Files", ToggleHidden),
                MenuItem::separator(),
                MenuItem::action("List View", ViewList),
                MenuItem::action("Icon View", ViewIcon),
                MenuItem::action("Compact View", ViewCompact),
                MenuItem::action("Table View", ViewTable),
                MenuItem::separator(),
                MenuItem::action("Zoom In", ZoomIn),
                MenuItem::action("Zoom Out", ZoomOut),
                MenuItem::action("Zoom Reset", ZoomReset),
            ]),
            Menu::new("Tools").items(vec![MenuItem::action(
                "Open Terminal Here",
                OpenTerminal,
            )]),
            Menu::new("Bookmarks").items(vec![
                MenuItem::action("Add Current Folder", AddBookmark),
                MenuItem::action("Remove Current Folder", RemoveBookmark),
            ]),
            Menu::new("Settings").items(vec![MenuItem::action(
                "Configure Fileman",
                ShowSettings,
            )]),
            Menu::new("Help").items(vec![MenuItem::action("About", ShowAbout)]),
        ]);
    }
}

pub(crate) fn with_active_fileman(
    cx: &mut App,
    f: impl FnOnce(&mut FilemanWindow, &mut Window, &mut Context<FilemanWindow>),
) {
    let handle = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next());
    let Some(handle) = handle else {
        return;
    };

    let _ = handle.update(cx, |root, window, cx| {
        if let Ok(view) = root.downcast::<FilemanWindow>() {
            view.update(cx, |fileman, cx| f(fileman, window, cx));
        }
    });
}

pub fn register_app_menu_handlers(cx: &mut App) {
    cx.on_action(|_: &CreateFolder, cx| {
        with_active_fileman(cx, |this, _, cx| this.create_folder(cx));
    });
    cx.on_action(|_: &CreateFile, cx| {
        with_active_fileman(cx, |this, _, cx| this.create_file(cx));
    });
    cx.on_action(|_: &GoBack, cx| {
        with_active_fileman(cx, |this, _, cx| this.go_back(cx));
    });
    cx.on_action(|_: &GoForward, cx| {
        with_active_fileman(cx, |this, _, cx| this.go_forward(cx));
    });
    cx.on_action(|_: &GoUp, cx| {
        with_active_fileman(cx, |this, _, cx| this.go_up(cx));
    });
    cx.on_action(|_: &ToggleHidden, cx| {
        with_active_fileman(cx, |this, _, cx| this.toggle_hidden(cx));
    });
    cx.on_action(|_: &DeleteSelected, cx| {
        with_active_fileman(cx, |this, _, cx| this.delete_selected(cx));
    });
    cx.on_action(|_: &DeletePermanent, cx| {
        with_active_fileman(cx, |this, _, cx| this.request_delete(true, cx));
    });
    cx.on_action(|_: &Refresh, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.reload_volume_mounts();
            this.reload_current_directory(cx);
        });
    });
    cx.on_action(|_: &SelectAll, cx| {
        with_active_fileman(cx, |this, _, cx| this.select_all_visible(cx));
    });
    cx.on_action(|_: &Rename, cx| {
        with_active_fileman(cx, |this, _, cx| this.start_rename_selected(cx));
    });
    cx.on_action(|_: &Copy, cx| {
        with_active_fileman(cx, |this, _, cx| this.copy_selected(cx));
    });
    cx.on_action(|_: &Cut, cx| {
        with_active_fileman(cx, |this, _, cx| this.cut_selected(cx));
    });
    cx.on_action(|_: &Paste, cx| {
        with_active_fileman(cx, |this, _, cx| this.paste_clipboard(cx));
    });
    cx.on_action(|_: &Duplicate, cx| {
        with_active_fileman(cx, |this, _, cx| this.duplicate_selected(cx));
    });
    cx.on_action(|_: &ClearSelection, cx| {
        with_active_fileman(cx, |this, _, cx| this.clear_selection(cx));
    });
    cx.on_action(|_: &InvertSelection, cx| {
        with_active_fileman(cx, |this, _, cx| this.invert_selection(cx));
    });
    cx.on_action(|_: &ActivateSearch, cx| {
        with_active_fileman(cx, |this, window, cx| this.activate_search(window, cx));
    });
    cx.on_action(|_: &ClearSearch, cx| {
        with_active_fileman(cx, |this, _, cx| this.clear_search(cx));
    });
    cx.on_action(|_: &ToggleSearchSubfolders, cx| {
        with_active_fileman(cx, |this, window, cx| this.toggle_search_subfolders(window, cx));
    });
    cx.on_action(|_: &SetSearchCurrentFolder, cx| {
        with_active_fileman(cx, |this, window, cx| {
            this.set_search_scope(SearchScope::CurrentFolder, window, cx);
        });
    });
    cx.on_action(|_: &SetSearchIncludeSubfolders, cx| {
        with_active_fileman(cx, |this, window, cx| {
            this.set_search_scope(SearchScope::Subfolders, window, cx);
        });
    });
    cx.on_action(|_: &FocusPathBar, cx| {
        with_active_fileman(cx, |this, window, cx| this.focus_path_bar(window, cx));
    });
    cx.on_action(|_: &GoHome, cx| {
        with_active_fileman(cx, |this, _, cx| this.go_home(cx));
    });
    cx.on_action(|_: &NewTab, cx| {
        with_active_fileman(cx, |this, _, cx| this.new_tab(cx));
    });
    cx.on_action(|_: &NewWindow, cx| {
        with_active_fileman(cx, |this, _, cx| this.spawn_new_window(cx));
    });
    cx.on_action(|_: &CloseTab, cx| {
        with_active_fileman(cx, |this, _, cx| this.close_tab(cx));
    });
    cx.on_action(|_: &AddBookmark, cx| {
        with_active_fileman(cx, |this, _, cx| this.add_bookmark_for_current(cx));
    });
    cx.on_action(|_: &RemoveBookmark, cx| {
        with_active_fileman(cx, |this, _, cx| this.remove_bookmark_for_current(cx));
    });
    cx.on_action(|_: &SortByName, cx| {
        with_active_fileman(cx, |this, _, cx| this.apply_sort(SortColumn::Name, None, cx));
    });
    cx.on_action(|_: &SortBySize, cx| {
        with_active_fileman(cx, |this, _, cx| this.apply_sort(SortColumn::Size, None, cx));
    });
    cx.on_action(|_: &SortByModified, cx| {
        with_active_fileman(cx, |this, _, cx| this.apply_sort(SortColumn::Modified, None, cx));
    });
    cx.on_action(|_: &SortByType, cx| {
        with_active_fileman(cx, |this, _, cx| this.apply_sort(SortColumn::Type, None, cx));
    });
    cx.on_action(|_: &ToggleSortOrder, cx| {
        with_active_fileman(cx, |this, _, cx| this.toggle_sort_order(cx));
    });
    cx.on_action(|_: &SortNameAsc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Name, Some(SortOrder::Ascending), cx);
        });
    });
    cx.on_action(|_: &SortNameDesc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Name, Some(SortOrder::Descending), cx);
        });
    });
    cx.on_action(|_: &SortSizeAsc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Size, Some(SortOrder::Ascending), cx);
        });
    });
    cx.on_action(|_: &SortSizeDesc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Size, Some(SortOrder::Descending), cx);
        });
    });
    cx.on_action(|_: &SortModifiedAsc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Modified, Some(SortOrder::Ascending), cx);
        });
    });
    cx.on_action(|_: &SortModifiedDesc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Modified, Some(SortOrder::Descending), cx);
        });
    });
    cx.on_action(|_: &SortTypeAsc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Type, Some(SortOrder::Ascending), cx);
        });
    });
    cx.on_action(|_: &SortTypeDesc, cx| {
        with_active_fileman(cx, |this, _, cx| {
            this.apply_sort(SortColumn::Type, Some(SortOrder::Descending), cx);
        });
    });
    cx.on_action(|_: &Undo, cx| {
        with_active_fileman(cx, |this, _, cx| this.undo_last(cx));
    });
    cx.on_action(|_: &Redo, cx| {
        with_active_fileman(cx, |this, _, cx| this.redo_last(cx));
    });
    cx.on_action(|_: &OpenTerminal, cx| {
        with_active_fileman(cx, |this, _, cx| this.open_terminal_here(cx));
    });
    cx.on_action(|_: &OpenSelection, cx| {
        with_active_fileman(cx, |this, _, cx| this.open_primary_selection(cx));
    });
    cx.on_action(|_: &OpenWithSystem, cx| {
        with_active_fileman(cx, |this, _, cx| this.open_selection_with_system(cx));
    });
    cx.on_action(|_: &ShowProperties, cx| {
        with_active_fileman(cx, |this, _, cx| this.show_properties_for_selection(cx));
    });
    cx.on_action(|_: &ShowSettings, cx| {
        with_active_fileman(cx, |this, _, cx| this.open_settings(cx));
    });
    cx.on_action(|_: &ShowAbout, cx| {
        with_active_fileman(cx, |this, _, cx| this.open_about(cx));
    });
    cx.on_action(|_: &GoToParent, cx| {
        with_active_fileman(cx, |this, _, cx| this.go_to_parent_of_selection(cx));
    });
    cx.on_action(|_: &ZoomIn, cx| {
        with_active_fileman(cx, |this, _, cx| this.zoom_icons_in(cx));
    });
    cx.on_action(|_: &ZoomOut, cx| {
        with_active_fileman(cx, |this, _, cx| this.zoom_icons_out(cx));
    });
    cx.on_action(|_: &ZoomReset, cx| {
        with_active_fileman(cx, |this, _, cx| this.zoom_icons_reset(cx));
    });
    cx.on_action(|_: &ViewList, cx| {
        with_active_fileman(cx, |this, _, cx| this.set_view_mode(ViewMode::List, cx));
    });
    cx.on_action(|_: &ViewIcon, cx| {
        with_active_fileman(cx, |this, _, cx| this.set_view_mode(ViewMode::Icon, cx));
    });
    cx.on_action(|_: &ViewCompact, cx| {
        with_active_fileman(cx, |this, _, cx| this.set_view_mode(ViewMode::Compact, cx));
    });
    cx.on_action(|_: &ViewTable, cx| {
        with_active_fileman(cx, |this, _, cx| this.set_view_mode(ViewMode::Table, cx));
    });
    cx.on_action(|_: &Quit, cx| cx.quit());
}
