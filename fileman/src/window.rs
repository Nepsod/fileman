use nptk::prelude::*;
use nptk_fileman_widgets::file_list::{FileList, FileListOperation};
use nptk_fileman_widgets::FilemanSidebar;
use crate::app::AppState;
use crate::operations;
use crate::toolbar;
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
    // Future: Copy, Move, etc.
}

/// Wrapper widget that manages FileList and connects it to navigation state
struct FileListWrapper {
    file_list: FileList,
    navigation: Arc<Mutex<crate::navigation::NavigationState>>,
    last_path: PathBuf,
    navigation_rx: Option<mpsc::UnboundedReceiver<PathBuf>>,
    // Track if we need to check path sync (only after navigation-related events)
    should_check_path_sync: bool,
    // File operation processing - receives from FileList widget
    file_list_operation_rx: Option<mpsc::UnboundedReceiver<FileListOperation>>,
    // File operation processing - receives from toolbar/other UI
    operation_rx: Option<mpsc::UnboundedReceiver<FileOperationRequest>>,
    // Status message sender (for displaying operation results)
    status_tx: Option<mpsc::UnboundedSender<String>>,
    // Selected paths request/response channels for toolbar delete button
    selected_paths_request_rx: Option<mpsc::UnboundedReceiver<()>>,
    selected_paths_response_tx: Option<mpsc::UnboundedSender<Vec<PathBuf>>>,
}

impl FileListWrapper {
    fn new(
        initial_path: PathBuf,
        navigation: Arc<Mutex<crate::navigation::NavigationState>>,
        navigation_rx: mpsc::UnboundedReceiver<PathBuf>,
        operation_rx: mpsc::UnboundedReceiver<FileOperationRequest>,
        status_tx: mpsc::UnboundedSender<String>,
        selected_paths_request_rx: mpsc::UnboundedReceiver<()>,
        selected_paths_response_tx: mpsc::UnboundedSender<Vec<PathBuf>>,
    ) -> Self {
        // Create channel for FileList operations
        let (file_list_op_tx, file_list_op_rx) = mpsc::unbounded_channel::<FileListOperation>();
        
        Self {
            file_list: FileList::new_with_operations(initial_path.clone(), Some(file_list_op_tx)),
            navigation,
            last_path: initial_path,
            navigation_rx: Some(navigation_rx),
            should_check_path_sync: false,
            file_list_operation_rx: Some(file_list_op_rx),
            operation_rx: Some(operation_rx),
            status_tx: Some(status_tx),
            selected_paths_request_rx: Some(selected_paths_request_rx),
            selected_paths_response_tx: Some(selected_paths_response_tx),
        }
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

        // Poll navigation events from sidebar (only check when we have events)
        if let Some(ref mut rx) = self.navigation_rx {
            // Use try_recv to poll non-blockingly
            while let Ok(path) = rx.try_recv() {
                if let Ok(mut nav) = self.navigation.lock() {
                    nav.navigate_to(path.clone());
                    self.file_list.set_path(path.clone());
                    self.last_path = path;
                    self.should_check_path_sync = true;
                    update.insert(Update::LAYOUT | Update::DRAW);
                }
            }
        }

        // Check if navigation state changed externally (e.g., from toolbar buttons)
        // ToolbarWrapper updates NavigationState directly, so we need to detect those changes
        if let Ok(nav) = self.navigation.lock() {
            let current_nav_path = nav.get_current_path();
            if current_nav_path != self.last_path {
                // Navigation changed externally (e.g., from toolbar)
                self.file_list.set_path(current_nav_path.clone());
                self.last_path = current_nav_path;
                self.should_check_path_sync = true;
                update.insert(Update::LAYOUT | Update::DRAW);
            }
        }

        // Update the wrapped FileList first to let it handle internal navigation
        let file_list_update = self.file_list.update(layout, context.clone(), info);
        update |= file_list_update;

        // Only check path sync when:
        // 1. We received navigation from sidebar (should_check_path_sync is set), OR
        // 2. FileList requested LAYOUT update (structural change, might be navigation)
        // This avoids checking on every hover/redraw cycle which causes infinite loops
        if self.should_check_path_sync || file_list_update.contains(Update::LAYOUT) {
            self.should_check_path_sync = false;
            
            // Check if FileList's path has changed internally (e.g., from double-click navigation)
            let file_list_path = self.file_list.get_current_path();
            if file_list_path != self.last_path {
                // Sync FileList's path change to NavigationState
                if let Ok(mut nav) = self.navigation.lock() {
                    nav.navigate_to(file_list_path.clone());
                    self.last_path = file_list_path;
                    update.insert(Update::LAYOUT | Update::DRAW);
                }
            } else {
                // Check if navigation path has changed externally (e.g., from toolbar buttons)
                if let Ok(nav) = self.navigation.lock() {
                    let current_nav_path = nav.get_current_path();
                    if current_nav_path != self.last_path {
                        self.file_list.set_path(current_nav_path.clone());
                        self.last_path = current_nav_path;
                        update.insert(Update::LAYOUT | Update::DRAW);
                    }
                }
            }
        }

        // Handle selected paths requests (for toolbar delete button)
        if let Some(ref mut rx) = self.selected_paths_request_rx {
            while rx.try_recv().is_ok() {
                // Respond with current selection
                let selected = self.file_list.selected_paths();
                if let Some(ref tx) = self.selected_paths_response_tx {
                    let _ = tx.send(selected);
                }
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
        if let Some(ref mut rx) = self.operation_rx {
            while let Ok(op) = rx.try_recv() {
                match op {
                    FileOperationRequest::Delete(paths) => {
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
                                    break; // Stop on first error
                                }
                            }
                        }
                        
                        // Update status message
                        if let Some(ref tx) = self.status_tx {
                            if all_success {
                                let _ = tx.send(format!("Deleted {} item(s)", paths.len()));
                            } else {
                                let _ = tx.send(format!("Error: {}", error_msg));
                            }
                        }
                        
                        // Refresh file list
                        let current_path = self.file_list.get_current_path();
                        self.file_list.set_path(current_path.clone());
                        update.insert(Update::LAYOUT | Update::DRAW);
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
                }
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

    /// Wrapper widget for location bar (path input) with bidirectional sync
struct LocationBarWrapper {
    inner: TextInput,
    navigation: Arc<Mutex<crate::navigation::NavigationState>>,
    navigation_tx: mpsc::UnboundedSender<crate::toolbar::NavigationAction>,
    current_path_text: StateSignal<String>,
    last_synced_path: PathBuf,
    signals_hooked: bool,
}

impl LocationBarWrapper {
    fn new(
        navigation: Arc<Mutex<crate::navigation::NavigationState>>,
        navigation_tx: mpsc::UnboundedSender<crate::toolbar::NavigationAction>,
    ) -> Self {
        let initial_path = {
            if let Ok(nav) = navigation.lock() {
                nav.get_current_path()
            } else {
                PathBuf::from("/")
            }
        };
        
        let initial_text = initial_path.to_string_lossy().to_string();
        
        Self {
            inner: TextInput::new().with_placeholder("Location...".to_string()),
            navigation,
            navigation_tx,
            current_path_text: StateSignal::new(initial_text.clone()),
            last_synced_path: initial_path,
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
            context.hook_signal(&mut self.current_path_text);
            self.signals_hooked = true;
        }

        // Sync text from navigation state changes
        if let Ok(nav) = self.navigation.lock() {
            let current_path = nav.get_current_path();
            if current_path != self.last_synced_path {
                self.current_path_text.set(current_path.to_string_lossy().to_string());
                self.last_synced_path = current_path.clone();
            }
        }

        // Check for Enter key press to navigate to entered path
        // Note: TextInput handles keys internally, so we check in update after TextInput processes them
        // For now, location bar navigation will be handled via a different mechanism
        // TODO: Implement proper Enter key detection for TextInput - may require custom TextInput wrapper
        // or checking for focus changes + Enter key combination

        // Update inner TextInput
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
    status_rx: Option<mpsc::UnboundedReceiver<String>>, // Temporary operation messages
    status_text: StateSignal<String>,
    status_message_timeout: Option<std::time::Instant>,
    signals_hooked: bool,
}

impl StatusBarWrapper {
    fn new(
        navigation: Arc<Mutex<crate::navigation::NavigationState>>,
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
                if let Ok(nav) = self.navigation.lock() {
                    let current_path = nav.get_current_path();
                    self.status_text.set(current_path.to_string_lossy().to_string());
                }
            }
        } else {
            // No temporary message - show current path
            // TODO: Enhance to show file count and selection count when available
            if let Ok(nav) = self.navigation.lock() {
                let current_path = nav.get_current_path();
                let path_str = current_path.to_string_lossy().to_string();
                // Only update if path actually changed to avoid unnecessary updates
                let current_status = self.status_text.get();
                if *current_status != path_str && !current_status.starts_with("Error:") && !current_status.contains("Created") && !current_status.contains("Deleted") {
                    self.status_text.set(path_str);
                }
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
            self.signals_hooked = true;
        }

        // Poll status messages from operations (these are temporary messages)
        if let Some(ref mut rx) = self.status_rx {
            while let Ok(msg) = rx.try_recv() {
                self.status_text.set(msg.clone());
                self.status_message_timeout = Some(std::time::Instant::now());
                update.insert(Update::DRAW);
            }
        }

        // Update status from navigation (shows current path when no temporary message)
        self.update_status_from_navigation();
        
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

pub fn build_window(_context: AppContext, state: AppState) -> impl Widget {
    let navigation = state.navigation.lock().unwrap();
    let initial_path = navigation.get_current_path();
    let nav_clone = state.navigation.clone();
    drop(navigation);

    // Create channels for operations and status
    let (operation_tx, operation_rx) = mpsc::unbounded_channel::<FileOperationRequest>();
    let (status_tx, status_rx) = mpsc::unbounded_channel::<String>();
    let (selected_paths_request_tx, selected_paths_request_rx) = mpsc::unbounded_channel::<()>();
    let (selected_paths_response_tx, selected_paths_response_rx) = mpsc::unbounded_channel::<Vec<PathBuf>>();

    // Create FilemanSidebar
    let mut sidebar = FilemanSidebar::new()
        .with_places(true)
        .with_bookmarks(true)
        .with_width(200.0);
    
    // Take the navigation receiver for FileListWrapper
    let sidebar_nav_rx = sidebar.take_navigation_receiver()
        .expect("FilemanSidebar should provide navigation receiver");

    // Create FileList wrapper that syncs with navigation state
    let file_list = FileListWrapper::new(
        initial_path.clone(),
        nav_clone.clone(),
        sidebar_nav_rx,
        operation_rx,
        status_tx.clone(),
        selected_paths_request_rx,
        selected_paths_response_tx.clone(),
    );

    // Create ToolbarWrapper
    let (mut toolbar_wrapper, toolbar_nav_tx) = crate::toolbar::ToolbarWrapper::new(
        nav_clone.clone(),
        operation_tx.clone(),
        selected_paths_request_tx,
        selected_paths_response_rx,
    );

    // Create LocationBarWrapper
    let location_bar = LocationBarWrapper::new(
        nav_clone.clone(),
        toolbar_nav_tx.clone(),
    );

    // Create StatusBarWrapper
    let statusbar = StatusBarWrapper::new(
        nav_clone.clone(),
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
            ..Default::default()
        })),
        // Content area (sidebar + file list)
        Box::new(Container::new(vec![
            Box::new(sidebar),
            Box::new(file_list),
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
