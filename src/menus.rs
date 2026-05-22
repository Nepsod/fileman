use crate::actions::*;
use crate::window::logic::foreground::log_entity_update;
use crate::window::FilemanWindow;
use nptk::gpui::{App, Context, Menu, MenuItem, Window};

type ViewContext<'a, T> = Context<'a, T>;

macro_rules! register_fileman_action {
    ($cx:expr, $Action:ty) => {
        $cx.on_action(|action: &$Action, cx| {
            with_active_fileman(cx, |this, window, cx| {
                this.dispatch_action(action, window, cx);
            });
        });
    };
}

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

    log_entity_update(
        "active_fileman_menu_action",
        handle.update(cx, |root, window, cx| {
            if let Ok(view) = root.downcast::<FilemanWindow>() {
                view.update(cx, |fileman, cx| f(fileman, window, cx));
            }
        }),
    );
}

pub fn register_app_menu_handlers(cx: &mut App) {
    register_fileman_action!(cx, CreateFolder);
    register_fileman_action!(cx, CreateFile);
    register_fileman_action!(cx, GoBack);
    register_fileman_action!(cx, GoForward);
    register_fileman_action!(cx, GoUp);
    register_fileman_action!(cx, ToggleHidden);
    register_fileman_action!(cx, DeleteSelected);
    register_fileman_action!(cx, DeletePermanent);
    register_fileman_action!(cx, Refresh);
    register_fileman_action!(cx, SelectAll);
    register_fileman_action!(cx, Rename);
    register_fileman_action!(cx, crate::actions::Copy);
    register_fileman_action!(cx, Cut);
    register_fileman_action!(cx, Paste);
    register_fileman_action!(cx, Duplicate);
    register_fileman_action!(cx, ClearSelection);
    register_fileman_action!(cx, InvertSelection);
    register_fileman_action!(cx, ActivateSearch);
    register_fileman_action!(cx, ClearSearch);
    register_fileman_action!(cx, ToggleSearchSubfolders);
    register_fileman_action!(cx, SetSearchCurrentFolder);
    register_fileman_action!(cx, SetSearchIncludeSubfolders);
    register_fileman_action!(cx, FocusPathBar);
    register_fileman_action!(cx, GoHome);
    register_fileman_action!(cx, NewTab);
    register_fileman_action!(cx, NewWindow);
    register_fileman_action!(cx, CloseTab);
    register_fileman_action!(cx, AddBookmark);
    register_fileman_action!(cx, RemoveBookmark);
    register_fileman_action!(cx, SortByName);
    register_fileman_action!(cx, SortBySize);
    register_fileman_action!(cx, SortByModified);
    register_fileman_action!(cx, SortByType);
    register_fileman_action!(cx, ToggleSortOrder);
    register_fileman_action!(cx, SortNameAsc);
    register_fileman_action!(cx, SortNameDesc);
    register_fileman_action!(cx, SortSizeAsc);
    register_fileman_action!(cx, SortSizeDesc);
    register_fileman_action!(cx, SortModifiedAsc);
    register_fileman_action!(cx, SortModifiedDesc);
    register_fileman_action!(cx, SortTypeAsc);
    register_fileman_action!(cx, SortTypeDesc);
    register_fileman_action!(cx, Undo);
    register_fileman_action!(cx, Redo);
    register_fileman_action!(cx, OpenTerminal);
    register_fileman_action!(cx, OpenSelection);
    register_fileman_action!(cx, OpenWithSystem);
    register_fileman_action!(cx, ShowProperties);
    register_fileman_action!(cx, ShowSettings);
    register_fileman_action!(cx, ShowAbout);
    register_fileman_action!(cx, GoToParent);
    register_fileman_action!(cx, ZoomIn);
    register_fileman_action!(cx, ZoomOut);
    register_fileman_action!(cx, ZoomReset);
    register_fileman_action!(cx, ViewList);
    register_fileman_action!(cx, ViewIcon);
    register_fileman_action!(cx, ViewCompact);
    register_fileman_action!(cx, ViewTable);
    cx.on_action(|_: &Quit, cx| cx.quit());
}
