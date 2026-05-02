use nptk::prelude::*;
use nptk::core::menu::MenuCommand;
use nptk::core::menu::manager::MenuManager;
use nptk::core::menu::unified::{MenuItem, MenuTemplate};
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct ReferenceMenubarActions {
    pub focus_location: Arc<dyn Fn() + Send + Sync>,
    pub activate_search: Arc<dyn Fn() + Send + Sync>,
    pub navigate_home: Arc<dyn Fn() + Send + Sync>,
    pub navigate_back: Arc<dyn Fn() + Send + Sync>,
    pub navigate_forward: Arc<dyn Fn() + Send + Sync>,
    pub navigate_up: Arc<dyn Fn() + Send + Sync>,
    pub set_view_list: Arc<dyn Fn() + Send + Sync>,
    pub set_view_icon: Arc<dyn Fn() + Send + Sync>,
    pub set_view_compact: Arc<dyn Fn() + Send + Sync>,
    pub set_view_table: Arc<dyn Fn() + Send + Sync>,
    pub show_properties: Arc<dyn Fn() + Send + Sync>,
    pub refresh_list: Arc<dyn Fn() + Send + Sync>,
    pub copy_selection: Arc<dyn Fn() + Send + Sync>,
    pub cut_selection: Arc<dyn Fn() + Send + Sync>,
    pub paste_clipboard: Arc<dyn Fn() + Send + Sync>,
    pub new_folder: Arc<dyn Fn() + Send + Sync>,
    pub rename_selection: Arc<dyn Fn() + Send + Sync>,
    pub delete_selection: Arc<dyn Fn() + Send + Sync>,
    pub delete_permanent_selection: Arc<dyn Fn() + Send + Sync>,
    pub sort_name_asc: Arc<dyn Fn() + Send + Sync>,
    pub sort_name_desc: Arc<dyn Fn() + Send + Sync>,
    pub sort_size_asc: Arc<dyn Fn() + Send + Sync>,
    pub sort_size_desc: Arc<dyn Fn() + Send + Sync>,
    pub sort_modified_asc: Arc<dyn Fn() + Send + Sync>,
    pub sort_modified_desc: Arc<dyn Fn() + Send + Sync>,
    pub sort_type_asc: Arc<dyn Fn() + Send + Sync>,
    pub sort_type_desc: Arc<dyn Fn() + Send + Sync>,
    pub set_search_current_folder: Arc<dyn Fn() + Send + Sync>,
    pub set_search_include_subfolders: Arc<dyn Fn() + Send + Sync>,
    pub select_all: Arc<dyn Fn() + Send + Sync>,
    pub deselect_all: Arc<dyn Fn() + Send + Sync>,
    pub invert_selection: Arc<dyn Fn() + Send + Sync>,
    pub toggle_show_hidden_files: Arc<dyn Fn() + Send + Sync>,
    pub new_file: Arc<dyn Fn() + Send + Sync>,
    pub open_selection: Arc<dyn Fn() + Send + Sync>,
    pub duplicate_selection: Arc<dyn Fn() + Send + Sync>,
    pub add_bookmark_current_folder: Arc<dyn Fn() + Send + Sync>,
    pub remove_bookmark_current_folder: Arc<dyn Fn() + Send + Sync>,
    pub open_terminal_here: Arc<dyn Fn() + Send + Sync>,
    pub show_about: Arc<dyn Fn() + Send + Sync>,
    pub configure_fileman: Arc<dyn Fn() + Send + Sync>,
}

/// Build a reference menubar for smoke-testing integration in fileman.
pub fn build_reference_menubar(
    status_tx: mpsc::UnboundedSender<String>,
    actions: ReferenceMenubarActions,
) -> MenuBar {
    let menu_manager = MenuManager::new();
    let navigate_home = actions.navigate_home.clone();
    let navigate_back = actions.navigate_back.clone();
    let navigate_forward = actions.navigate_forward.clone();
    let navigate_up = actions.navigate_up.clone();
    let focus_location = actions.focus_location.clone();
    let activate_search = actions.activate_search.clone();
    let set_view_list = actions.set_view_list.clone();
    let set_view_icon = actions.set_view_icon.clone();
    let set_view_compact = actions.set_view_compact.clone();
    let set_view_table = actions.set_view_table.clone();
    let show_properties = actions.show_properties.clone();
    let refresh_list = actions.refresh_list.clone();
    let copy_selection = actions.copy_selection.clone();
    let cut_selection = actions.cut_selection.clone();
    let paste_clipboard = actions.paste_clipboard.clone();
    let new_folder = actions.new_folder.clone();
    let rename_selection = actions.rename_selection.clone();
    let delete_selection = actions.delete_selection.clone();
    let delete_permanent_selection = actions.delete_permanent_selection.clone();
    let sort_name_asc = actions.sort_name_asc.clone();
    let sort_name_desc = actions.sort_name_desc.clone();
    let sort_size_asc = actions.sort_size_asc.clone();
    let sort_size_desc = actions.sort_size_desc.clone();
    let sort_modified_asc = actions.sort_modified_asc.clone();
    let sort_modified_desc = actions.sort_modified_desc.clone();
    let sort_type_asc = actions.sort_type_asc.clone();
    let sort_type_desc = actions.sort_type_desc.clone();
    let set_search_current_folder = actions.set_search_current_folder.clone();
    let set_search_include_subfolders = actions.set_search_include_subfolders.clone();
    let select_all = actions.select_all.clone();
    let deselect_all = actions.deselect_all.clone();
    let invert_selection = actions.invert_selection.clone();
    let toggle_show_hidden_files = actions.toggle_show_hidden_files.clone();
    let new_file = actions.new_file.clone();
    let open_selection = actions.open_selection.clone();
    let duplicate_selection = actions.duplicate_selection.clone();
    let add_bookmark_current_folder = actions.add_bookmark_current_folder.clone();
    let remove_bookmark_current_folder = actions.remove_bookmark_current_folder.clone();
    let open_terminal_here = actions.open_terminal_here.clone();
    let show_about = actions.show_about.clone();
    let configure_fileman = actions.configure_fileman.clone();

    let status_tx_file_home = status_tx.clone();

    let file_menu = MenuTemplate::new("File")
        .add_item(
            MenuItem::new(MenuCommand::Custom(1001), "Home")
                .with_action(move || {
                    (navigate_home)();
                    let _ = status_tx_file_home.send("Menu: File -> Home".to_string());
                    Update::DRAW
                }),
        )
        .add_item(MenuItem::separator())
        .add_item(
            MenuItem::new(MenuCommand::Custom(1006), "New Folder")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (new_folder)();
                        let _ = status_tx.send("Menu: File -> New Folder".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1007), "New File")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (new_file)();
                        let _ = status_tx.send("Menu: File -> New File".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1008), "Open")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (open_selection)();
                        let _ = status_tx.send("Menu: File -> Open".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1002), "Properties")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (show_properties)();
                        let _ = status_tx.send("Menu: File -> Properties".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(MenuItem::separator())
        .add_item(
            MenuItem::new(MenuCommand::Custom(1003), "Back")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (navigate_back)();
                        let _ = status_tx.send("Menu: File -> Back".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1004), "Forward")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (navigate_forward)();
                        let _ = status_tx.send("Menu: File -> Forward".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1005), "Up")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (navigate_up)();
                        let _ = status_tx.send("Menu: File -> Up".to_string());
                        Update::DRAW
                    }
                }),
        );

    let edit_menu = MenuTemplate::new("Edit")
        .add_item(
            MenuItem::new(MenuCommand::Custom(1101), "Focus Location Bar")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (focus_location)();
                        let _ = status_tx
                            .send("Menu: Edit -> Focus Location Bar".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1102), "Activate Search")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (activate_search)();
                        let _ = status_tx.send("Menu: Edit -> Activate Search".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1108), "Select All")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (select_all)();
                        let _ = status_tx.send("Menu: Edit -> Select All".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1110), "Deselect All")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (deselect_all)();
                        let _ = status_tx.send("Menu: Edit -> Deselect All".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1111), "Invert Selection")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (invert_selection)();
                        let _ = status_tx.send("Menu: Edit -> Invert Selection".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(MenuItem::separator())
        .add_item(
            MenuItem::new(MenuCommand::Custom(1103), "Copy")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (copy_selection)();
                        let _ = status_tx.send("Menu: Edit -> Copy".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1104), "Cut")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (cut_selection)();
                        let _ = status_tx.send("Menu: Edit -> Cut".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1105), "Paste")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (paste_clipboard)();
                        let _ = status_tx.send("Menu: Edit -> Paste".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(MenuItem::separator())
        .add_item(
            MenuItem::new(MenuCommand::Custom(1106), "Rename")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (rename_selection)();
                        let _ = status_tx.send("Menu: Edit -> Rename".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1107), "Move to Trash")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (delete_selection)();
                        let _ = status_tx.send("Menu: Edit -> Move to Trash".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1112), "Delete Permanently")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (delete_permanent_selection)();
                        let _ = status_tx.send("Menu: Edit -> Delete Permanently".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1109), "Duplicate")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (duplicate_selection)();
                        let _ = status_tx.send("Menu: Edit -> Duplicate".to_string());
                        Update::DRAW
                    }
                }),
        );

    let mut sort_menu = MenuTemplate::new("menubar-sort");
    sort_menu = sort_menu.add_item(
        MenuItem::new(MenuCommand::Custom(1210), "Name (Ascending)")
            .with_action({
                let status_tx = status_tx.clone();
                move || {
                    (sort_name_asc)();
                    let _ = status_tx.send("Menu: View -> Sort -> Name (Ascending)".to_string());
                    Update::DRAW
                }
            }),
    );
    sort_menu = sort_menu.add_item(
        MenuItem::new(MenuCommand::Custom(1211), "Name (Descending)")
            .with_action({
                let status_tx = status_tx.clone();
                move || {
                    (sort_name_desc)();
                    let _ = status_tx.send("Menu: View -> Sort -> Name (Descending)".to_string());
                    Update::DRAW
                }
            }),
    );
    sort_menu = sort_menu.add_item(
        MenuItem::new(MenuCommand::Custom(1212), "Size (Ascending)")
            .with_action({
                let status_tx = status_tx.clone();
                move || {
                    (sort_size_asc)();
                    let _ = status_tx.send("Menu: View -> Sort -> Size (Ascending)".to_string());
                    Update::DRAW
                }
            }),
    );
    sort_menu = sort_menu.add_item(
        MenuItem::new(MenuCommand::Custom(1213), "Size (Descending)")
            .with_action({
                let status_tx = status_tx.clone();
                move || {
                    (sort_size_desc)();
                    let _ = status_tx.send("Menu: View -> Sort -> Size (Descending)".to_string());
                    Update::DRAW
                }
            }),
    );
    sort_menu = sort_menu.add_item(
        MenuItem::new(MenuCommand::Custom(1214), "Date Modified (Ascending)")
            .with_action({
                let status_tx = status_tx.clone();
                move || {
                    (sort_modified_asc)();
                    let _ = status_tx.send("Menu: View -> Sort -> Date Modified (Ascending)".to_string());
                    Update::DRAW
                }
            }),
    );
    sort_menu = sort_menu.add_item(
        MenuItem::new(MenuCommand::Custom(1215), "Date Modified (Descending)")
            .with_action({
                let status_tx = status_tx.clone();
                move || {
                    (sort_modified_desc)();
                    let _ = status_tx.send("Menu: View -> Sort -> Date Modified (Descending)".to_string());
                    Update::DRAW
                }
            }),
    );
    sort_menu = sort_menu.add_item(
        MenuItem::new(MenuCommand::Custom(1216), "Type (Ascending)")
            .with_action({
                let status_tx = status_tx.clone();
                move || {
                    (sort_type_asc)();
                    let _ = status_tx.send("Menu: View -> Sort -> Type (Ascending)".to_string());
                    Update::DRAW
                }
            }),
    );
    sort_menu = sort_menu.add_item(
        MenuItem::new(MenuCommand::Custom(1217), "Type (Descending)")
            .with_action({
                let status_tx = status_tx.clone();
                move || {
                    (sort_type_desc)();
                    let _ = status_tx.send("Menu: View -> Sort -> Type (Descending)".to_string());
                    Update::DRAW
                }
            }),
    );

    let view_menu = MenuTemplate::new("View")
        .add_item(
            MenuItem::new(MenuCommand::Custom(1201), "Refresh")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (refresh_list)();
                        let _ = status_tx.send("Menu: View -> Refresh".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1240), "Sort By").with_submenu(sort_menu),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1220), "Search: Current Folder")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (set_search_current_folder)();
                        let _ = status_tx.send("Menu: View -> Search: Current Folder".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1221), "Search: Include Subfolders")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (set_search_include_subfolders)();
                        let _ = status_tx
                            .send("Menu: View -> Search: Include Subfolders".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1222), "Show Hidden Files")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (toggle_show_hidden_files)();
                        let _ = status_tx.send("Menu: View -> Show Hidden Files (toggle)".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(MenuItem::separator())
        .add_item(
            MenuItem::new(MenuCommand::Custom(1202), "List View")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (set_view_list)();
                        let _ = status_tx.send("Menu: View -> List View".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1203), "Icon View")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (set_view_icon)();
                        let _ = status_tx.send("Menu: View -> Icon View".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1204), "Compact View")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (set_view_compact)();
                        let _ = status_tx.send("Menu: View -> Compact View".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1205), "Table View")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (set_view_table)();
                        let _ = status_tx.send("Menu: View -> Table View".to_string());
                        Update::DRAW
                    }
                }),
        );

    let tools_menu = MenuTemplate::new("Tools")
        .add_item(
            MenuItem::new(MenuCommand::Custom(1501), "Open Terminal Here")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (open_terminal_here)();
                        let _ = status_tx.send("Menu: Tools -> Open Terminal Here".to_string());
                        Update::DRAW
                    }
                }),
        );

    let bookmarks_menu = MenuTemplate::new("Bookmarks")
        .add_item(
            MenuItem::new(MenuCommand::Custom(1401), "Add Current Folder")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (add_bookmark_current_folder)();
                        let _ = status_tx.send("Menu: Bookmarks -> Add Current Folder".to_string());
                        Update::DRAW
                    }
                }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(1402), "Remove Current Folder")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (remove_bookmark_current_folder)();
                        let _ = status_tx
                            .send("Menu: Bookmarks -> Remove Current Folder".to_string());
                        Update::DRAW
                    }
                }),
        );

    let settings_menu = MenuTemplate::new("Settings").add_item(
        MenuItem::new(MenuCommand::Custom(1601), "Configure Fileman").with_action({
            let status_tx = status_tx.clone();
            move || {
                (configure_fileman)();
                let _ = status_tx.send("Menu: Settings -> Configure Fileman".to_string());
                Update::DRAW
            }
        }),
    );

    let help_menu = MenuTemplate::new("Help")
        .add_item(
            MenuItem::new(MenuCommand::Custom(1301), "About")
                .with_action({
                    let status_tx = status_tx.clone();
                    move || {
                        (show_about)();
                        let _ = status_tx.send("Menu: Help -> About".to_string());
                        Update::DRAW
                    }
                }),
        );

    MenuBar::new()
        .with_template(file_menu)
        .with_template(edit_menu)
        .with_template(view_menu)
        .with_template(tools_menu)
        .with_template(bookmarks_menu)
        .with_template(settings_menu)
        .with_template(help_menu)
        .with_menu_manager(menu_manager)
}
