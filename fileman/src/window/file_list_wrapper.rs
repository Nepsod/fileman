use super::file_operation::FileOperationRequest;
use async_trait::async_trait;
use crate::config::DeletePolicy;
use crate::operations;
use nalgebra::Vector2;
use nptk::core::menu::unified::{MenuItem, MenuTemplate};
use nptk::core::menu::MenuCommand;
use nptk::core::model::SortOrder;
use nptk::core::signal::eval::EvalSignal;
use nptk::core::vg::kurbo::Point;
use nptk::prelude::*;
use nptk::services::clipboard::ClipboardService;
use nptk::services::filesystem::entry::FileEntry;
use nptk_fileman_widgets::file_list::{FileList, FileListOperation, SearchScope};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Wrapper widget that manages FileList and connects it to navigation state
pub(crate) struct FileListWrapper {
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
    delete_policy: Arc<Mutex<DeletePolicy>>,
    /// From config `[System].Terminal`; superseded by `TERMINAL` env at spawn time.
    terminal_command: Arc<Mutex<Option<String>>>,
    config_path: Option<PathBuf>,
    show_hidden_files_signal: StateSignal<bool>,
}

impl FileListWrapper {
    pub(crate) fn new(
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
        delete_policy: Arc<Mutex<DeletePolicy>>,
        terminal_command: Arc<Mutex<Option<String>>>,
        config_path: Option<PathBuf>,
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
            .with_show_hidden_files_signal(show_hidden_files_signal.clone());
        
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
            config_path,
            show_hidden_files_signal,
        }
    }

    pub(crate) fn apply_startup_folder_view(&mut self, fileman: &crate::config::FilemanConfig) {
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

        let confirm_delete = self
            .delete_policy
            .lock()
            .map(|p| p.confirm_delete)
            .unwrap_or(true);
        let confirm_trash = self
            .delete_policy
            .lock()
            .map(|p| p.confirm_trash)
            .unwrap_or(true);

        if permanent && !confirm_delete {
            if let Ok(mut p) = self.pending_delete_confirmation.lock() {
                *p = Some((paths.to_vec(), true));
            }
            return;
        }
        if !permanent && !confirm_trash {
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

        let ctx_cancel = context.clone();
        let dialog_content = StandardModalLayout::build(
            vec![Box::new(message_text)],
            vec![
                DialogButton::new("Cancel", {
                    context.callback(move || {
                        ctx_cancel.close_top_popup();
                        Update::DRAW
                    })
                }),
                DialogButton::new(confirm_label, {
                    let pending_delete_btn = pending_delete.clone();
                    let paths_btn = paths_to_delete.clone();
                    MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                        if let Ok(mut pending) = pending_delete_btn.lock() {
                            *pending = Some((paths_btn.clone(), permanent));
                        }
                        Update::DRAW
                    })))
                }),
            ],
        );

        open_popup_at(&context, title, (400, 150), (300, 200), Box::new(dialog_content));
    }

    fn show_about_dialog(&self, context: AppContext) {
        const ABOUT_TITLE_PX: f32 = 14.0;
        const ABOUT_BODY_PX: f32 = 11.0;

        let full_row = LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
            ..Default::default()
        };

        let title_line = Text::new(env!("CARGO_PKG_NAME").to_string())
            .with_font_size(ABOUT_TITLE_PX)
            .with_layout_style(full_row.clone());
        let version_line = Text::new(format!(
            "Version\u{00A0}{}",
            env!("CARGO_PKG_VERSION")
        ))
            .with_font_size(ABOUT_BODY_PX + 1.4)
            .with_layout_style(full_row.clone());
        let blurb = Text::new(env!("CARGO_PKG_DESCRIPTION").to_string())
            .with_font_size(ABOUT_BODY_PX)
            .with_layout_style(full_row.clone());
        let authors_line = Text::new(format!(
            "Authors:\u{00A0}{}",
            env!("CARGO_PKG_AUTHORS")
        ))
            .with_font_size(ABOUT_BODY_PX)
            .with_layout_style(full_row.clone());
        let license_line = Text::new(format!("License:\u{00A0}{}", env!("CARGO_PKG_LICENSE")))
            .with_font_size(ABOUT_BODY_PX)
            .with_layout_style(full_row.clone());

        let repo_url = env!("CARGO_PKG_REPOSITORY");
        let repo_display = if let Some(slash_idx) = repo_url.rfind('/') {
            format!(
                "Repository:\n{}\u{2060}{}",
                &repo_url[..=slash_idx],
                &repo_url[slash_idx + 1..]
            )
        } else {
            format!("Repository:\n{}", repo_url)
        };
        let repo_line = Text::new(repo_display)
            .with_font_size(ABOUT_BODY_PX)
            .with_layout_style(full_row.clone());

        let ctx_close = context.clone();
        let dialog_content = StandardModalLayout::build_with_style(
            vec![
                Box::new(title_line),
                Box::new(version_line),
                Box::new(blurb),
                Box::new(authors_line),
                Box::new(license_line),
                Box::new(repo_line),
            ],
            vec![DialogButton::new("Close", {
                context.callback(move || {
                    ctx_close.close_top_popup();
                    Update::DRAW
                })
            })
            .with_font_size(14.0)],
            StandardModalStyle::about_like(),
        );

        open_popup_at(
            &context,
            "About Fileman",
            (400, 220),
            (400, 380),
            Box::new(dialog_content),
        );
    }

    fn show_settings_dialog(&self, context: AppContext) {
        crate::settings_dialog::open_configure_fileman_popup(
            context,
            self.config_path.clone(),
            self.show_hidden_files_signal.clone(),
            self.delete_policy.clone(),
            self.terminal_command.clone(),
        );
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
        
        let ctx_cancel = context.clone();
        let dialog_content = StandardModalLayout::build(
            vec![
                Box::new(message_text),
                Box::new(input_field),
            ],
            vec![
                DialogButton::new("Cancel", {
                    context.callback(move || {
                        ctx_cancel.close_top_popup();
                        Update::DRAW
                    })
                }),
                DialogButton::new("Rename", {
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
                }),
            ],
        );

        open_popup_at(&context, "Rename File", (400, 200), (300, 250), Box::new(dialog_content));
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
        
        let ctx_cancel = context.clone();
        let dialog_content = StandardModalLayout::build(
            vec![Box::new(message_text), Box::new(input_field)],
            vec![
                DialogButton::new("Cancel", {
                    context.callback(move || {
                        ctx_cancel.close_top_popup();
                        Update::DRAW
                    })
                }),
                DialogButton::new("Create", {
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
                }),
            ],
        );

        open_popup_at(&context, "New Folder", (400, 200), (300, 250), Box::new(dialog_content));
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

        let ctx_cancel = context.clone();
        let dialog_content = StandardModalLayout::build(
            vec![Box::new(message_text), Box::new(input_field)],
            vec![
                DialogButton::new("Cancel", {
                    context.callback(move || {
                        ctx_cancel.close_top_popup();
                        Update::DRAW
                    })
                }),
                DialogButton::new("Create", {
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
                }),
            ],
        );

        open_popup_at(&context, "New File", (400, 200), (300, 250), Box::new(dialog_content));
    }

    fn refresh_file_list_at_current_path(&mut self) -> Update {
        let current_path = self.file_list.get_current_path();
        self.file_list.set_path(current_path.clone());
        Update::LAYOUT | Update::DRAW
    }

    fn ensure_signals_hooked(&mut self, context: &AppContext) {
        if !self.signals_hooked {
            context.hook_signal(&mut self.navigation_path_signal);
            context.hook_signal(&mut self.file_list_path_signal);
            self.signals_hooked = true;
        }
    }

    fn drain_sidebar_navigation_rx(&mut self) -> Update {
        let mut update = Update::empty();
        if let Some(ref mut rx) = self.navigation_rx {
            while let Ok(path) = rx.try_recv() {
                if let Ok(mut nav) = self.navigation.lock() {
                    nav.navigate_to(path.clone());
                    update.insert(Update::LAYOUT | Update::DRAW);
                }
            }
        }
        update
    }

    fn sync_navigation_path_into_file_list_if_differ(&mut self) -> Update {
        let mut update = Update::empty();
        let nav_path = (*self.navigation_path_signal.get()).clone();
        let file_list_path = (*self.file_list_path_signal.get()).clone();
        if nav_path != file_list_path {
            self.file_list.set_path(nav_path.clone());
            update.insert(Update::LAYOUT | Update::DRAW);
        }
        update
    }

    fn recover_file_list_path_if_missing(&mut self) -> Update {
        let mut update = Update::empty();
        let current_path = (*self.file_list_path_signal.get()).clone();
        if !current_path.exists() {
            let mut recovery_path = current_path.clone();
            while !recovery_path.exists() && recovery_path != PathBuf::from("/") {
                if let Some(parent) = recovery_path.parent() {
                    recovery_path = parent.to_path_buf();
                } else {
                    break;
                }
            }
            if recovery_path.exists() && recovery_path != current_path {
                if let Ok(mut nav) = self.navigation.lock() {
                    nav.navigate_to(recovery_path.clone());
                    self.file_list.set_path(recovery_path);
                    update.insert(Update::LAYOUT | Update::DRAW);
                }
            }
        }
        update
    }

    fn poll_folder_duplicate_done_channel(&mut self) -> Update {
        let mut update = Update::empty();
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
        update
    }

    fn sync_file_list_path_into_navigation_if_differ(&mut self, nav_path: PathBuf) -> Update {
        let mut update = Update::empty();
        let file_list_path_after = (*self.file_list_path_signal.get()).clone();
        if file_list_path_after != nav_path {
            if let Ok(mut nav) = self.navigation.lock() {
                nav.navigate_to(file_list_path_after.clone());
                update.insert(Update::LAYOUT | Update::DRAW);
            }
        }
        update
    }

    fn handle_drained_file_list_operations(
        &mut self,
        context: AppContext,
        update: &mut Update,
    ) {
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
                }
                FileListOperation::Copy(paths) => {
                    if let Ok(mut clipboard) = self.clipboard.lock() {
                        if let Err(e) = clipboard.set_files(&paths, false) {
                            log::error!("Failed to copy files: {}", e);
                            if let Some(tx) = &self.status_tx {
                                let _ = tx.send(format!("Failed to copy: {}", e));
                            }
                        } else if let Some(tx) = &self.status_tx {
                            let _ = tx.send(format!("Copied {} files", paths.len()));
                        }
                    }
                }
                FileListOperation::Cut(paths) => {
                    if let Ok(mut clipboard) = self.clipboard.lock() {
                        if let Err(e) = clipboard.set_files(&paths, true) {
                            log::error!("Failed to cut files: {}", e);
                            if let Some(tx) = &self.status_tx {
                                let _ = tx.send(format!("Failed to cut: {}", e));
                            }
                        } else if let Some(tx) = &self.status_tx {
                            let _ = tx.send(format!("Cut {} files", paths.len()));
                        }
                    }
                }
                FileListOperation::Paste => {
                    update.insert(self.paste_files());
                }
                FileListOperation::DeleteToTrash(paths) => {
                    let use_trash = self
                        .delete_policy
                        .lock()
                        .map(|p| p.use_trash)
                        .unwrap_or(true);
                    if use_trash {
                        let _ = self.operation_tx.send(FileOperationRequest::Delete(paths));
                    } else {
                        let _ = self
                            .operation_tx
                            .send(FileOperationRequest::DeletePermanent(paths));
                    }
                }
                FileListOperation::DeletePermanent(paths) => {
                    let _ = self
                        .operation_tx
                        .send(FileOperationRequest::DeletePermanent(paths));
                }
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
                }
                FileListOperation::Refresh => {
                    let path = self.file_list.get_current_path();
                    self.file_list.set_path(path);
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
    }

    fn process_operation_request_channel_and_followups(
        &mut self,
        context: AppContext,
        update: &mut Update,
    ) {
        let mut pending_deletes: Vec<(Vec<PathBuf>, bool)> = Vec::new();
        let mut pending_properties = Vec::new();
        let mut pending_renames = Vec::new();
        let mut pending_creates = Vec::new();
        let mut pending_create_files = Vec::new();
        let mut deferred_operation_channel_ops: Vec<FileOperationRequest> = Vec::new();
        let mut pending_show_about = false;
        let mut pending_show_settings = false;

        let mut drained_requests = Vec::new();
        if let Some(ref mut rx) = self.operation_rx {
            while let Ok(op) = rx.try_recv() {
                drained_requests.push(op);
            }
        }
        for op in drained_requests {
            match op {
                    FileOperationRequest::Delete(paths) => {
                        log::warn!("RECEIVED DELETE REQUEST for {} path(s)", paths.len());
                        let use_trash = self
                            .delete_policy
                            .lock()
                            .map(|p| p.use_trash)
                            .unwrap_or(true);
                        let permanent = !use_trash;
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
                        let term_owned: Option<String> = self
                            .terminal_command
                            .lock()
                            .ok()
                            .and_then(|guard| guard.clone());
                        match crate::terminal::open_terminal_in_directory(
                            &cwd,
                            term_owned.as_deref(),
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
                    FileOperationRequest::ShowAbout => {
                        pending_show_about = true;
                    }
                    FileOperationRequest::ShowSettings => {
                        pending_show_settings = true;
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
                            update.insert(self.refresh_file_list_at_current_path());
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

        for paths in pending_properties {
            self.show_properties_for_paths(&paths, context.clone());
            update.insert(Update::DRAW);
        }

        for path in pending_renames {
            self.show_rename_dialog(path, context.clone());
            update.insert(Update::DRAW);
        }

        for parent in pending_creates {
            self.show_new_folder_dialog(parent, context.clone());
            update.insert(Update::DRAW);
        }

        for parent in pending_create_files {
            self.show_new_file_dialog(parent, context.clone());
            update.insert(Update::DRAW);
        }

        if pending_show_about {
            self.show_about_dialog(context.clone());
            update.insert(Update::DRAW);
        }

        if pending_show_settings {
            self.show_settings_dialog(context.clone());
            update.insert(Update::DRAW);
        }

        if !pending_deletes.is_empty() {
            log::warn!(
                "SHOWING {} DELETE CONFIRMATION DIALOG(S)",
                pending_deletes.len()
            );
        }
        for (paths, permanent) in pending_deletes {
            self.show_delete_confirmation_dialog(&paths, permanent, context.clone());
            update.insert(Update::DRAW);
        }

        let delete_confirm_job = match self.pending_delete_confirmation.lock() {
            Ok(mut pending_delete) => pending_delete.take(),
            Err(_) => None,
        };
        if let Some((paths, permanent)) = delete_confirm_job {
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

            update.insert(self.refresh_file_list_at_current_path());
        }

        let rename_job = match self.pending_rename.lock() {
            Ok(mut pending) => pending.take(),
            Err(_) => None,
        };
        if let Some((path, new_name)) = rename_job {
            if let Some(parent) = path.parent() {
                let new_path = parent.join(new_name);
                match operations::rename_path(path.clone(), new_path.clone()) {
                    Ok(_) => {
                        log::info!("Renamed: {:?} -> {:?}", path, new_path);
                        if let Some(ref tx) = self.status_tx {
                            let _ = tx.send("Renamed successfully".to_string());
                        }
                        update.insert(self.refresh_file_list_at_current_path());
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

        let create_dir_job = match self.pending_create_dir.lock() {
            Ok(mut pending) => pending.take(),
            Err(_) => None,
        };
        if let Some((parent, name)) = create_dir_job {
            let new_dir = parent.join(name);
            match operations::create_directory(new_dir.clone()) {
                Ok(_) => {
                    log::info!("Created directory: {:?}", new_dir);
                    if let Some(ref tx) = self.status_tx {
                        let _ = tx.send("Directory created".to_string());
                    }
                    update.insert(self.refresh_file_list_at_current_path());
                }
                Err(e) => {
                    log::error!("Failed to create directory {:?}: {}", new_dir, e);
                    if let Some(ref tx) = self.status_tx {
                        let _ = tx.send(format!("Error: {}", e));
                    }
                }
            }
        }

        let create_file_job = match self.pending_create_file.lock() {
            Ok(mut pending) => pending.take(),
            Err(_) => None,
        };
        if let Some((parent, name)) = create_file_job {
            let new_file = parent.join(name);
            match operations::create_file(new_file.clone()) {
                Ok(_) => {
                    log::info!("Created file: {:?}", new_file);
                    if let Some(ref tx) = self.status_tx {
                        let _ = tx.send("File created".to_string());
                    }
                    update.insert(self.refresh_file_list_at_current_path());
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

        self.ensure_signals_hooked(&context);
        update |= self.drain_sidebar_navigation_rx();

        let nav_path = (*self.navigation_path_signal.get()).clone();
        update |= self.sync_navigation_path_into_file_list_if_differ();

        let file_list_update = self.file_list.update(layout, context.clone(), info).await;
        update |= file_list_update;

        update |= self.recover_file_list_path_if_missing();
        update |= self.poll_folder_duplicate_done_channel();
        update |= self.sync_file_list_path_into_navigation_if_differ(nav_path);

        self.handle_drained_file_list_operations(context.clone(), &mut update);
        self.process_operation_request_channel_and_followups(context, &mut update);

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
