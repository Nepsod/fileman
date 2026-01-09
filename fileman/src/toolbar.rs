use nptk::prelude::*;
use nptk::core::signal::eval::EvalSignal;
use crate::app::AppState;
use crate::navigation::NavigationState;
use crate::window::FileOperationRequest;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;

// Toolbar types are re-exported from nptk prelude
// They're already available via `use nptk::prelude::*;`

/// Navigation actions that can be sent from toolbar buttons
#[derive(Debug, Clone)]
pub enum NavigationAction {
    Back,
    Forward,
    Up,
    Home,
    NavigateTo(PathBuf),
}

/// Wrapper widget for toolbar with navigation and file operation buttons
pub struct ToolbarWrapper {
    inner: Toolbar,
    navigation: Arc<Mutex<NavigationState>>,
    operation_tx: mpsc::UnboundedSender<FileOperationRequest>,
    navigation_tx: mpsc::UnboundedSender<NavigationAction>,
    navigation_rx: Option<mpsc::UnboundedReceiver<NavigationAction>>,
    // Channel to request selected paths from FileList
    selected_paths_request_tx: mpsc::UnboundedSender<()>,
    selected_paths_response_rx: Option<mpsc::UnboundedReceiver<Vec<PathBuf>>>,
    can_go_back: nptk::core::signal::state::StateSignal<bool>,
    can_go_forward: nptk::core::signal::state::StateSignal<bool>,
    has_selection: nptk::core::signal::state::StateSignal<bool>,
    signals_hooked: bool,
}

impl ToolbarWrapper {
    pub fn new(
        navigation: Arc<Mutex<NavigationState>>,
        operation_tx: mpsc::UnboundedSender<FileOperationRequest>,
        selected_paths_request_tx: mpsc::UnboundedSender<()>,
        selected_paths_response_rx: mpsc::UnboundedReceiver<Vec<PathBuf>>,
    ) -> (Self, mpsc::UnboundedSender<NavigationAction>) {
        let (nav_tx, nav_rx) = mpsc::unbounded_channel();
        use std::sync::atomic::{AtomicU8, Ordering};
        
        // Create buttons using EvalSignal to perform side effects when pressed
        // EvalSignal evaluates the closure every time get() is called (when button is pressed)
        let nav_clone1 = navigation.clone();
        let nav_clone2 = navigation.clone();
        let nav_clone3 = navigation.clone();
        let nav_clone4 = navigation.clone();
        let nav_clone5 = navigation.clone();
        let op_tx_clone = operation_tx.clone();
        
        let back_btn = ToolbarButton::new(Text::new("←".to_string()))
            .with_on_pressed(nptk::core::signal::MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                if let Ok(mut nav) = nav_clone1.lock() {
                    if nav.go_back().is_some() {
                        return Update::LAYOUT | Update::DRAW;
                    }
                }
                Update::empty()
            }))));

        let forward_btn = ToolbarButton::new(Text::new("→".to_string()))
            .with_on_pressed(nptk::core::signal::MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                if let Ok(mut nav) = nav_clone2.lock() {
                    if nav.go_forward().is_some() {
                        return Update::LAYOUT | Update::DRAW;
                    }
                }
                Update::empty()
            }))));

        let up_btn = ToolbarButton::new(Text::new("↑".to_string()))
            .with_on_pressed(nptk::core::signal::MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                if let Ok(mut nav) = nav_clone3.lock() {
                    if let Some(parent) = nav.parent_path() {
                        nav.navigate_to(parent);
                        return Update::LAYOUT | Update::DRAW;
                    }
                }
                Update::empty()
            }))));

        let home_btn = ToolbarButton::new(Text::new("Home".to_string()))
            .with_on_pressed(nptk::core::signal::MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                if let Ok(mut nav) = nav_clone4.lock() {
                    let home = std::env::var("HOME")
                        .ok()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("/home"));
                    nav.navigate_to(home);
                    return Update::LAYOUT | Update::DRAW;
                }
                Update::empty()
            }))));

        let new_folder_btn = ToolbarButton::new(Text::new("New Folder".to_string()))
            .with_on_pressed(nptk::core::signal::MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                if let Ok(nav) = nav_clone5.lock() {
                    let current = nav.get_current_path();
                    let name = format!("New Folder {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
                    let _ = op_tx_clone.send(FileOperationRequest::CreateDirectory {
                        parent: current,
                        name,
                    });
                    return Update::LAYOUT | Update::DRAW;
                }
                Update::empty()
            }))));

        // Delete button - request selected paths and delete them
        let delete_op_tx = operation_tx.clone();
        let sel_request_tx = selected_paths_request_tx.clone();
        let delete_btn = ToolbarButton::new(Text::new("Delete".to_string()))
            .with_on_pressed(nptk::core::signal::MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                // Request selected paths - FileListWrapper will respond via channel
                // Then we'll process the delete in update() when we receive the response
                let _ = sel_request_tx.send(());
                Update::DRAW
            }))));

        let toolbar = Toolbar::new()
            .with_child(back_btn)
            .with_child(forward_btn)
            .with_child(up_btn)
            .with_separator()
            .with_child(home_btn)
            .with_separator()
            .with_child(new_folder_btn)
            .with_child(delete_btn);

        let wrapper = Self {
            inner: toolbar,
            navigation,
            operation_tx: operation_tx.clone(),
            navigation_tx: nav_tx.clone(),
            navigation_rx: Some(nav_rx),
            selected_paths_request_tx,
            selected_paths_response_rx: Some(selected_paths_response_rx),
            can_go_back: nptk::core::signal::state::StateSignal::new(false),
            can_go_forward: nptk::core::signal::state::StateSignal::new(false),
            has_selection: nptk::core::signal::state::StateSignal::new(false),
            signals_hooked: false,
        };

        (wrapper, nav_tx)
    }

    /// Get the navigation action sender for external use (e.g., from location bar)
    pub fn navigation_tx(&self) -> &mpsc::UnboundedSender<NavigationAction> {
        &self.navigation_tx
    }

    /// Get the operation sender for external use
    pub fn operation_tx(&self) -> &mpsc::UnboundedSender<FileOperationRequest> {
        &self.operation_tx
    }

    pub fn take_navigation_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<NavigationAction>> {
        self.navigation_rx.take()
    }

    pub fn take_selection_response_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<Vec<PathBuf>>> {
        self.selected_paths_response_rx.take()
    }
}

impl Widget for ToolbarWrapper {
    fn widget_id(&self) -> nptk::theme::id::WidgetId {
        nptk::theme::id::WidgetId::new("fileman", "ToolbarWrapper")
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
            context.hook_signal(&mut self.can_go_back);
            context.hook_signal(&mut self.can_go_forward);
            context.hook_signal(&mut self.has_selection);
            self.signals_hooked = true;
        }

        // Process navigation actions from external sources (like location bar)
        // Note: Button presses are handled via the buttons' on_pressed callbacks
        // which directly update NavigationState. We process actions here for
        // programmatic navigation requests.
        if let Some(ref mut rx) = self.navigation_rx {
            while let Ok(action) = rx.try_recv() {
                if let Ok(mut nav) = self.navigation.lock() {
                    match action {
                        NavigationAction::Back => {
                            if nav.go_back().is_some() {
                                update.insert(Update::LAYOUT | Update::DRAW);
                            }
                        }
                        NavigationAction::Forward => {
                            if nav.go_forward().is_some() {
                                update.insert(Update::LAYOUT | Update::DRAW);
                            }
                        }
                        NavigationAction::Up => {
                            if let Some(parent) = nav.parent_path() {
                                nav.navigate_to(parent);
                                update.insert(Update::LAYOUT | Update::DRAW);
                            }
                        }
                        NavigationAction::Home => {
                            let home = std::env::var("HOME")
                                .ok()
                                .map(PathBuf::from)
                                .unwrap_or_else(|| PathBuf::from("/home"));
                            nav.navigate_to(home);
                            update.insert(Update::LAYOUT | Update::DRAW);
                        }
                        NavigationAction::NavigateTo(path) => {
                            nav.navigate_to(path);
                            update.insert(Update::LAYOUT | Update::DRAW);
                        }
                    }
                }
            }
        }
        
        // Handle delete button - process selected paths response and delete
        if let Some(ref mut rx) = self.selected_paths_response_rx {
            while let Ok(paths) = rx.try_recv() {
                if !paths.is_empty() {
                    // Send delete operation request
                    let _ = self.operation_tx.send(FileOperationRequest::Delete(paths));
                    update.insert(Update::LAYOUT | Update::DRAW);
                }
            }
        }

        // Update button states from navigation
        if let Ok(nav) = self.navigation.lock() {
            self.can_go_back.set(nav.can_go_back());
            self.can_go_forward.set(nav.can_go_forward());
        }

        // Update inner toolbar
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

impl nptk::core::widget::WidgetLayoutExt for ToolbarWrapper {
    fn set_layout_style(&mut self, layout_style: impl Into<nptk::core::signal::MaybeSignal<nptk::core::layout::LayoutStyle>>) {
        self.inner.set_layout_style(layout_style)
    }
}

// Legacy function - kept for compatibility but not used
pub fn build_toolbar(_context: &AppContext, _state: &AppState) -> impl Widget {
    Toolbar::new()
        .with_child(ToolbarButton::new(Text::new("←".to_string())))
        .with_child(ToolbarButton::new(Text::new("→".to_string())))
        .with_child(ToolbarButton::new(Text::new("↑".to_string())))
        .with_separator()
        .with_child(ToolbarButton::new(Text::new("Home".to_string())))
        .with_separator()
        .with_child(ToolbarButton::new(Text::new("New Folder".to_string())))
        .with_child(ToolbarButton::new(Text::new("Delete".to_string())))
}
