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

    let status_tx_help = status_tx.clone();
    let help_menu = MenuTemplate::new("Help")
        .add_item(
            MenuItem::new(MenuCommand::Custom(1301), "About")
                .with_action(move || {
                    let _ = status_tx_help.send(format!(
                        "About: {} {}",
                        env!("CARGO_PKG_NAME"),
                        env!("CARGO_PKG_VERSION")
                    ));
                    Update::DRAW
                }),
        );

    MenuBar::new()
        .with_template(file_menu)
        .with_template(edit_menu)
        .with_template(view_menu)
        .with_template(help_menu)
        .with_menu_manager(menu_manager)
}
