use nptk::prelude::*;
use nptk::core::shortcut::Shortcut;
use nptk::core::window::KeyCode;
use nptk_fileman_widgets::file_list::FileListViewMode;
use nptk_fileman_widgets::file_list::SearchScope;
use nptk_fileman_widgets::FilemanSidebar;
use nptk::core::model::SortOrder;
use crate::app::AppState;
use crate::config::FilemanConfig;
use nalgebra::Vector2;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use super::file_list_wrapper::FileListWrapper;
use super::file_operation::FileOperationRequest;

pub fn build_window(context: AppContext, state: AppState) -> impl Widget {
    let navigation = state.navigation.lock().unwrap();
    let initial_path = navigation.get_current_path();
    // Clone navigation path signal for reactive subscription
    let navigation_path_signal = navigation.current_path().clone();
    let nav_clone = state.navigation.clone();
    drop(navigation);

    // Create channels for operations and status (async operations still use channels)
    let (operation_tx, operation_rx) = mpsc::unbounded_channel::<FileOperationRequest>();
    let (status_tx, status_rx) = mpsc::unbounded_channel::<String>();

    // Create focus channel for location bar
    let (focus_tx, focus_rx) = mpsc::unbounded_channel::<()>();
    let (activate_search_tx, activate_search_rx) = mpsc::unbounded_channel::<()>();

    // Register keyboard shortcuts
    // Focus location bar
    context.shortcut_registry.register(
        Shortcut::ctrl(KeyCode::KeyL),
        {
            let tx = focus_tx.clone();
            move || {
                let _ = tx.send(());
                Update::DRAW
            }
        },
    );
    context.shortcut_registry.register(
        Shortcut::new(KeyCode::F6, nptk::core::window::ModifiersState::empty()),
        {
            let tx = focus_tx.clone();
            move || {
                let _ = tx.send(());
                Update::DRAW
            }
        },
    );
    let activate_search_shortcut_tx = activate_search_tx.clone();
    context.shortcut_registry.register(
        Shortcut::ctrl(KeyCode::KeyF),
        move || {
            let _ = activate_search_shortcut_tx.send(());
            Update::DRAW
        },
    );

    let (add_bookmark_tx, add_bookmark_rx) = mpsc::unbounded_channel::<PathBuf>();
    let (remove_bookmark_tx, remove_bookmark_rx) = mpsc::unbounded_channel::<PathBuf>();

    // Create FilemanSidebar
    let mut sidebar = FilemanSidebar::new()
        .with_places(true)
        .with_bookmarks(true)
        .with_devices(true)
        .with_width(state.fileman.sidebar_width() as f32)
        .with_current_path_signal(navigation_path_signal.clone())
        .with_add_bookmark_receiver(add_bookmark_rx)
        .with_remove_bookmark_receiver(remove_bookmark_rx)
        .with_bookmark_status_sender(status_tx.clone());

    // Take the navigation receiver for FileListWrapper
    let sidebar_nav_rx = sidebar
        .take_navigation_receiver()
        .expect("FilemanSidebar should provide navigation receiver");

    let search_scope_signal = StateSignal::new(SearchScope::CurrentFolder);
    let search_query_signal = StateSignal::new(String::new());
    let show_hidden_files_signal = StateSignal::new(state.fileman.initial_show_hidden());
    let (search_tx, search_rx) = mpsc::unbounded_channel::<String>();
    let (folder_duplicate_done_tx, folder_duplicate_done_rx) = mpsc::unbounded_channel::<()>();

    // Create FileList wrapper that syncs with navigation state
    let config_path = FilemanConfig::config_file_path();
    let delete_policy = Arc::new(Mutex::new(state.fileman.delete_policy()));
    let terminal_command = Arc::new(Mutex::new(
        state.fileman.terminal_command().map(str::to_string),
    ));

    let mut file_list_wrapper = FileListWrapper::new(
        initial_path.clone(),
        nav_clone.clone(),
        sidebar_nav_rx,
        operation_tx.clone(),
        operation_rx,
        status_tx.clone(),
        navigation_path_signal.clone(),
        search_scope_signal.clone(),
        search_query_signal.clone(),
        Some(search_rx),
        show_hidden_files_signal.clone(),
        folder_duplicate_done_tx,
        folder_duplicate_done_rx,
        delete_policy,
        terminal_command,
        config_path,
    );

    if let Some(mode) = state.fileman.default_view_mode() {
        file_list_wrapper.view_mode_signal().set(mode);
    }
    file_list_wrapper.apply_startup_folder_view(&state.fileman);

    // Set file list to grow and fill remaining space
    file_list_wrapper.set_layout_style(LayoutStyle {
        size: Vector2::new(Dimension::auto(), Dimension::percent(1.0)),
        flex_grow: 1.0, // Grow to fill remaining horizontal space
        flex_shrink: 1.0, // Allow shrinking if needed
        ..Default::default()
    });

    // Clone selected paths signal from FileList for ToolbarWrapper and StatusBarWrapper
    let selected_paths_signal = file_list_wrapper.selected_paths_signal().clone();

    // Create ToolbarWrapper
    let (toolbar_wrapper, toolbar_nav_tx) = crate::toolbar::ToolbarWrapper::new(
        nav_clone.clone(),
        operation_tx.clone(),
        navigation_path_signal.clone(),
        selected_paths_signal.clone(),
        file_list_wrapper.view_mode_signal().clone(),
    );
    let view_mode_signal = file_list_wrapper.view_mode_signal().clone();
    let search_scope_for_menubar = search_scope_signal.clone();

    let menu_bar = crate::menus::build_reference_menubar(
        status_tx.clone(),
        crate::menus::ReferenceMenubarActions {
            focus_location: Arc::new({
                let tx = focus_tx.clone();
                move || {
                    let _ = tx.send(());
                }
            }),
            activate_search: Arc::new({
                let tx = activate_search_tx.clone();
                move || {
                    let _ = tx.send(());
                }
            }),
            navigate_home: Arc::new({
                let tx = toolbar_nav_tx.clone();
                move || {
                    let _ = tx.send(crate::toolbar::NavigationAction::Home);
                }
            }),
            navigate_back: Arc::new({
                let tx = toolbar_nav_tx.clone();
                move || {
                    let _ = tx.send(crate::toolbar::NavigationAction::Back);
                }
            }),
            navigate_forward: Arc::new({
                let tx = toolbar_nav_tx.clone();
                move || {
                    let _ = tx.send(crate::toolbar::NavigationAction::Forward);
                }
            }),
            navigate_up: Arc::new({
                let tx = toolbar_nav_tx.clone();
                move || {
                    let _ = tx.send(crate::toolbar::NavigationAction::Up);
                }
            }),
            set_view_list: Arc::new({
                let signal = view_mode_signal.clone();
                move || {
                    signal.set(FileListViewMode::List);
                }
            }),
            set_view_icon: Arc::new({
                let signal = view_mode_signal.clone();
                move || {
                    signal.set(FileListViewMode::Icon);
                }
            }),
            set_view_compact: Arc::new({
                let signal = view_mode_signal.clone();
                move || {
                    signal.set(FileListViewMode::Compact);
                }
            }),
            set_view_table: Arc::new({
                let signal = view_mode_signal.clone();
                move || {
                    signal.set(FileListViewMode::Table);
                }
            }),
            show_properties: Arc::new({
                let operation_tx = operation_tx.clone();
                let selected_paths_signal = selected_paths_signal.clone();
                let status_tx = status_tx.clone();
                move || {
                    let paths = (*selected_paths_signal.get()).clone();
                    if paths.is_empty() {
                        let _ = status_tx.send("Properties: nothing selected".to_string());
                    } else {
                        let _ = operation_tx.send(FileOperationRequest::Properties(paths));
                    }
                }
            }),
            refresh_list: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::Refresh);
                }
            }),
            copy_selection: Arc::new({
                let operation_tx = operation_tx.clone();
                let selected_paths_signal = selected_paths_signal.clone();
                let status_tx = status_tx.clone();
                move || {
                    let paths = (*selected_paths_signal.get()).clone();
                    if paths.is_empty() {
                        let _ = status_tx.send("Copy: nothing selected".to_string());
                    } else {
                        let _ = operation_tx.send(FileOperationRequest::Copy(paths));
                    }
                }
            }),
            cut_selection: Arc::new({
                let operation_tx = operation_tx.clone();
                let selected_paths_signal = selected_paths_signal.clone();
                let status_tx = status_tx.clone();
                move || {
                    let paths = (*selected_paths_signal.get()).clone();
                    if paths.is_empty() {
                        let _ = status_tx.send("Cut: nothing selected".to_string());
                    } else {
                        let _ = operation_tx.send(FileOperationRequest::Cut(paths));
                    }
                }
            }),
            paste_clipboard: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::Paste);
                }
            }),
            new_folder: Arc::new({
                let operation_tx = operation_tx.clone();
                let navigation_path_signal = navigation_path_signal.clone();
                move || {
                    let parent = (*navigation_path_signal.get()).clone();
                    let _ = operation_tx.send(FileOperationRequest::PromptCreateDirectory(parent));
                }
            }),
            rename_selection: Arc::new({
                let operation_tx = operation_tx.clone();
                let selected_paths_signal = selected_paths_signal.clone();
                let status_tx = status_tx.clone();
                move || {
                    let paths = (*selected_paths_signal.get()).clone();
                    match paths.len() {
                        0 => {
                            let _ = status_tx.send("Rename: select a single item".to_string());
                        }
                        1 => {
                            let _ =
                                operation_tx.send(FileOperationRequest::PromptRename(paths[0].clone()));
                        }
                        _ => {
                            let _ = status_tx.send("Rename: select only one item".to_string());
                        }
                    }
                }
            }),
            delete_selection: Arc::new({
                let operation_tx = operation_tx.clone();
                let selected_paths_signal = selected_paths_signal.clone();
                let status_tx = status_tx.clone();
                move || {
                    let paths = (*selected_paths_signal.get()).clone();
                    if paths.is_empty() {
                        let _ = status_tx.send("Move to Trash: nothing selected".to_string());
                    } else {
                        let _ = operation_tx.send(FileOperationRequest::Delete(paths));
                    }
                }
            }),
            delete_permanent_selection: Arc::new({
                let operation_tx = operation_tx.clone();
                let selected_paths_signal = selected_paths_signal.clone();
                let status_tx = status_tx.clone();
                move || {
                    let paths = (*selected_paths_signal.get()).clone();
                    if paths.is_empty() {
                        let _ = status_tx.send("Delete permanently: nothing selected".to_string());
                    } else {
                        let _ = operation_tx.send(FileOperationRequest::DeletePermanent(paths));
                    }
                }
            }),
            sort_name_asc: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::Sort(0, SortOrder::Ascending));
                }
            }),
            sort_name_desc: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::Sort(0, SortOrder::Descending));
                }
            }),
            sort_size_asc: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::Sort(1, SortOrder::Ascending));
                }
            }),
            sort_size_desc: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::Sort(1, SortOrder::Descending));
                }
            }),
            sort_modified_asc: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::Sort(3, SortOrder::Ascending));
                }
            }),
            sort_modified_desc: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::Sort(3, SortOrder::Descending));
                }
            }),
            sort_type_asc: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::Sort(2, SortOrder::Ascending));
                }
            }),
            sort_type_desc: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::Sort(2, SortOrder::Descending));
                }
            }),
            set_search_current_folder: Arc::new({
                let scope_signal = search_scope_for_menubar.clone();
                move || {
                    scope_signal.set(SearchScope::CurrentFolder);
                }
            }),
            set_search_include_subfolders: Arc::new({
                let scope_signal = search_scope_for_menubar.clone();
                move || {
                    scope_signal.set(SearchScope::FolderAndSubfolders);
                }
            }),
            select_all: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::SelectAll);
                }
            }),
            deselect_all: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::DeselectAll);
                }
            }),
            invert_selection: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::InvertSelection);
                }
            }),
            toggle_show_hidden_files: Arc::new({
                let signal = show_hidden_files_signal.clone();
                let status_tx = status_tx.clone();
                move || {
                    let next = !*signal.get();
                    signal.set(next);
                    let msg = if next {
                        "Showing hidden files"
                    } else {
                        "Hiding hidden files"
                    };
                    let _ = status_tx.send(msg.to_string());
                }
            }),
            new_file: Arc::new({
                let operation_tx = operation_tx.clone();
                let navigation_path_signal = navigation_path_signal.clone();
                move || {
                    let parent = (*navigation_path_signal.get()).clone();
                    let _ = operation_tx.send(FileOperationRequest::PromptCreateFile(parent));
                }
            }),
            open_selection: Arc::new({
                let operation_tx = operation_tx.clone();
                let selected_paths_signal = selected_paths_signal.clone();
                let status_tx = status_tx.clone();
                move || {
                    let paths = (*selected_paths_signal.get()).clone();
                    if paths.is_empty() {
                        let _ = status_tx.send("Open: nothing selected".to_string());
                    } else {
                        let _ = operation_tx.send(FileOperationRequest::OpenSelection);
                    }
                }
            }),
            duplicate_selection: Arc::new({
                let operation_tx = operation_tx.clone();
                let selected_paths_signal = selected_paths_signal.clone();
                let status_tx = status_tx.clone();
                move || {
                    let paths = (*selected_paths_signal.get()).clone();
                    if paths.is_empty() {
                        let _ = status_tx.send("Duplicate: nothing selected".to_string());
                    } else {
                        let _ = operation_tx.send(FileOperationRequest::Duplicate(paths));
                    }
                }
            }),
            add_bookmark_current_folder: Arc::new({
                let bookmark_tx = add_bookmark_tx.clone();
                let navigation_path_signal = navigation_path_signal.clone();
                let status_tx = status_tx.clone();
                move || {
                    let path = (*navigation_path_signal.get()).clone();
                    if !path.is_dir() {
                        let _ = status_tx.send("Bookmarks: current path is not a folder".to_string());
                    } else {
                        let _ = bookmark_tx.send(path);
                    }
                }
            }),
            remove_bookmark_current_folder: Arc::new({
                let bookmark_tx = remove_bookmark_tx.clone();
                let navigation_path_signal = navigation_path_signal.clone();
                let status_tx = status_tx.clone();
                move || {
                    let path = (*navigation_path_signal.get()).clone();
                    if !path.is_dir() {
                        let _ = status_tx.send("Bookmarks: current path is not a folder".to_string());
                    } else {
                        let _ = bookmark_tx.send(path);
                    }
                }
            }),
            open_terminal_here: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::OpenTerminalHere);
                }
            }),
            show_about: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::ShowAbout);
                }
            }),
            configure_fileman: Arc::new({
                let operation_tx = operation_tx.clone();
                move || {
                    let _ = operation_tx.send(FileOperationRequest::ShowSettings);
                }
            }),
        },
    );

    // Create FileLocationBar (shared search_query_signal for live search)
    use nptk_fileman_widgets::location_bar::FileLocationBar;

    let nav_tx_clone = toolbar_nav_tx.clone();
    let location_bar = FileLocationBar::new(
        navigation_path_signal.clone(),
        search_query_signal,
        Some(search_tx),
    )
    .with_focus_receiver(focus_rx)
    .with_activate_search_receiver(activate_search_rx)
    .with_search_scope_signal(search_scope_signal)
    .with_on_navigate(move |path| {
        let _ = nav_tx_clone.send(crate::toolbar::NavigationAction::NavigateTo(path));
        Update::DRAW
    });

    // Create FileStatusBar
    use nptk_fileman_widgets::status_bar::FileStatusBar;

    let statusbar = FileStatusBar::new(
        navigation_path_signal.clone(),
        selected_paths_signal.clone(),
        file_list_wrapper.entries_signal().clone(),
    )
    .with_message_receiver(status_rx);

    // Build main layout
    Container::new(vec![
        Box::new(menu_bar),
        // Toolbar area
        Box::new(Container::new(vec![
            Box::new(toolbar_wrapper),
            Box::new(location_bar),
        ])
        .with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
            flex_direction: FlexDirection::Column,
            gap: Vector2::new(LengthPercentage::length(0.0), LengthPercentage::length(4.0)),
            ..Default::default()
        })),
        // Content area (sidebar + file list)
        Box::new(Container::new(vec![
            Box::new(sidebar),
            Box::new(file_list_wrapper),
        ])
        .with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
            flex_direction: FlexDirection::Row,
            gap: Vector2::new(LengthPercentage::length(0.0), LengthPercentage::length(0.0)),
            ..Default::default()
        })),
        // Statusbar
        Box::new(statusbar),
    ])
    .with_layout_style(LayoutStyle {
        size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
        flex_direction: FlexDirection::Column,
        ..Default::default()
    })
}
