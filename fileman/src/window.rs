use nptk::prelude::*;
use nptk::core::signal::eval::EvalSignal;
use nptk::core::shortcut::{Shortcut, ShortcutRegistry};
use nptk::core::window::KeyCode;
use nptk_fileman_widgets::file_list::{FileList, FileListOperation};
use nptk_fileman_widgets::FilemanSidebar;
use nptk::widgets::breadcrumbs::{Breadcrumbs, BreadcrumbItem};
use crate::app::AppState;
use crate::operations;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;

/// File operation requests that can be sent from UI to be processed
#[derive(Debug, Clone)]
pub enum FileOperationRequest {
    Delete(Vec<PathBuf>),
    CreateDirectory { parent: PathBuf, name: String },
    Rename { from: PathBuf, to: PathBuf },
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
        let file_list = FileList::new_with_operations(initial_path.clone(), Some(file_list_op_tx), None);
        
        // Clone signals from FileList for reactive subscription
        let file_list_path_signal = file_list.current_path_signal().clone();
        
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
        }
    }

    /// Get the selected paths signal (for reactive subscription by other widgets)
    pub fn selected_paths_signal(&self) -> &StateSignal<Vec<PathBuf>> {
        self.file_list.selected_paths_signal()
    }

    /// Show properties popup for the given paths
    pub fn show_properties_for_paths(&mut self, paths: &[PathBuf], context: nptk::core::app::context::AppContext) {
        // Properties functionality is handled internally by FileListContent
        // This is a placeholder for the public API
        log::info!("Properties requested for: {:?}", paths);
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
            size: Vector2::new(Dimension::length(400.0), Dimension::auto()),
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
}

impl Widget for FileListWrapper {
    fn widget_id(&self) -> nptk::theme::id::WidgetId {
        self.file_list.widget_id()
    }

    fn layout_style(&self) -> nptk::core::layout::StyleNode {
        self.file_list.layout_style()
    }

    fn update(
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
        let file_list_update = self.file_list.update(layout, context.clone(), info);
        update |= file_list_update;

        // Path refresh/recovery logic: If current directory no longer exists, navigate to parent
        // This handles the case where a directory is deleted externally (similar to SerenityOS)
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
        if let Some(ref mut rx) = self.file_list_operation_rx {
            while let Ok(op) = rx.try_recv() {
                match op {
                    FileListOperation::Delete(paths) => {
                        // Convert to FileOperationRequest and process
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
                        
                        // Refresh file list
                        let current_path = self.file_list.get_current_path();
                        self.file_list.set_path(current_path.clone());
                        update.insert(Update::LAYOUT | Update::DRAW);
                    }
                }
            }
        }

        // Process file operations from toolbar/other UI
        // Note: Delete operations need confirmation, so show dialog first
        // Collect operations first to avoid borrow conflicts
        let mut pending_deletes = Vec::new();
        if let Some(ref mut rx) = self.operation_rx {
            while let Ok(op) = rx.try_recv() {
                match op {
                    FileOperationRequest::Delete(paths) => {
                        // Collect delete requests to show confirmation dialog
                        log::warn!("RECEIVED DELETE REQUEST for {} path(s)", paths.len());
                        pending_deletes.push(paths);
                    }
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
                    FileOperationRequest::Properties(paths) => {
                        // Show properties using the same mechanism as context menu
                        // We need to trigger the properties action through the FileList's operation channel
                        // For now, log the request - the actual implementation would need to be done
                        // through the FileList's internal operation system
                        log::info!("Properties requested for paths: {:?}", paths);
                        if let Some(ref tx) = self.status_tx {
                            let _ = tx.send("Properties functionality available via right-click".to_string());
                        }
                        update.insert(Update::DRAW);
                    }
                }
            }
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
        
        update
    }

    fn render(
        &mut self,
        graphics: &mut dyn nptk::core::vgi::Graphics,
        theme: &mut dyn nptk::theme::theme::Theme,
        layout: &nptk::core::layout::LayoutNode,
        info: &mut nptk::core::app::info::AppInfo,
        context: nptk::core::app::context::AppContext,
    ) {
        self.file_list.render(graphics, theme, layout, info, context)
    }
}

impl WidgetLayoutExt for FileListWrapper {
    fn set_layout_style(&mut self, layout_style: impl Into<nptk::core::signal::MaybeSignal<nptk::core::layout::LayoutStyle>>) {
        self.file_list.set_layout_style(layout_style)
    }
}

/// Helper function to convert PathBuf to breadcrumb items
fn path_to_breadcrumb_items(path: &PathBuf) -> Vec<BreadcrumbItem> {
    let mut items = Vec::new();
    let mut current_path = PathBuf::new();
    
    // Handle root path
    if path.has_root() {
        items.push(BreadcrumbItem::new("/").with_id("/".to_string()));
        current_path.push("/");
    }
    
    // Add each component
    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            current_path.push(name);
            let label = name.to_string_lossy().to_string();
            let id = current_path.to_string_lossy().to_string();
            items.push(BreadcrumbItem::new(label).with_id(id));
        }
    }
    
    // Last item is not clickable (current location)
    if let Some(last) = items.last_mut() {
        last.clickable = false;
    }
    
    items
}

/// Wrapper widget for location bar (breadcrumbs and text input) with bidirectional sync
struct LocationBarWrapper {
    inner: Container,
    navigation: Arc<Mutex<crate::navigation::NavigationState>>,
    navigation_tx: mpsc::UnboundedSender<crate::toolbar::NavigationAction>,
    navigation_path_signal: StateSignal<PathBuf>,
    breadcrumb_items_signal: StateSignal<Vec<BreadcrumbItem>>,
    text_input_value: StateSignal<String>,
    last_synced_nav_path: PathBuf, // Track last synced navigation path to only update text input when nav path changes
    signals_hooked: bool,
}

impl LocationBarWrapper {
    fn new(
        navigation: Arc<Mutex<crate::navigation::NavigationState>>,
        navigation_tx: mpsc::UnboundedSender<crate::toolbar::NavigationAction>,
        navigation_path_signal: StateSignal<PathBuf>,
    ) -> Self {
        let initial_path = (*navigation_path_signal.get()).clone();
        let initial_items = path_to_breadcrumb_items(&initial_path);
        let breadcrumb_items_signal = StateSignal::new(initial_items.clone());
        let initial_text = initial_path.to_string_lossy().to_string();
        let text_input_value = StateSignal::new(initial_text.clone());
        
        let nav_tx_clone1 = navigation_tx.clone();
        let nav_tx_clone2 = navigation_tx.clone();
        
        let breadcrumbs = Breadcrumbs::new()
            .with_items_signal(breadcrumb_items_signal.clone())
            .with_on_click(move |item: &BreadcrumbItem| {
                // Navigate to the clicked breadcrumb path
                if let Some(id) = &item.id {
                    let path = PathBuf::from(id);
                    if path.exists() {
                        let _ = nav_tx_clone1.send(crate::toolbar::NavigationAction::NavigateTo(path));
                        return Update::LAYOUT | Update::DRAW;
                    }
                }
                Update::empty()
            })
            .with_neighbors_provider(move |item: &BreadcrumbItem| {
                // Show sibling directories when clicking separator
                if let Some(id) = &item.id {
                    let parent_path = PathBuf::from(id);
                    if let Ok(entries) = std::fs::read_dir(&parent_path) {
                        let mut neighbors = Vec::new();
                        for entry in entries.flatten() {
                            if let Ok(metadata) = entry.metadata() {
                                if metadata.is_dir() {
                                    let entry_path = entry.path();
                                    let label = entry_path.file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let path_str = entry_path.to_string_lossy().to_string();
                                    neighbors.push(BreadcrumbItem::new(label).with_id(path_str));
                                }
                            }
                        }
                        if !neighbors.is_empty() {
                            return Some(neighbors);
                        }
                    }
                }
                None
            })
            .with_on_neighbor_select(move |_original_item: &BreadcrumbItem, selected_neighbor: &BreadcrumbItem| {
                // Navigate to selected neighbor directory
                if let Some(id) = &selected_neighbor.id {
                    let path = PathBuf::from(id);
                    if path.exists() {
                        let _ = nav_tx_clone2.send(crate::toolbar::NavigationAction::NavigateTo(path));
                        return Update::LAYOUT | Update::DRAW;
                    }
                }
                Update::empty()
            })
            .with_layout_style(LayoutStyle {
                size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
                ..Default::default()
            });
        
        let text_input = TextInput::new()
            .with_text_signal(text_input_value.clone())
            .with_placeholder("Path...".to_string())
            .with_layout_style(LayoutStyle {
                size: Vector2::new(Dimension::length(300.0), Dimension::length(30.0)),
                ..Default::default()
            });
        
        let container = Container::new(vec![
            Box::new(breadcrumbs),
            Box::new(text_input),
        ]).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
            flex_direction: FlexDirection::Row,
            gap: Vector2::new(LengthPercentage::length(0.0), LengthPercentage::length(0.0)),
            align_items: Some(AlignItems::Center),
            ..Default::default()
        });
        
        Self {
            inner: container,
            navigation,
            navigation_tx,
            navigation_path_signal,
            breadcrumb_items_signal,
            text_input_value,
            last_synced_nav_path: initial_path,
            signals_hooked: false,
        }
    }
}

impl Widget for LocationBarWrapper {
    fn widget_id(&self) -> nptk::theme::id::WidgetId {
        nptk::theme::id::WidgetId::new("fileman", "LocationBarWrapper")
    }

    fn layout_style(&self) -> nptk::core::layout::StyleNode {
        self.inner.layout_style()
    }

    fn update(
        &mut self,
        layout: &nptk::core::layout::LayoutNode,
        context: nptk::core::app::context::AppContext,
        info: &mut nptk::core::app::info::AppInfo,
    ) -> nptk::core::app::update::Update {
        let mut update = Update::empty();

        // Hook signals on first update
        if !self.signals_hooked {
            context.hook_signal(&mut self.navigation_path_signal);
            context.hook_signal(&mut self.breadcrumb_items_signal);
            context.hook_signal(&mut self.text_input_value);
            self.signals_hooked = true;
        }

        // Reactively update breadcrumb items when navigation path changes
        let nav_path = (*self.navigation_path_signal.get()).clone();
        let current_items = (*self.breadcrumb_items_signal.get()).clone();
        let new_items = path_to_breadcrumb_items(&nav_path);
        
        // Only update if items changed (compare by path IDs to avoid unnecessary updates)
        if current_items.len() != new_items.len() 
            || current_items.iter().zip(new_items.iter()).any(|(a, b)| a.id != b.id) {
            self.breadcrumb_items_signal.set(new_items);
            update |= Update::LAYOUT | Update::DRAW;
        }

        // Sync text input value from navigation path signal (only when navigation path changes)
        // Don't overwrite user input - only sync when the navigation path itself changes
        if nav_path != self.last_synced_nav_path {
            let path_str = nav_path.to_string_lossy().to_string();
            self.text_input_value.set(path_str);
            self.last_synced_nav_path = nav_path;
            update |= Update::LAYOUT | Update::DRAW;
        }

        // Update inner Container (which updates both breadcrumbs and text_input)
        // Note: TextInput handles its own keyboard input internally via its signal binding
        update |= self.inner.update(layout, context, info);
        
        update
    }

    fn render(
        &mut self,
        graphics: &mut dyn nptk::core::vgi::Graphics,
        theme: &mut dyn nptk::theme::theme::Theme,
        layout: &nptk::core::layout::LayoutNode,
        info: &mut nptk::core::app::info::AppInfo,
        context: nptk::core::app::context::AppContext,
    ) {
        self.inner.render(graphics, theme, layout, info, context)
    }
}

impl nptk::core::widget::WidgetLayoutExt for LocationBarWrapper {
    fn set_layout_style(&mut self, layout_style: impl Into<nptk::core::signal::MaybeSignal<nptk::core::layout::LayoutStyle>>) {
        self.inner.set_layout_style(layout_style)
    }
}

/// Status update information
#[derive(Clone, Debug)]
pub struct StatusUpdate {
    pub message: Option<String>, // Temporary message (operation result, error, etc.)
    pub path: Option<PathBuf>,   // Current path
    pub file_count: Option<usize>, // Total file count
    pub selection_count: Option<usize>, // Selected file count
}

/// Wrapper widget for status bar with dynamic content
struct StatusBarWrapper {
    inner: Container,
    navigation: Arc<Mutex<crate::navigation::NavigationState>>,
    navigation_path_signal: StateSignal<PathBuf>,
    selected_paths_signal: StateSignal<Vec<PathBuf>>,
    status_rx: Option<mpsc::UnboundedReceiver<String>>, // Temporary operation messages
    status_text: StateSignal<String>,
    status_message_timeout: Option<std::time::Instant>,
    signals_hooked: bool,
}

impl StatusBarWrapper {
    fn new(
        navigation: Arc<Mutex<crate::navigation::NavigationState>>,
        navigation_path_signal: StateSignal<PathBuf>,
        selected_paths_signal: StateSignal<Vec<PathBuf>>,
        status_rx: mpsc::UnboundedReceiver<String>,
    ) -> Self {
        let status_text = StateSignal::new("Ready".to_string());
        
        let status_text_clone = status_text.clone();
        let container = Container::new(vec![
            Box::new(Text::new(status_text_clone.maybe())),
        ]).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::length(24.0)),
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            padding: nptk::core::layout::Rect {
                left: LengthPercentage::length(8.0),
                right: LengthPercentage::length(8.0),
                top: LengthPercentage::length(4.0),
                bottom: LengthPercentage::length(4.0),
            },
            ..Default::default()
        });

        Self {
            inner: container,
            navigation,
            navigation_path_signal,
            selected_paths_signal,
            status_rx: Some(status_rx),
            status_text,
            status_message_timeout: None,
            signals_hooked: false,
        }
    }

    fn update_status_from_navigation(&mut self) {
        // Check if timeout expired for status messages
        if let Some(timeout) = self.status_message_timeout {
            if timeout.elapsed() > std::time::Duration::from_secs(3) {
                self.status_message_timeout = None;
                // Update to show current path after message timeout
                let nav_path = (*self.navigation_path_signal.get()).clone();
                let path_str = nav_path.to_string_lossy().to_string();
                let selection_count = (*self.selected_paths_signal.get()).len();
                let status_msg = if selection_count > 0 {
                    format!("{} - {} item(s) selected", path_str, selection_count)
                } else {
                    path_str
                };
                self.status_text.set(status_msg);
            }
        } else {
            // No temporary message - show current path (with selection count if applicable)
            let nav_path = (*self.navigation_path_signal.get()).clone();
            let path_str = nav_path.to_string_lossy().to_string();
            let selection_count = (*self.selected_paths_signal.get()).len();
            let status_msg = if selection_count > 0 {
                format!("{} - {} item(s) selected", path_str, selection_count)
            } else {
                path_str
            };
            // Only update if status actually changed to avoid unnecessary updates
            let current_status = (*self.status_text.get()).clone();
            let should_update = current_status != status_msg 
                && !current_status.starts_with("Error:") 
                && !current_status.contains("Created") 
                && !current_status.contains("Deleted");
            if should_update {
                self.status_text.set(status_msg);
            }
        }
    }
}

impl Widget for StatusBarWrapper {
    fn widget_id(&self) -> nptk::theme::id::WidgetId {
        nptk::theme::id::WidgetId::new("fileman", "StatusBarWrapper")
    }

    fn layout_style(&self) -> nptk::core::layout::StyleNode {
        self.inner.layout_style()
    }

    fn update(
        &mut self,
        layout: &nptk::core::layout::LayoutNode,
        context: nptk::core::app::context::AppContext,
        info: &mut nptk::core::app::info::AppInfo,
    ) -> nptk::core::app::update::Update {
        let mut update = Update::empty();

        // Hook signals on first update
        if !self.signals_hooked {
            context.hook_signal(&mut self.status_text);
            context.hook_signal(&mut self.navigation_path_signal);
            context.hook_signal(&mut self.selected_paths_signal);
            self.signals_hooked = true;
        }

        // Poll status messages from operations (these are temporary messages)
        let mut has_active_temporary_message = false;
        if let Some(ref mut rx) = self.status_rx {
            while let Ok(msg) = rx.try_recv() {
                self.status_text.set(msg.clone());
                self.status_message_timeout = Some(std::time::Instant::now());
                update.insert(Update::DRAW);
            }
        }
        
        // Check if we have an active temporary message (within timeout)
        if let Some(timeout) = self.status_message_timeout {
            if timeout.elapsed() <= std::time::Duration::from_secs(3) {
                has_active_temporary_message = true;
            }
        }

        // Priority: 1) Temporary messages, 2) Framework status bar text (button status tips), 3) Default navigation info
        if !has_active_temporary_message {
            // Get framework status bar text (from button status tips)
            let framework_status_text = context.status_bar.get_text();
            if !framework_status_text.is_empty() {
                // Framework status bar has text (e.g., from button hover) - use it
                self.status_text.set(framework_status_text);
                update.insert(Update::DRAW);
            } else {
                // No framework status text - update status from navigation
                self.update_status_from_navigation();
            }
        }
        // If has_active_temporary_message is true, status_text was already set when the message was received
        
        // If status changed, trigger redraw
        if !update.is_empty() {
            update.insert(Update::DRAW);
        }

        // Update inner container
        update |= self.inner.update(layout, context, info);
        update
    }

    fn render(
        &mut self,
        graphics: &mut dyn nptk::core::vgi::Graphics,
        theme: &mut dyn nptk::theme::theme::Theme,
        layout: &nptk::core::layout::LayoutNode,
        info: &mut nptk::core::app::info::AppInfo,
        context: nptk::core::app::context::AppContext,
    ) {
        self.inner.render(graphics, theme, layout, info, context)
    }
}

impl nptk::core::widget::WidgetLayoutExt for StatusBarWrapper {
    fn set_layout_style(&mut self, layout_style: impl Into<nptk::core::signal::MaybeSignal<nptk::core::layout::LayoutStyle>>) {
        self.inner.set_layout_style(layout_style)
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
    
    // Register keyboard shortcuts
    // TODO: Implement focus text input functionality for "Go to Location" shortcuts
    context.shortcut_registry.register(
        Shortcut::ctrl(KeyCode::KeyL),
        || Update::DRAW, // Placeholder - will implement focus text input later
    );
    context.shortcut_registry.register(
        Shortcut::new(KeyCode::F6, nptk::core::window::ModifiersState::empty()),
        || Update::DRAW, // Placeholder - will implement focus text input later
    );

    // Create FilemanSidebar
    let mut sidebar = FilemanSidebar::new()
        .with_places(true)
        .with_bookmarks(true)
        .with_width(200.0);
    
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

    // Clone selected paths signal from FileList for ToolbarWrapper and StatusBarWrapper
    let selected_paths_signal = file_list_wrapper.selected_paths_signal().clone();

    // Create ToolbarWrapper
    let (mut toolbar_wrapper, toolbar_nav_tx) = crate::toolbar::ToolbarWrapper::new(
        nav_clone.clone(),
        operation_tx.clone(),
        navigation_path_signal.clone(),
        selected_paths_signal.clone(),
    );

    // Create LocationBarWrapper
    let location_bar = LocationBarWrapper::new(
        nav_clone.clone(),
        toolbar_nav_tx.clone(),
        navigation_path_signal.clone(),
    );

    // Create StatusBarWrapper
    let statusbar = StatusBarWrapper::new(
        nav_clone.clone(),
        navigation_path_signal.clone(),
        selected_paths_signal.clone(),
        status_rx,
    );

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
