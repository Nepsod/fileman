use nptk::prelude::*;
use async_trait::async_trait;
use nptk::core::signal::eval::EvalSignal;
use nptk::core::shortcut::Shortcut;
use nptk::core::window::KeyCode;
use nptk_fileman_widgets::file_list::{FileList, FileListOperation, SearchScope};
use nptk_fileman_widgets::file_list::FileListViewMode;
use nptk::services::filesystem::entry::FileEntry;
use nptk_fileman_widgets::FilemanSidebar;
// use nptk::widgets::breadcrumbs::{Breadcrumbs, BreadcrumbItem}; // Unused
use crate::app::AppState;
use crate::config::DeletePolicy;
use crate::operations;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;
use nalgebra::Vector2;
use nptk::core::menu::unified::{MenuTemplate, MenuItem};
use nptk::core::menu::MenuCommand;
use nptk::core::vg::kurbo::Point;
use nptk::services::clipboard::ClipboardService;
use nptk::core::model::SortOrder;

/// File operation requests that can be sent from UI to be processed
#[derive(Debug, Clone)]
pub enum FileOperationRequest {
    /// Move selected paths to trash after confirmation.
    Delete(Vec<PathBuf>),
    /// Permanently delete after confirmation (e.g. Shift+Delete from list).
    DeletePermanent(Vec<PathBuf>),
    // CreateDirectory { parent: PathBuf, name: String }, // Unused
    // Rename { from: PathBuf, to: PathBuf }, // Unused
    PromptRename(PathBuf), // Prompt for new name for single file
    PromptCreateDirectory(PathBuf), // Prompt for new directory name in parent
    PromptCreateFile(PathBuf),      // Prompt for new empty file name in parent
    Properties(Vec<PathBuf>),
    Copy(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
    Paste,
    /// Reload entries for the current path (same as list context "refresh" behavior).
    Refresh,
    /// Sort file list by column index and order (same as list context menu).
    Sort(usize, SortOrder),
    /// Select all listed entries (same as Ctrl+A in the file list).
    SelectAll,
    /// Clear file list selection.
    DeselectAll,
    /// Invert file list selection.
    InvertSelection,
    /// Open selected paths (folders navigate, files launch default app).
    OpenSelection,
    Duplicate(Vec<PathBuf>),
    /// Spawn terminal with cwd = current folder.
    OpenTerminalHere,
}

/// Wrapper widget that manages FileList and connects it to navigation state
struct FileListWrapper {
    file_list: FileList,
    navigation: Arc<Mutex<crate::navigation::NavigationState>>,
    navigation_rx: Option<mpsc::UnboundedReceiver<PathBuf>>,
    // Reactive signals - cloned from NavigationState and FileList
    navigation_path_signal: StateSignal<PathBuf>,
    file_list_path_signal: StateSignal<PathBuf>,
    signals_hooked: bool,
    // File operation processing - receives from FileList widget (already confirmed)
    file_list_operation_rx: Option<mpsc::UnboundedReceiver<FileListOperation>>,
    // File operation processing - receives from toolbar/other UI (needs confirmation)
    operation_tx: mpsc::UnboundedSender<FileOperationRequest>,
    operation_rx: Option<mpsc::UnboundedReceiver<FileOperationRequest>>,
    // Status message sender (for displaying operation results)
    status_tx: Option<mpsc::UnboundedSender<String>>,
    /// After confirm: paths and whether to delete permanently (otherwise move to trash).
    pending_delete_confirmation: Arc<Mutex<Option<(Vec<PathBuf>, bool)>>>,
    // Pending rename operations (from dialog)
    pending_rename: Arc<Mutex<Option<(PathBuf, String)>>>,
    // Pending create directory operations (from dialog)
    pending_create_dir: Arc<Mutex<Option<(PathBuf, String)>>>,
    pending_create_file: Arc<Mutex<Option<(PathBuf, String)>>>,
    // Clipboard service
    clipboard: Arc<Mutex<ClipboardService>>,
    /// Notify UI thread after async folder duplicate completes (`try_recv` in `update`).
    folder_duplicate_done_tx: mpsc::UnboundedSender<()>,
    folder_duplicate_done_rx: Option<mpsc::UnboundedReceiver<()>>,
    delete_policy: DeletePolicy,
    /// From config `[System].Terminal`; superseded by `TERMINAL` env at spawn time.
    terminal_command: Option<String>,
}

impl FileListWrapper {
    fn new(
        initial_path: PathBuf,
        navigation: Arc<Mutex<crate::navigation::NavigationState>>,
        navigation_rx: mpsc::UnboundedReceiver<PathBuf>,
        operation_tx: mpsc::UnboundedSender<FileOperationRequest>,
        operation_rx: mpsc::UnboundedReceiver<FileOperationRequest>,
        status_tx: mpsc::UnboundedSender<String>,
        navigation_path_signal: StateSignal<PathBuf>,
        search_scope_signal: StateSignal<SearchScope>,
        search_query_signal: StateSignal<String>,
        search_pending_rx: Option<mpsc::UnboundedReceiver<String>>,
        show_hidden_files_signal: StateSignal<bool>,
        folder_duplicate_done_tx: mpsc::UnboundedSender<()>,
        folder_duplicate_done_rx: mpsc::UnboundedReceiver<()>,
        delete_policy: DeletePolicy,
        terminal_command: Option<String>,
    ) -> Self {
        // Create channel for FileList operations
        let (file_list_op_tx, file_list_op_rx) = mpsc::unbounded_channel::<FileListOperation>();
        
        // Create FileList with shared search_query for live search; search_pending_rx avoids signal write from TextInput
        let file_list = FileList::new_with_operations(
            initial_path.clone(),
            Some(file_list_op_tx.clone()),
            None,
            Some(search_query_signal),
            search_pending_rx,
        )
            .with_search_scope_signal(search_scope_signal)
            .with_show_hidden_files_signal(show_hidden_files_signal);
        
        // Clone signals from FileList for reactive subscription
        let file_list_path_signal = file_list.current_path_signal().clone();
        
        // Initialize clipboard
        let clipboard = Arc::new(Mutex::new(ClipboardService::new()));
        
        let file_list = file_list.with_on_context_menu({
            let op_tx = file_list_op_tx.clone();
            move |path: PathBuf, pos: Vector2<f64>, context: AppContext| {
                // Create native context menu using NPTK's MenuManager
                let mut template = MenuTemplate::new("context-menu");
                
                // Open action
                if path.is_dir() || path.is_file() {
                    let op_open = op_tx.clone();
                    let path_open = path.clone();
                    template = template.add_item(
                        MenuItem::new(MenuCommand::Custom(1), "Open")
                            .with_action(move || {
                                let _ =
                                    op_open.send(FileListOperation::OpenPaths(vec![path_open.clone()]));
                                Update::DRAW
                            })
                    );
                }
                
                // Separator
                template = template.add_item(MenuItem::separator());
                
                let op_tx_copy = op_tx.clone();
                let path_clone = path.clone();
                template = template.add_item(
                    MenuItem::new(MenuCommand::Custom(2), "Copy")
                        .with_action(move || {
                            let _ = op_tx_copy.send(FileListOperation::Copy(vec![path_clone.clone()]));
                            Update::empty()
                        })
                );

                let op_tx_cut = op_tx.clone();
                let path_clone = path.clone();
                template = template.add_item(
                    MenuItem::new(MenuCommand::Custom(3), "Cut")
                        .with_action(move || {
                             let _ = op_tx_cut.send(FileListOperation::Cut(vec![path_clone.clone()]));
                            Update::empty()
                        })
                );

                let op_tx_paste = op_tx.clone();
                template = template.add_item(
                    MenuItem::new(MenuCommand::Custom(4), "Paste")
                        .with_action(move || {
                             let _ = op_tx_paste.send(FileListOperation::Paste);
                            Update::empty()
                        })
                );

                let op_tx_dup = op_tx.clone();
                let path_dup = path.clone();
                template = template.add_item(
                    MenuItem::new(MenuCommand::Custom(41), "Duplicate")
                        .with_action(move || {
                            let _ = op_tx_dup.send(FileListOperation::Duplicate(vec![path_dup.clone()]));
                            Update::empty()
                        })
                );
                
                // Separator
                template = template.add_item(MenuItem::separator());

                // Properties action
                let op_tx_clone = op_tx.clone();
                let path_clone = path.clone();
                template = template.add_item(
                    MenuItem::new(MenuCommand::Custom(2), "Properties")
                        .with_action(move || {
                             let _ = op_tx_clone.send(FileListOperation::Properties(vec![path_clone.clone()]));
                             Update::DRAW
                        })
                );
                
                // Rename action
                let op_tx_clone = op_tx.clone();
                let path_clone = path.clone();
                template = template.add_item(
                    MenuItem::new(MenuCommand::Custom(4), "Rename")
                        .with_action(move || {
                             let _ = op_tx_clone.send(FileListOperation::PromptRename(path_clone.clone()));
                             Update::DRAW
                        })
                );
                
                // Delete → trash
                let op_tx_clone = op_tx.clone();
                let path_clone = path.clone();
                template = template.add_item(
                    MenuItem::new(MenuCommand::Custom(3), "Move to Trash")
                        .with_action(move || {
                            let _ = op_tx_clone
                                .send(FileListOperation::DeleteToTrash(vec![path_clone.clone()]));
                            Update::DRAW
                        })
                );

                // View Options
                template = template.add_item(MenuItem::separator());
                
                let mut sort_menu = MenuTemplate::new("sort-menu");
                let op_tx_sort = op_tx.clone();
                sort_menu = sort_menu.add_item(
                    MenuItem::new(MenuCommand::Custom(10), "Name (Asc)")
                        .with_action(move || {
                            let _ = op_tx_sort.send(FileListOperation::Sort(0, SortOrder::Ascending));
                            Update::DRAW
                        })
                );
                
                let op_tx_sort = op_tx.clone();
                sort_menu = sort_menu.add_item(
                    MenuItem::new(MenuCommand::Custom(11), "Name (Desc)")
                        .with_action(move || {
                            let _ = op_tx_sort.send(FileListOperation::Sort(0, SortOrder::Descending));
                            Update::DRAW
                        })
                );

                let op_tx_sort = op_tx.clone();
                sort_menu = sort_menu.add_item(
                    MenuItem::new(MenuCommand::Custom(12), "Size (Asc)")
                        .with_action(move || {
                            let _ = op_tx_sort.send(FileListOperation::Sort(1, SortOrder::Ascending));
                            Update::DRAW
                        })
                );
                
                let op_tx_sort = op_tx.clone();
                sort_menu = sort_menu.add_item(
                    MenuItem::new(MenuCommand::Custom(13), "Size (Desc)")
                        .with_action(move || {
                            let _ = op_tx_sort.send(FileListOperation::Sort(1, SortOrder::Descending));
                            Update::DRAW
                        })
                );

                let op_tx_sort = op_tx.clone();
                sort_menu = sort_menu.add_item(
                    MenuItem::new(MenuCommand::Custom(14), "Date Modified (Asc)")
                        .with_action(move || {
                            let _ = op_tx_sort.send(FileListOperation::Sort(3, SortOrder::Ascending));
                            Update::DRAW
                        })
                );

                let op_tx_sort = op_tx.clone();
                sort_menu = sort_menu.add_item(
                    MenuItem::new(MenuCommand::Custom(15), "Date Modified (Desc)")
                        .with_action(move || {
                            let _ = op_tx_sort.send(FileListOperation::Sort(3, SortOrder::Descending));
                            Update::DRAW
                        })
                );

                let op_tx_sort = op_tx.clone();
                sort_menu = sort_menu.add_item(
                    MenuItem::new(MenuCommand::Custom(16), "Type (Asc)")
                        .with_action(move || {
                            let _ = op_tx_sort.send(FileListOperation::Sort(2, SortOrder::Ascending));
                            Update::DRAW
                        })
                );

                let op_tx_sort = op_tx.clone();
                sort_menu = sort_menu.add_item(
                    MenuItem::new(MenuCommand::Custom(17), "Type (Desc)")
                        .with_action(move || {
                            let _ = op_tx_sort.send(FileListOperation::Sort(2, SortOrder::Descending));
                            Update::DRAW
                        })
                );
                
                template = template.add_item(
                    MenuItem::new(MenuCommand::Custom(5), "Sort By").with_submenu(sort_menu)
                );

                // Show the menu at cursor position
                context.menu_manager.show(template, Point::new(pos.x, pos.y));
                
                Update::DRAW
            }
        });
                Self {
            file_list,
            navigation,
            navigation_rx: Some(navigation_rx),
            navigation_path_signal,
            file_list_path_signal,
            signals_hooked: false,
            file_list_operation_rx: Some(file_list_op_rx),
            operation_tx,
            operation_rx: Some(operation_rx),
            status_tx: Some(status_tx),
            pending_delete_confirmation: Arc::new(Mutex::new(None)),
            pending_rename: Arc::new(Mutex::new(None)),
            pending_create_dir: Arc::new(Mutex::new(None)),
            pending_create_file: Arc::new(Mutex::new(None)),
            clipboard,
            folder_duplicate_done_tx,
            folder_duplicate_done_rx: Some(folder_duplicate_done_rx),
            delete_policy,
            terminal_command,
        }
    }

    fn apply_startup_folder_view(&mut self, fileman: &crate::config::FilemanConfig) {
        if let Some((col, order)) = fileman.initial_sort() {
            self.file_list.sort(col, order);
        }
        if let Some(sz) = fileman.initial_icon_size() {
            self.file_list.set_icon_size(sz);
        }
    }

    /// Paste files from clipboard to current directory
    fn paste_files(&mut self) -> Update {
        let current_path = (*self.file_list_path_signal.get()).clone();
        
        let clipboard_content = if let Ok(mut clipboard) = self.clipboard.lock() {
            match clipboard.get_files() {
                Ok(Some(content)) => Some(content),
                Ok(None) => {
                    if let Some(tx) = &self.status_tx {
                        let _ = tx.send("Clipboard is empty".to_string());
                    }
                    None
                },
                Err(e) => {
                    log::error!("Failed to get clipboard content: {}", e);
                    if let Some(tx) = &self.status_tx {
                        let _ = tx.send(format!("Clipboard error: {}", e));
                    }
                    None
                }
            }
        } else {
            None
        };

        if let Some((custom_paths, is_cut)) = clipboard_content {
             // Perform Paste Operation
             // For now, we'll implement a simple copy/move logic here or delegate to a background task
             // Since file operations can be slow, ideally we should spawn a task.
             // But FileListWrapper update is sync.
             // We can use tokio::spawn if we are in async context, but Widget::update is async, so we can spawn.
             // Wait, Widget::update is async in our codebase? Yes.
             
             let status_tx = self.status_tx.clone();
             
             tokio::spawn(async move {
                 let action = if is_cut { "Moving" } else { "Copying" };
                 if let Some(tx) = &status_tx {
                     let _ = tx.send(format!("{} {} files...", action, custom_paths.len()));
                 }
                 
                 for from_path in custom_paths {
                     if let Some(file_name) = from_path.file_name() {
                         let mut to_path = current_path.join(file_name);
                         
                         // Simple collision avoidance: append _copy if exists
                         if to_path.exists() {
                             // Basic logic: stem + _copy + ext
                             // This is a naive implementation
                             let file_stem = to_path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();
                             let extension = to_path.extension().map(|s| s.to_string_lossy()).unwrap_or_default();
                             
                             let new_name = if extension.is_empty() {
                                 format!("{}_copy", file_stem)
                             } else {
                                 format!("{}_copy.{}", file_stem, extension)
                             };
                             to_path = current_path.join(new_name);
                         }
                         
                         let result = if is_cut {
                             match tokio::fs::rename(&from_path, &to_path).await {
                                 Ok(_) => Ok(()),
                                 Err(e) => {
                                     // Fallback to copy + delete for cross-device moves or other rename failures
                                     // 18 = EXDEV (Cross-device link)
                                     if e.raw_os_error() == Some(18) {
                                         log::info!("Cross-device move detected, falling back to copy+delete");
                                         if from_path.is_dir() {
                                             match operations::copy_recursive(from_path.clone(), to_path.clone()).await {
                                                 Ok(_) => operations::delete_path(from_path.clone()).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)),
                                                 Err(copy_err) => Err(copy_err),
                                             }
                                         } else {
                                             match tokio::fs::copy(&from_path, &to_path).await {
                                                 Ok(_) => operations::delete_path(from_path.clone()).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)),
                                                 Err(copy_err) => Err(copy_err),
                                             }
                                         }
                                     } else {
                                         Err(e)
                                     }
                                 }
                             }
                         } else {
                             // tokio::fs::copy only copies contents, not recursively for dirs.
                             // For text entries (files), copy works. For dirs, we need recursive copy.
                             // For MVP validation, we'll support files only or use a simple recursive copy helper later.
                             // Let's assume files for now or use `fs_extra` if available.
                             // Implementing simple file copy:
                              if from_path.is_dir() {
                                  operations::copy_recursive(from_path.clone(), to_path.clone()).await
                              } else {
                                 tokio::fs::copy(&from_path, &to_path).await.map(|_| ())
                             }
                         };
                         
                         match result {
                             Ok(_) => {
                                 log::info!("{} {:?} to {:?}", action, from_path, to_path);
                             }
                             Err(e) => {
                                 log::error!("Failed to {} {:?}: {}", action.to_lowercase(), from_path, e);
                             }
                         }
                     }
                 }
                 
                 if let Some(tx) = &status_tx {
                     let _ = tx.send(format!("{} complete", action));
                 }
             });
             
             // We spawned a task, but we might want to refresh the file list eventually.
             // The file list watches basic changes via notify (if implemented) or we can manually refresh using FileListOperation::Refresh
             // We don't have direct access to send Refresh command here easily without self.file_list_operation_rx (which is receiver).
             // Actually we have `file_list` struct, we can call methods on it if exposed.
             // For now, let's rely on manual refresh or file system watcher if present.
             // Wait, `FileList` has `refresh()` method but it's internal logic mostly.
             // We can trigger a layout update which might not re-read files unless we force it.
        }
        
        Update::empty()
    }

    /// Get the selected paths signal (for reactive subscription by other widgets)
    pub fn selected_paths_signal(&self) -> &StateSignal<Vec<PathBuf>> {
        self.file_list.selected_paths_signal()
    }
    
    /// Get the view mode signal
    pub fn view_mode_signal(&self) -> &StateSignal<nptk_fileman_widgets::file_list::FileListViewMode> {
        self.file_list.view_mode_signal()
    }

    /// Show properties popup for the given paths
    pub fn show_properties_for_paths(&mut self, paths: &[PathBuf], context: nptk::core::app::context::AppContext) {
        self.file_list.show_properties_popup(paths, context);
    }

    /// Show delete confirmation dialog (`permanent`: true = skip trash).
    fn show_delete_confirmation_dialog(
        &self,
        paths: &[PathBuf],
        permanent: bool,
        context: AppContext,
    ) {
        if paths.is_empty() {
            return;
        }

        if permanent && !self.delete_policy.confirm_delete {
            if let Ok(mut p) = self.pending_delete_confirmation.lock() {
                *p = Some((paths.to_vec(), true));
            }
            return;
        }
        if !permanent && !self.delete_policy.confirm_trash {
            if let Ok(mut p) = self.pending_delete_confirmation.lock() {
                *p = Some((paths.to_vec(), false));
            }
            return;
        }

        let message = if permanent {
            if paths.len() == 1 {
                let path = &paths[0];
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("<unnamed>");
                format!(
                    "Permanently delete \"{}\"? This cannot be undone.",
                    name
                )
            } else {
                format!(
                    "Permanently delete {} selected item(s)? This cannot be undone.",
                    paths.len()
                )
            }
        } else if paths.len() == 1 {
            let path = &paths[0];
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("<unnamed>");
            format!("Move \"{}\" to the trash?", name)
        } else {
            format!(
                "Move {} selected item(s) to the trash?",
                paths.len()
            )
        };

        let pending_delete = self.pending_delete_confirmation.clone();
        let paths_to_delete = paths.to_vec();
        let confirm_label = if permanent {
            "Delete permanently"
        } else {
            "Move to Trash"
        };
        let title = if permanent {
            "Confirm permanent delete"
        } else {
            "Confirm trash"
        };

        let message_text = Text::new(message);

        let cancel_btn = Button::new(Text::new("Cancel".to_string()))
            .with_on_pressed(MaybeSignal::value(Update::DRAW));

        let delete_btn = Button::new(Text::new(confirm_label.to_string()))
            .with_on_pressed({
                let pending_delete_btn = pending_delete.clone();
                let paths_btn = paths_to_delete.clone();
                MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                    if let Ok(mut pending) = pending_delete_btn.lock() {
                        *pending = Some((paths_btn.clone(), permanent));
                    }
                    Update::DRAW
                })))
            });

        let dialog_content = Container::new(vec![
            Box::new(message_text),
            Box::new(Container::new(vec![
                Box::new(cancel_btn),
                Box::new(delete_btn),
            ]).with_layout_style(LayoutStyle {
                flex_direction: FlexDirection::Row,
                gap: Vector2::new(LengthPercentage::length(8.0), LengthPercentage::length(0.0)),
                justify_content: Some(JustifyContent::FlexEnd),
                size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
                ..Default::default()
            })),
        ])
        .with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
            flex_direction: FlexDirection::Column,
            padding: Rect {
                left: LengthPercentage::length(16.0),
                right: LengthPercentage::length(16.0),
                top: LengthPercentage::length(16.0),
                bottom: LengthPercentage::length(16.0),
            },
            gap: Vector2::new(LengthPercentage::length(0.0), LengthPercentage::length(16.0)),
            ..Default::default()
        });

        context
            .popup_manager
            .create_popup_at(Box::new(dialog_content), title, (400, 150), (300, 200));
    }

    /// Show rename dialog
    fn show_rename_dialog(&self, path: PathBuf, context: AppContext) {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let input_signal = StateSignal::new(name.clone());
        let pending_rename = self.pending_rename.clone();
        let path_clone = path.clone();

        let message_text = Text::new(format!("Rename \"{}\" to:", name).to_string());
        
        let input_field = TextInput::new()
            .with_text_signal(input_signal.clone())
            .with_placeholder("Enter new name".to_string());
            
        let input_signal_clone = input_signal.clone();
        
        // Cancel button
        let cancel_btn = Button::new(Text::new("Cancel".to_string()))
            .with_on_pressed(MaybeSignal::value(Update::DRAW));

        // OK button
        let ok_btn = Button::new(Text::new("Rename".to_string()))
            .with_on_pressed({
                let pending = pending_rename.clone();
                let p = path_clone.clone();
                let s = input_signal_clone.clone();
                MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                    let new_name = (*s.get()).clone();
                    if !new_name.is_empty() {
                         if let Ok(mut lock) = pending.lock() {
                             *lock = Some((p.clone(), new_name));
                         }
                    }
                    Update::DRAW
                })))
            });

        let dialog_content = Container::new(vec![
            Box::new(message_text),
            Box::new(input_field),
            Box::new(Container::new(vec![
                Box::new(cancel_btn),
                Box::new(ok_btn),
            ]).with_layout_style(LayoutStyle {
                flex_direction: FlexDirection::Row,
                gap: Vector2::new(LengthPercentage::length(8.0), LengthPercentage::length(0.0)),
                justify_content: Some(JustifyContent::FlexEnd),
                size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
                ..Default::default()
            })),
        ]).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
            flex_direction: FlexDirection::Column,
            padding: Rect {
                left: LengthPercentage::length(16.0),
                right: LengthPercentage::length(16.0),
                top: LengthPercentage::length(16.0),
                bottom: LengthPercentage::length(16.0),
            },
            gap: Vector2::new(LengthPercentage::length(0.0), LengthPercentage::length(16.0)),
            ..Default::default()
        });

        context.popup_manager.create_popup_at(Box::new(dialog_content), "Rename File", (400, 200), (300, 250));
    }

    /// Show new folder dialog
    fn show_new_folder_dialog(&self, parent: PathBuf, context: AppContext) {
        let input_signal = StateSignal::new("New Folder".to_string());
        let pending_create = self.pending_create_dir.clone();
        let parent_clone = parent.clone();

        let message_text = Text::new("Create new folder named:".to_string());
        
        let input_field = TextInput::new()
            .with_text_signal(input_signal.clone())
            .with_placeholder("Folder name".to_string());
            
        let input_signal_clone = input_signal.clone();
        
        // Cancel button
        let cancel_btn = Button::new(Text::new("Cancel".to_string()))
            .with_on_pressed(MaybeSignal::value(Update::DRAW));

        // OK button
        let ok_btn = Button::new(Text::new("Create".to_string()))
            .with_on_pressed({
                let pending = pending_create.clone();
                let p = parent_clone.clone();
                let s = input_signal_clone.clone();
                MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                    let new_name = (*s.get()).clone();
                    if !new_name.is_empty() {
                         if let Ok(mut lock) = pending.lock() {
                             *lock = Some((p.clone(), new_name));
                         }
                    }
                    Update::DRAW
                })))
            });

        let dialog_content = Container::new(vec![
            Box::new(message_text),
            Box::new(input_field),
            Box::new(Container::new(vec![
                Box::new(cancel_btn),
                Box::new(ok_btn),
            ]).with_layout_style(LayoutStyle {
                flex_direction: FlexDirection::Row,
                gap: Vector2::new(LengthPercentage::length(8.0), LengthPercentage::length(0.0)),
                justify_content: Some(JustifyContent::FlexEnd),
                size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
                ..Default::default()
            })),
        ]).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
            flex_direction: FlexDirection::Column,
            padding: Rect {
                left: LengthPercentage::length(16.0),
                right: LengthPercentage::length(16.0),
                top: LengthPercentage::length(16.0),
                bottom: LengthPercentage::length(16.0),
            },
            gap: Vector2::new(LengthPercentage::length(0.0), LengthPercentage::length(16.0)),
            ..Default::default()
        });

        context.popup_manager.create_popup_at(Box::new(dialog_content), "New Folder", (400, 200), (300, 250));
    }

    /// Show new empty file dialog
    fn show_new_file_dialog(&self, parent: PathBuf, context: AppContext) {
        let input_signal = StateSignal::new("New File".to_string());
        let pending_create = self.pending_create_file.clone();
        let parent_clone = parent.clone();

        let message_text = Text::new("Create new empty file named:".to_string());

        let input_field = TextInput::new()
            .with_text_signal(input_signal.clone())
            .with_placeholder("File name".to_string());

        let input_signal_clone = input_signal.clone();

        let cancel_btn = Button::new(Text::new("Cancel".to_string()))
            .with_on_pressed(MaybeSignal::value(Update::DRAW));

        let ok_btn = Button::new(Text::new("Create".to_string()))
            .with_on_pressed({
                let pending = pending_create.clone();
                let p = parent_clone.clone();
                let s = input_signal_clone.clone();
                MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                    let new_name = (*s.get()).clone();
                    if !new_name.is_empty() {
                        if let Ok(mut lock) = pending.lock() {
                            *lock = Some((p.clone(), new_name));
                        }
                    }
                    Update::DRAW
                })))
            });

        let dialog_content = Container::new(vec![
            Box::new(message_text),
            Box::new(input_field),
            Box::new(Container::new(vec![
                Box::new(cancel_btn),
                Box::new(ok_btn),
            ]).with_layout_style(LayoutStyle {
                flex_direction: FlexDirection::Row,
                gap: Vector2::new(LengthPercentage::length(8.0), LengthPercentage::length(0.0)),
                justify_content: Some(JustifyContent::FlexEnd),
                size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
                ..Default::default()
            })),
        ]).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
            flex_direction: FlexDirection::Column,
            padding: Rect {
                left: LengthPercentage::length(16.0),
                right: LengthPercentage::length(16.0),
                top: LengthPercentage::length(16.0),
                bottom: LengthPercentage::length(16.0),
            },
            gap: Vector2::new(LengthPercentage::length(0.0), LengthPercentage::length(16.0)),
            ..Default::default()
        });

        context.popup_manager.create_popup_at(Box::new(dialog_content), "New File", (400, 200), (300, 250));
    }
}

#[async_trait(?Send)]
impl Widget for FileListWrapper {

    fn layout_style(&self, _context: &nptk::core::layout::LayoutContext) -> nptk::core::layout::StyleNode {
        self.file_list.layout_style(_context)
    }

    async fn update(
        &mut self,
        layout: &nptk::core::layout::LayoutNode,
        context: nptk::core::app::context::AppContext,
        info: &mut nptk::core::app::info::AppInfo,
    ) -> nptk::core::app::update::Update {
        let mut update = Update::empty();

        // Hook signals on first update for reactive subscription
        if !self.signals_hooked {
            context.hook_signal(&mut self.navigation_path_signal);
            context.hook_signal(&mut self.file_list_path_signal);
            self.signals_hooked = true;
        }

        // Handle sidebar navigation events (sync to NavigationState, which will reactively update FileList)
        if let Some(ref mut rx) = self.navigation_rx {
            while let Ok(path) = rx.try_recv() {
                if let Ok(mut nav) = self.navigation.lock() {
                    nav.navigate_to(path.clone());
                    update.insert(Update::LAYOUT | Update::DRAW);
                }
            }
        }

        // Reactively sync NavigationState path changes to FileList
        let nav_path = (*self.navigation_path_signal.get()).clone();
        let file_list_path = (*self.file_list_path_signal.get()).clone();
        if nav_path != file_list_path {
            self.file_list.set_path(nav_path.clone());
            update.insert(Update::LAYOUT | Update::DRAW);
        }

        // Update the wrapped FileList to let it handle internal updates
        let file_list_update = self.file_list.update(layout, context.clone(), info).await;
        update |= file_list_update;

        // Path refresh/recovery logic: If current directory no longer exists, navigate to parent
        // This handles the case where a directory is deleted externally
        let current_path = (*self.file_list_path_signal.get()).clone();
        if !current_path.exists() {
            // Navigate to parent directory, continuing up until we find a valid directory
            let mut recovery_path = current_path.clone();
            while !recovery_path.exists() && recovery_path != PathBuf::from("/") {
                if let Some(parent) = recovery_path.parent() {
                    recovery_path = parent.to_path_buf();
                } else {
                    break;
                }
            }
            // If we found a valid parent, navigate there
            if recovery_path.exists() && recovery_path != current_path {
                if let Ok(mut nav) = self.navigation.lock() {
                    nav.navigate_to(recovery_path.clone());
                    self.file_list.set_path(recovery_path);
                    update.insert(Update::LAYOUT | Update::DRAW);
                }
            }
        }

        if let Some(ref mut rx) = self.folder_duplicate_done_rx {
            let mut need_refresh = false;
            while rx.try_recv().is_ok() {
                need_refresh = true;
            }
            if need_refresh {
                let current_path = self.file_list.get_current_path();
                self.file_list.set_path(current_path);
                update.insert(Update::LAYOUT | Update::DRAW);
            }
        }

        // Reactively sync FileList path changes to NavigationState (e.g., from double-click navigation)
        let file_list_path_after = (*self.file_list_path_signal.get()).clone();
        if file_list_path_after != nav_path {
            if let Ok(mut nav) = self.navigation.lock() {
                nav.navigate_to(file_list_path_after.clone());
                update.insert(Update::LAYOUT | Update::DRAW);
            }
        }

        // Process file operations from FileList widget (context menu, etc.)
        // Collect operations to avoid borrow conflicts
        let mut operations = Vec::new();
        
        if let Some(ref mut rx) = self.file_list_operation_rx {
            while let Ok(op) = rx.try_recv() {
                operations.push(op);
            }
        }
        
        for op in operations {
            match op {
                FileListOperation::Properties(paths) => {
                     self.show_properties_for_paths(&paths, context.clone());
                }
                FileListOperation::PromptRename(path) => {
                    self.show_rename_dialog(path, context.clone());
                },
                FileListOperation::Copy(paths) => {
                     if let Ok(mut clipboard) = self.clipboard.lock() {
                        if let Err(e) = clipboard.set_files(&paths, false) {
                            log::error!("Failed to copy files: {}", e);
                            if let Some(tx) = &self.status_tx {
                                let _ = tx.send(format!("Failed to copy: {}", e));
                            }
                        } else {
                            if let Some(tx) = &self.status_tx {
                                let _ = tx.send(format!("Copied {} files", paths.len()));
                            }
                        }
                    }
                },
                FileListOperation::Cut(paths) => {
                     if let Ok(mut clipboard) = self.clipboard.lock() {
                        if let Err(e) = clipboard.set_files(&paths, true) {
                            log::error!("Failed to cut files: {}", e);
                             if let Some(tx) = &self.status_tx {
                                let _ = tx.send(format!("Failed to cut: {}", e));
                            }
                        } else {
                            if let Some(tx) = &self.status_tx {
                                let _ = tx.send(format!("Cut {} files", paths.len()));
                            }
                        }
                    }
                },
                FileListOperation::Paste => {
                    update.insert(self.paste_files());
                },
                FileListOperation::DeleteToTrash(paths) => {
                    if self.delete_policy.use_trash {
                        let _ = self.operation_tx.send(FileOperationRequest::Delete(paths));
                    } else {
                        let _ = self
                            .operation_tx
                            .send(FileOperationRequest::DeletePermanent(paths));
                    }
                },
                FileListOperation::DeletePermanent(paths) => {
                    let _ = self
                        .operation_tx
                        .send(FileOperationRequest::DeletePermanent(paths));
                },
                FileListOperation::DeselectAll => {
                    self.file_list.clear_selection();
                    update.insert(Update::DRAW);
                }
                FileListOperation::InvertSelection => {
                    self.file_list.invert_selection();
                    update.insert(Update::DRAW);
                }
                FileListOperation::Sort(col, order) => {
                    self.file_list.sort(col, order);
                    update.insert(Update::DRAW);
                },
                FileListOperation::Refresh => {
                    let path = self.file_list.get_current_path();
                    self.file_list.set_path(path); // Re-setting path triggers refresh
                    update.insert(Update::DRAW);
                }
                FileListOperation::Open => {
                    self.file_list.open_selected_paths(|dir| {
                        if let Ok(mut nav) = self.navigation.lock() {
                            nav.navigate_to(dir);
                        }
                    });
                    update.insert(Update::DRAW);
                }
                FileListOperation::OpenPaths(paths) => {
                    self.file_list.open_paths(&paths, |dir| {
                        if let Ok(mut nav) = self.navigation.lock() {
                            nav.navigate_to(dir);
                        }
                    });
                    update.insert(Update::DRAW);
                }
                FileListOperation::Duplicate(paths) => {
                    let _ = self
                        .operation_tx
                        .send(FileOperationRequest::Duplicate(paths));
                }
                FileListOperation::NavigateUp => {
                    if let Ok(mut nav) = self.navigation.lock() {
                        if let Some(parent) = nav.parent_path() {
                            nav.navigate_to(parent);
                            update.insert(Update::LAYOUT | Update::DRAW);
                        }
                    }
                }
                FileListOperation::PromptNewFolder => {
                    let parent = self.file_list.get_current_path();
                    let _ = self
                        .operation_tx
                        .send(FileOperationRequest::PromptCreateDirectory(parent));
                }
                FileListOperation::PromptNewFile => {
                    let parent = self.file_list.get_current_path();
                    let _ = self
                        .operation_tx
                        .send(FileOperationRequest::PromptCreateFile(parent));
                }
            }
        }

        // Process file operations from toolbar/other UI
        // Note: Delete operations need confirm, Properties need dialog
        // Collect operations first to avoid borrow conflicts
        let mut pending_deletes: Vec<(Vec<PathBuf>, bool)> = Vec::new();
        let mut pending_properties = Vec::new();
        let mut pending_renames = Vec::new();
        let mut pending_creates = Vec::new();
        let mut pending_create_files = Vec::new();
        let mut deferred_operation_channel_ops: Vec<FileOperationRequest> = Vec::new();

        if let Some(ref mut rx) = self.operation_rx {
            while let Ok(op) = rx.try_recv() {
                match op {
                    FileOperationRequest::Delete(paths) => {
                        log::warn!("RECEIVED DELETE REQUEST for {} path(s)", paths.len());
                        let permanent = !self.delete_policy.use_trash;
                        pending_deletes.push((paths, permanent));
                    }
                    FileOperationRequest::DeletePermanent(paths) => {
                        pending_deletes.push((paths, true));
                    }
                    /*
                    FileOperationRequest::CreateDirectory { parent, name } => {
                        let new_dir = parent.join(&name);
                        match operations::create_directory(new_dir.clone()) {
                            Ok(_) => {
                                log::info!("Created directory: {:?}", new_dir);
                                if let Some(ref tx) = self.status_tx {
                                    let _ = tx.send(format!("Created directory '{}'", name));
                                }
                                // Refresh file list
                                let current_path = self.file_list.get_current_path();
                                self.file_list.set_path(current_path.clone());
                                update.insert(Update::LAYOUT | Update::DRAW);
                            }
                            Err(e) => {
                                log::error!("Failed to create directory {:?}: {}", new_dir, e);
                                if let Some(ref tx) = self.status_tx {
                                    let _ = tx.send(format!("Error: {}", e));
                                }
                            }
                        }
                    }
                    FileOperationRequest::Rename { from, to } => {
                         match operations::rename_path(from.clone(), to.clone()) {
                            Ok(_) => {
                                log::info!("Renamed: {:?} -> {:?}", from, to);
                                if let Some(ref tx) = self.status_tx {
                                    let _ = tx.send("Renamed successfully".to_string());
                                }
                                // Refresh file list
                                let current_path = self.file_list.get_current_path();
                                self.file_list.set_path(current_path.clone());
                                update.insert(Update::LAYOUT | Update::DRAW);
                            }
                            Err(e) => {
                                log::error!("Failed to rename {:?} to {:?}: {}", from, to, e);
                                if let Some(ref tx) = self.status_tx {
                                    let _ = tx.send(format!("Error: {}", e));
                                }
                            }
                        }
                    }
                    */
                    FileOperationRequest::Properties(paths) => {
                        // Collect properties requests
                        pending_properties.push(paths);
                    }
                    FileOperationRequest::PromptRename(path) => {
                        pending_renames.push(path);
                    }
                    FileOperationRequest::PromptCreateDirectory(parent) => {
                        pending_creates.push(parent);
                    }
                    FileOperationRequest::PromptCreateFile(parent) => {
                        pending_create_files.push(parent);
                    }
                    FileOperationRequest::Copy(paths) => {
                        deferred_operation_channel_ops.push(FileOperationRequest::Copy(paths));
                    }
                    FileOperationRequest::Cut(paths) => {
                        deferred_operation_channel_ops.push(FileOperationRequest::Cut(paths));
                    }
                    FileOperationRequest::Paste => {
                        deferred_operation_channel_ops.push(FileOperationRequest::Paste);
                    }
                    FileOperationRequest::Refresh => {
                        deferred_operation_channel_ops.push(FileOperationRequest::Refresh);
                    }
                    FileOperationRequest::Sort(col, order) => {
                        deferred_operation_channel_ops
                            .push(FileOperationRequest::Sort(col, order));
                    }
                    FileOperationRequest::SelectAll => {
                        self.file_list.select_all();
                        update.insert(Update::DRAW);
                    }
                    FileOperationRequest::DeselectAll => {
                        self.file_list.clear_selection();
                        update.insert(Update::DRAW);
                    }
                    FileOperationRequest::InvertSelection => {
                        self.file_list.invert_selection();
                        update.insert(Update::DRAW);
                    }
                    FileOperationRequest::OpenSelection => {
                        self.file_list.open_selected_paths(|dir| {
                            if let Ok(mut nav) = self.navigation.lock() {
                                nav.navigate_to(dir);
                            }
                        });
                        update.insert(Update::DRAW);
                    }
                    FileOperationRequest::OpenTerminalHere => {
                        let cwd = self.file_list.get_current_path();
                        match crate::terminal::open_terminal_in_directory(
                            &cwd,
                            self.terminal_command.as_deref(),
                        ) {
                            Ok(()) => {
                                if let Some(ref tx) = self.status_tx {
                                    let _ = tx.send("Opened terminal in current folder".to_string());
                                }
                            }
                            Err(e) => {
                                if let Some(ref tx) = self.status_tx {
                                    let _ = tx.send(format!("Terminal: {}", e));
                                }
                            }
                        }
                    }
                    FileOperationRequest::Duplicate(paths) => {
                        let mut need_sync_refresh = false;
                        for path in paths {
                            let path_log = path.clone();
                            if path.is_dir() {
                                match operations::duplicate_destination_in_parent(&path) {
                                    Ok(dest) => {
                                        let from = path.clone();
                                        let status_tx = self.status_tx.clone();
                                        let done_tx = self.folder_duplicate_done_tx.clone();
                                        tokio::spawn(async move {
                                            match operations::duplicate_directory_tree(from, dest.clone())
                                                .await
                                            {
                                                Ok(()) => {
                                                    log::info!(
                                                        "Duplicated folder {:?} -> {:?}",
                                                        path_log,
                                                        dest
                                                    );
                                                    if let Some(ref tx) = status_tx {
                                                        let _ = tx.send(format!(
                                                            "Duplicated folder to {:?}",
                                                            dest
                                                        ));
                                                    }
                                                    let _ = done_tx.send(());
                                                }
                                                Err(e) => {
                                                    log::error!(
                                                        "Folder duplicate {:?}: {}",
                                                        path_log,
                                                        e
                                                    );
                                                    if let Some(ref tx) = status_tx {
                                                        let _ = tx.send(format!("Duplicate: {}", e));
                                                    }
                                                }
                                            }
                                        });
                                    }
                                    Err(e) => {
                                        log::error!("Duplicate {:?}: {}", path_log, e);
                                        if let Some(ref tx) = self.status_tx {
                                            let _ = tx.send(format!("Duplicate: {}", e));
                                        }
                                    }
                                }
                            } else {
                                match operations::duplicate_in_parent(path) {
                                    Ok(dest) => {
                                        log::info!("Duplicated {:?} -> {:?}", path_log, dest);
                                        if let Some(ref tx) = self.status_tx {
                                            let _ = tx.send(format!("Duplicated to {:?}", dest));
                                        }
                                        need_sync_refresh = true;
                                    }
                                    Err(e) => {
                                        log::error!("Duplicate {:?}: {}", path_log, e);
                                        if let Some(ref tx) = self.status_tx {
                                            let _ = tx.send(format!("Duplicate: {}", e));
                                        }
                                    }
                                }
                            }
                        }
                        if need_sync_refresh {
                            let current_path = self.file_list.get_current_path();
                            self.file_list.set_path(current_path.clone());
                            update.insert(Update::LAYOUT | Update::DRAW);
                        }
                    }
                }
            }
        }

        for op in deferred_operation_channel_ops {
            match op {
                FileOperationRequest::Copy(paths) => {
                    if let Ok(mut clipboard) = self.clipboard.lock() {
                        if let Err(e) = clipboard.set_files(&paths, false) {
                            log::error!("Failed to copy files: {}", e);
                            if let Some(tx) = &self.status_tx {
                                let _ = tx.send(format!("Failed to copy: {}", e));
                            }
                        } else if let Some(tx) = &self.status_tx {
                            let _ = tx.send(format!("Copied {} path(s)", paths.len()));
                        }
                    }
                    update.insert(Update::DRAW);
                }
                FileOperationRequest::Cut(paths) => {
                    if let Ok(mut clipboard) = self.clipboard.lock() {
                        if let Err(e) = clipboard.set_files(&paths, true) {
                            log::error!("Failed to cut files: {}", e);
                            if let Some(tx) = &self.status_tx {
                                let _ = tx.send(format!("Failed to cut: {}", e));
                            }
                        } else if let Some(tx) = &self.status_tx {
                            let _ = tx.send(format!("Cut {} path(s)", paths.len()));
                        }
                    }
                    update.insert(Update::DRAW);
                }
                FileOperationRequest::Paste => {
                    update.insert(self.paste_files());
                }
                FileOperationRequest::Refresh => {
                    let path = self.file_list.get_current_path();
                    self.file_list.set_path(path);
                    update.insert(Update::LAYOUT | Update::DRAW);
                }
                FileOperationRequest::Sort(col, order) => {
                    self.file_list.sort(col, order);
                    update.insert(Update::DRAW);
                }
                _ => {}
            }
        }
        
        // Process pending properties requests (after releasing borrow)
        for paths in pending_properties {
             self.show_properties_for_paths(&paths, context.clone());
             update.insert(Update::DRAW);
        }

        // Process pending renames
        for path in pending_renames {
             self.show_rename_dialog(path, context.clone());
             update.insert(Update::DRAW);
        }

        // Process pending creates
        for parent in pending_creates {
             self.show_new_folder_dialog(parent, context.clone());
             update.insert(Update::DRAW);
        }

        for parent in pending_create_files {
            self.show_new_file_dialog(parent, context.clone());
            update.insert(Update::DRAW);
        }
        
        // Show confirmation dialogs for pending delete operations (after releasing borrow)
        if !pending_deletes.is_empty() {
            log::warn!("SHOWING {} DELETE CONFIRMATION DIALOG(S)", pending_deletes.len());
        }
        for (paths, permanent) in pending_deletes {
            self.show_delete_confirmation_dialog(&paths, permanent, context.clone());
            update.insert(Update::DRAW);
        }
        
        // Process confirmed delete operations from toolbar (user clicked confirm in dialog)
        if let Ok(mut pending_delete) = self.pending_delete_confirmation.lock() {
            if let Some((paths, permanent)) = pending_delete.take() {
                let paths_clone = paths.clone();
                let mut all_success = true;
                let mut error_msg = String::new();

                for path in &paths {
                    let result = if permanent {
                        operations::delete_path(path.clone())
                    } else {
                        operations::move_to_trash(path.clone())
                    };
                    match result {
                        Ok(()) => {
                            log::info!("Removed: {:?} (permanent={})", path, permanent);
                        }
                        Err(e) => {
                            log::error!("Failed to remove {:?}: {}", path, e);
                            all_success = false;
                            error_msg = e;
                            break;
                        }
                    }
                }

                if let Some(ref tx) = self.status_tx {
                    if all_success {
                        let msg = if permanent {
                            format!("Permanently deleted {} item(s)", paths_clone.len())
                        } else {
                            format!("Moved {} item(s) to trash", paths_clone.len())
                        };
                        let _ = tx.send(msg);
                    } else {
                        let _ = tx.send(format!("Error: {}", error_msg));
                    }
                }

                let current_path = self.file_list.get_current_path();
                self.file_list.set_path(current_path.clone());
                update.insert(Update::LAYOUT | Update::DRAW);
            }
        }

        // Process confirmed rename operations (from dialog)
        if let Ok(mut pending) = self.pending_rename.lock() {
            if let Some((path, new_name)) = pending.take() {
                // Construct new path
                if let Some(parent) = path.parent() {
                    let new_path = parent.join(new_name);
                    match operations::rename_path(path.clone(), new_path.clone()) {
                        Ok(_) => {
                            log::info!("Renamed: {:?} -> {:?}", path, new_path);
                            if let Some(ref tx) = self.status_tx {
                                let _ = tx.send("Renamed successfully".to_string());
                            }
                            // Refresh file list
                            let current_path = self.file_list.get_current_path();
                            self.file_list.set_path(current_path.clone());
                            update.insert(Update::LAYOUT | Update::DRAW);
                        }
                        Err(e) => {
                            log::error!("Failed to rename {:?} to {:?}: {}", path, new_path, e);
                            if let Some(ref tx) = self.status_tx {
                                let _ = tx.send(format!("Error: {}", e));
                            }
                        }
                    }
                }
            }
        }

        // Process confirmed create directory operations (from dialog)
        if let Ok(mut pending) = self.pending_create_dir.lock() {
            if let Some((parent, name)) = pending.take() {
                let new_dir = parent.join(name);
                match operations::create_directory(new_dir.clone()) {
                    Ok(_) => {
                        log::info!("Created directory: {:?}", new_dir);
                        if let Some(ref tx) = self.status_tx {
                            let _ = tx.send("Directory created".to_string());
                        }
                        // Refresh file list
                        let current_path = self.file_list.get_current_path();
                        self.file_list.set_path(current_path.clone());
                        update.insert(Update::LAYOUT | Update::DRAW);
                    }
                    Err(e) => {
                        log::error!("Failed to create directory {:?}: {}", new_dir, e);
                        if let Some(ref tx) = self.status_tx {
                            let _ = tx.send(format!("Error: {}", e));
                        }
                    }
                }
            }
        }

        if let Ok(mut pending) = self.pending_create_file.lock() {
            if let Some((parent, name)) = pending.take() {
                let new_file = parent.join(name);
                match operations::create_file(new_file.clone()) {
                    Ok(_) => {
                        log::info!("Created file: {:?}", new_file);
                        if let Some(ref tx) = self.status_tx {
                            let _ = tx.send("File created".to_string());
                        }
                        let current_path = self.file_list.get_current_path();
                        self.file_list.set_path(current_path.clone());
                        update.insert(Update::LAYOUT | Update::DRAW);
                    }
                    Err(e) => {
                        log::error!("Failed to create file {:?}: {}", new_file, e);
                        if let Some(ref tx) = self.status_tx {
                            let _ = tx.send(format!("Error: {}", e));
                        }
                    }
                }
            }
        }
        
        update
    }

    fn render(
        &mut self,
        graphics: &mut dyn nptk::core::vgi::Graphics,
        layout: &nptk::core::layout::LayoutNode,
        info: &mut nptk::core::app::info::AppInfo,
        context: nptk::core::app::context::AppContext,
    ) {
        self.file_list.render(graphics, layout, info, context)
    }
}

impl WidgetLayoutExt for FileListWrapper {
    fn set_layout_style(&mut self, layout_style: impl Into<nptk::core::signal::MaybeSignal<nptk::core::layout::LayoutStyle>>) {
        self.file_list.set_layout_style(layout_style)
    }
}



// LocationBarWrapper removed (replaced by FileLocationBar)



// StatusBarWrapper removed (replaced by FileStatusBar)

// Ensure FileListWrapper exposes file_list or search signal
impl FileListWrapper {
    #[allow(dead_code)]
    pub fn search_query_signal(&self) -> StateSignal<String> {
        self.file_list.search_query_signal().clone()
    }
    
    pub fn entries_signal(&self) -> StateSignal<Vec<FileEntry>> {
        self.file_list.entries_signal().clone()
    }
}

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
    let sidebar_nav_rx = sidebar.take_navigation_receiver()
        .expect("FilemanSidebar should provide navigation receiver");

    let search_scope_signal = StateSignal::new(SearchScope::CurrentFolder);
    let search_query_signal = StateSignal::new(String::new());
    let show_hidden_files_signal =
        StateSignal::new(state.fileman.initial_show_hidden());
    let (search_tx, search_rx) = mpsc::unbounded_channel::<String>();
    let (folder_duplicate_done_tx, folder_duplicate_done_rx) = mpsc::unbounded_channel::<()>();

    // Create FileList wrapper that syncs with navigation state
    let delete_policy = state.fileman.delete_policy();
    let terminal_command = state.fileman.terminal_command().map(str::to_string);

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
    ).with_message_receiver(status_rx);

    // Build main layout
    Container::new(vec![
        Box::new(menu_bar),
        // Toolbar area
        Box::new(Container::new(vec![
            Box::new(toolbar_wrapper),
            Box::new(location_bar),
        ]).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
            flex_direction: FlexDirection::Column,
            gap: Vector2::new(LengthPercentage::length(0.0), LengthPercentage::length(4.0)),
            ..Default::default()
        })),
        // Content area (sidebar + file list)
        Box::new(Container::new(vec![
            Box::new(sidebar),
            Box::new(file_list_wrapper),
        ]).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
            flex_direction: FlexDirection::Row,
            gap: Vector2::new(LengthPercentage::length(0.0), LengthPercentage::length(0.0)),
            ..Default::default()
        })),
        // Statusbar
        Box::new(statusbar),
    ]).with_layout_style(LayoutStyle {
        size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
        flex_direction: FlexDirection::Column,
        ..Default::default()
    })
}
