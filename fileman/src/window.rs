use nptk::prelude::*;
use async_trait::async_trait;
use nptk::core::signal::eval::EvalSignal;
use nptk::core::shortcut::Shortcut;
use nptk::core::window::KeyCode;
use nptk_fileman_widgets::file_list::{FileList, FileListOperation};
use nptk::services::filesystem::entry::FileEntry;
use nptk_fileman_widgets::FilemanSidebar;
// use nptk::widgets::breadcrumbs::{Breadcrumbs, BreadcrumbItem}; // Unused
use crate::app::AppState;
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
    Delete(Vec<PathBuf>),
    // CreateDirectory { parent: PathBuf, name: String }, // Unused
    // Rename { from: PathBuf, to: PathBuf }, // Unused
    PromptRename(PathBuf), // Prompt for new name for single file
    PromptCreateDirectory(PathBuf), // Prompt for new directory name in parent
    Properties(Vec<PathBuf>),
    // Future: Copy, Move, etc.
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
    operation_rx: Option<mpsc::UnboundedReceiver<FileOperationRequest>>,
    // Status message sender (for displaying operation results)
    status_tx: Option<mpsc::UnboundedSender<String>>,
    // Pending delete operations waiting for confirmation (from toolbar)
    pending_delete_confirmation: Arc<Mutex<Option<Vec<PathBuf>>>>,
    // Pending rename operations (from dialog)
    pending_rename: Arc<Mutex<Option<(PathBuf, String)>>>,
    // Pending create directory operations (from dialog)
    pending_create_dir: Arc<Mutex<Option<(PathBuf, String)>>>,
    // Clipboard service
    clipboard: Arc<Mutex<ClipboardService>>,
}

impl FileListWrapper {
    fn new(
        initial_path: PathBuf,
        navigation: Arc<Mutex<crate::navigation::NavigationState>>,
        navigation_rx: mpsc::UnboundedReceiver<PathBuf>,
        operation_rx: mpsc::UnboundedReceiver<FileOperationRequest>,
        status_tx: mpsc::UnboundedSender<String>,
        navigation_path_signal: StateSignal<PathBuf>,
    ) -> Self {
        // Create channel for FileList operations
        let (file_list_op_tx, file_list_op_rx) = mpsc::unbounded_channel::<FileListOperation>();
        
        // Create FileList (selection_change_tx is optional for backward compatibility)
        let file_list = FileList::new_with_operations(initial_path.clone(), Some(file_list_op_tx.clone()), None);
        
        // Clone signals from FileList for reactive subscription
        let file_list_path_signal = file_list.current_path_signal().clone();
        
        // Initialize clipboard
        let clipboard = Arc::new(Mutex::new(ClipboardService::new()));
        
        let file_list = file_list.with_on_context_menu({
            let nav_tx = navigation.clone();
            let op_tx = file_list_op_tx.clone();
            move |path: PathBuf, pos: Vector2<f64>, context: AppContext| {
                // Create native context menu using NPTK's MenuManager
                let mut template = MenuTemplate::new("context-menu");
                
                // Open action
                if path.is_dir() || path.is_file() {
                    let nav_tx_clone = nav_tx.clone();
                    let path_clone = path.clone();
                    template = template.add_item(
                        MenuItem::new(MenuCommand::Custom(1), "Open")
                            .with_action(move || {
                                if let Ok(mut n) = nav_tx_clone.lock() {
                                    n.navigate_to(path_clone.clone()); 
                                }
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
                
                // Delete action
                let op_tx_clone = op_tx.clone();
                let path_clone = path.clone();
                template = template.add_item(
                    MenuItem::new(MenuCommand::Custom(3), "Delete")
                        .with_action(move || {
                            let _ = op_tx_clone.send(FileListOperation::Delete(vec![path_clone.clone()]));
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
            operation_rx: Some(operation_rx),
            status_tx: Some(status_tx),
            pending_delete_confirmation: Arc::new(Mutex::new(None)),
            pending_rename: Arc::new(Mutex::new(None)),
            pending_create_dir: Arc::new(Mutex::new(None)),
            clipboard,
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

    /// Perform delete request
    fn perform_delete_request(&mut self, paths: Vec<PathBuf>, _context: AppContext) {
        let paths_clone = paths.clone();
        // Process delete operation
        let mut all_success = true;
        let mut error_msg = String::new();
        
        for path in &paths {
            match operations::delete_path(path.clone()) {
                Ok(_) => {
                    log::info!("Deleted: {:?}", path);
                }
                Err(e) => {
                    log::error!("Failed to delete {:?}: {}", path, e);
                    all_success = false;
                    error_msg = e;
                    break;
                }
            }
        }
        
        // Update status message
        if let Some(ref tx) = self.status_tx {
            if all_success {
                let _ = tx.send(format!("Deleted {} item(s)", paths_clone.len()));
            } else {
                let _ = tx.send(format!("Error: {}", error_msg));
            }
        }
        
        // Refresh file list by resetting path (triggers reload)
        let current_path = self.file_list.get_current_path();
        self.file_list.set_path(current_path);
    }

    /// Show delete confirmation dialog
    fn show_delete_confirmation_dialog(&self, paths: &[PathBuf], context: AppContext) {
        if paths.is_empty() {
            return;
        }

        // Build message text
        let message = if paths.len() == 1 {
            let path = &paths[0];
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("<unnamed>");
            format!("Are you sure you want to delete \"{}\"?", name)
        } else {
            format!("Are you sure you want to delete {} selected item(s)?", paths.len())
        };

        let pending_delete = self.pending_delete_confirmation.clone();
        let paths_to_delete = paths.to_vec();

        // Message text widget
        let message_text = Text::new(message);
        
        // Cancel button - closes dialog (popup closes automatically on click outside or ESC)
        let cancel_btn = Button::new(Text::new("Cancel".to_string()))
            .with_on_pressed(MaybeSignal::value(Update::DRAW));
        
        // Delete button - confirms deletion
        let delete_btn = Button::new(Text::new("Delete".to_string()))
            .with_on_pressed({
                let pending_delete_btn = pending_delete.clone();
                let paths_btn = paths_to_delete.clone();
                MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                    // Set pending delete confirmation - will be processed in update()
                    if let Ok(mut pending) = pending_delete_btn.lock() {
                        *pending = Some(paths_btn.clone());
                    }
                    Update::DRAW
                })))
            });

        // Build dialog content
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

        // Show popup at center of screen
        context
            .popup_manager
            .create_popup_at(Box::new(dialog_content), "Confirm Delete", (400, 150), (300, 200));
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
                FileListOperation::Delete(paths) => {
                     // Convert to FileOperationRequest and process
                    let paths_clone = paths.clone();
                    self.perform_delete_request(paths, context.clone());
                },
                FileListOperation::Sort(col, order) => {
                    self.file_list.sort(col, order);
                    update.insert(Update::DRAW);
                },
                FileListOperation::Refresh => {
                    let path = self.file_list.get_current_path();
                    self.file_list.set_path(path); // Re-setting path triggers refresh
                    update.insert(Update::DRAW);
                }
            }
        }

        // Process file operations from toolbar/other UI
        // Note: Delete operations need confirm, Properties need dialog
        // Collect operations first to avoid borrow conflicts
        let mut pending_deletes = Vec::new();
        let mut pending_properties = Vec::new();
        let mut pending_renames = Vec::new();
        let mut pending_creates = Vec::new();
        
        if let Some(ref mut rx) = self.operation_rx {
            while let Ok(op) = rx.try_recv() {
                match op {
                    FileOperationRequest::Delete(paths) => {
                        log::warn!("RECEIVED DELETE REQUEST for {} path(s)", paths.len());
                        pending_deletes.push(paths);
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
                }
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
        
        // Show confirmation dialogs for pending delete operations (after releasing borrow)
        if !pending_deletes.is_empty() {
            log::warn!("SHOWING {} DELETE CONFIRMATION DIALOG(S)", pending_deletes.len());
        }
        for paths in pending_deletes {
            self.show_delete_confirmation_dialog(&paths, context.clone());
            update.insert(Update::DRAW);
        }
        
        // Process confirmed delete operations from toolbar (user clicked "Delete" in confirmation dialog)
        if let Ok(mut pending_delete) = self.pending_delete_confirmation.lock() {
            if let Some(paths) = pending_delete.take() {
                // User confirmed - proceed with deletion
                let paths_clone = paths.clone();
                let mut all_success = true;
                let mut error_msg = String::new();
                
                for path in &paths {
                    match operations::delete_path(path.clone()) {
                        Ok(_) => {
                            log::info!("Deleted: {:?}", path);
                        }
                        Err(e) => {
                            log::error!("Failed to delete {:?}: {}", path, e);
                            all_success = false;
                            error_msg = e;
                            break;
                        }
                    }
                }
                
                // Update status message
                if let Some(ref tx) = self.status_tx {
                    if all_success {
                        let _ = tx.send(format!("Deleted {} item(s)", paths_clone.len()));
                    } else {
                        let _ = tx.send(format!("Error: {}", error_msg));
                    }
                }
                
                // Refresh file list
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

    // Create FilemanSidebar
    let mut sidebar = FilemanSidebar::new()
        .with_places(true)
        .with_bookmarks(true)
        .with_devices(true)
        .with_width(200.0)
        .with_current_path_signal(navigation_path_signal.clone());
    
    // Take the navigation receiver for FileListWrapper
    let sidebar_nav_rx = sidebar.take_navigation_receiver()
        .expect("FilemanSidebar should provide navigation receiver");

    // Create FileList wrapper that syncs with navigation state
    let mut file_list_wrapper = FileListWrapper::new(
        initial_path.clone(),
        nav_clone.clone(),
        sidebar_nav_rx,
        operation_rx,
        status_tx.clone(),
        navigation_path_signal.clone(),
    );
    
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

    // Create FileLocationBar
    use nptk_fileman_widgets::location_bar::FileLocationBar;
    
    // Get search query signal from FileList wrapper
    let file_list_search_query = file_list_wrapper.search_query_signal();

    let nav_tx_clone = toolbar_nav_tx.clone();
    let location_bar = FileLocationBar::new(navigation_path_signal.clone())
        .with_focus_receiver(focus_rx)
        .with_search_query_signal(file_list_search_query) // Pass the shared signal
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
