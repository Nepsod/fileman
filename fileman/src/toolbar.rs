use nptk::prelude::*;
use async_trait::async_trait;
use nptk::core::signal::eval::EvalSignal;
use crate::navigation::NavigationState;
use crate::window::FileOperationRequest;
use nptk_fileman_widgets::file_list::FileListViewMode;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;
use nptk::core::menu::MenuCommand;
use nptk_widgets_extra::menu_button::MenuButton;

// Helper to create menu items with unified type
use nptk::core::menu::unified::MenuItem;
use nptk::widgets::container::Container;
use nptk::core::layout::{FlexDirection, AlignItems, LayoutStyle, LengthPercentage, Dimension};

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

/// A signal that executes a function every time get() is called.
/// This is needed for button handlers that perform side effects, as EvalSignal memoizes the result.
#[derive(Clone)]
struct FuncSignal<F, T> {
    f: F,
    _marker: std::marker::PhantomData<T>,
}

impl<F, T> FuncSignal<F, T> {
    pub fn new(f: F) -> Self {
        Self {
            f,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<F, T> nptk::core::signal::Signal<T> for FuncSignal<F, T>
where
    F: Fn() -> T + Send + Sync + Clone + 'static,
    T: Send + Sync + 'static + Clone,
{
    fn get(&self) -> nptk::core::reference::Ref<'_, T> {
        nptk::core::reference::Ref::Owned((self.f)())
    }

    fn set_value(&self, _value: T) {}

    fn listen(&self, _listener: nptk::core::signal::Listener<T>) {}

    fn notify(&self) {}

    fn dyn_clone(&self) -> nptk::core::signal::BoxedSignal<T> {
        Box::new((*self).clone())
    }
}

/// Wrapper widget for toolbar with navigation and file operation buttons
pub struct ToolbarWrapper {
    inner: Toolbar,
    navigation: Arc<Mutex<NavigationState>>,
    operation_tx: mpsc::UnboundedSender<FileOperationRequest>,
    #[allow(dead_code)]
    navigation_tx: mpsc::UnboundedSender<NavigationAction>,
    navigation_rx: Option<mpsc::UnboundedReceiver<NavigationAction>>,
    // Reactive signals
    navigation_path_signal: nptk::core::signal::state::StateSignal<PathBuf>,
    selected_paths_signal: nptk::core::signal::state::StateSignal<Vec<PathBuf>>,
    can_go_back: nptk::core::signal::state::StateSignal<bool>,
    can_go_forward: nptk::core::signal::state::StateSignal<bool>,
    has_selection: nptk::core::signal::state::StateSignal<bool>,
    signals_hooked: bool,
    new_folder_requested: Arc<Mutex<bool>>,
    properties_requested: Arc<Mutex<bool>>,
    delete_requested: Arc<Mutex<bool>>,
    rename_requested: Arc<Mutex<bool>>,
    view_mode_signal: nptk::core::signal::state::StateSignal<FileListViewMode>,
}

impl ToolbarWrapper {
    pub fn new(
        navigation: Arc<Mutex<NavigationState>>,
        operation_tx: mpsc::UnboundedSender<FileOperationRequest>,
        navigation_path_signal: nptk::core::signal::state::StateSignal<PathBuf>,
        selected_paths_signal: nptk::core::signal::state::StateSignal<Vec<PathBuf>>,
        view_mode_signal: nptk::core::signal::state::StateSignal<FileListViewMode>,
    ) -> (Self, mpsc::UnboundedSender<NavigationAction>) {
        let (nav_tx, nav_rx) = mpsc::unbounded_channel();
        
        // Create buttons using EvalSignal to perform side effects when pressed
        // EvalSignal evaluates the closure every time get() is called (when button is pressed)
        
        let nav_tx_back = nav_tx.clone();
        let back_btn = ToolbarButton::with_children(vec![
            Box::new(Icon::new("arrow-left", 24, None)),
            Box::new(Text::new("Back".to_string()).with_font_size(14.0))
        ])
            .with_on_pressed(nptk::core::signal::MaybeSignal::signal(Box::new(FuncSignal::new(move || {
                let _ = nav_tx_back.send(NavigationAction::Back);
                Update::DRAW // UI update handled by receiver
            }))))
            .with_tooltip("Go back")
            .with_status_tip("Navigate to the previous directory in history");

        let nav_tx_fwd = nav_tx.clone();
        let forward_btn = ToolbarButton::with_children(vec![
            Box::new(Icon::new("arrow-right", 24, None)),
            Box::new(Text::new("Forward".to_string()).with_font_size(14.0))
        ])
            .with_on_pressed(nptk::core::signal::MaybeSignal::signal(Box::new(FuncSignal::new(move || {
                let _ = nav_tx_fwd.send(NavigationAction::Forward);
                Update::DRAW
            }))))
            .with_tooltip("Go forward")
            .with_status_tip("Navigate to the next directory in history");

        let nav_tx_up = nav_tx.clone();
        let up_btn = ToolbarButton::with_children(vec![
            Box::new(Icon::new("arrow-up", 24, None)),
            Box::new(Text::new("Up".to_string()).with_font_size(14.0))
        ])
            .with_on_pressed(nptk::core::signal::MaybeSignal::signal(Box::new(FuncSignal::new(move || {
                let _ = nav_tx_up.send(NavigationAction::Up);
                Update::DRAW
            }))))
            .with_status_tip("Navigate to the parent directory");

        let nav_tx_home = nav_tx.clone();
        let home_btn = ToolbarButton::with_children(vec![
            Box::new(Icon::new("folder-home", 24, None)),
            Box::new(Text::new("Home".to_string()).with_font_size(14.0))
        ])
            .with_on_pressed(nptk::core::signal::MaybeSignal::signal(Box::new(FuncSignal::new(move || {
                let _ = nav_tx_home.send(NavigationAction::Home);
                Update::DRAW
            }))))
            .with_tooltip("Go home")
            .with_status_tip("Navigate to the home directory");

        let new_folder_requested = Arc::new(Mutex::new(false));
        let new_folder_btn = ToolbarButton::with_children(vec![
            Box::new(Icon::new("folder-new", 24, None)),
            Box::new(Text::new("New Folder".to_string()).with_font_size(14.0))
        ])
            .with_on_pressed({
                let new_folder_flag = new_folder_requested.clone();
                nptk::core::signal::MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                    if let Ok(mut flag) = new_folder_flag.lock() {
                        *flag = true;
                    }
                    Update::DRAW
                })))
            })
            .with_tooltip("New Folder")
            .with_status_tip("Create a new folder");

        let properties_requested = Arc::new(Mutex::new(false));
        let properties_btn = ToolbarButton::with_children(vec![
            Box::new(Icon::new("document-properties", 24, None)),
            Box::new(Text::new("Properties".to_string()).with_font_size(14.0))
        ])
            .with_on_pressed({
                let properties_flag = properties_requested.clone();
                nptk::core::signal::MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                    if let Ok(mut flag) = properties_flag.lock() {
                        *flag = true;
                    }
                    Update::DRAW
                })))
            })
            .with_tooltip("Properties")
            .with_status_tip("Show properties of the selected items");

        // Rename button
        let rename_requested = Arc::new(Mutex::new(false));
        let rename_btn = ToolbarButton::with_children(vec![
            Box::new(Icon::new("edit-rename", 24, None)),
            Box::new(Text::new("Rename".to_string()).with_font_size(14.0))
        ])
            .with_on_pressed({
                let rename_flag = rename_requested.clone();
                nptk::core::signal::MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                    if let Ok(mut flag) = rename_flag.lock() {
                        *flag = true;
                    }
                    Update::DRAW
                })))
            })
            .with_tooltip("Rename")
            .with_status_tip("Rename selected item");

        // Delete button - read selected paths from signal directly
        let delete_requested = Arc::new(Mutex::new(false));
        let delete_requested_clone = delete_requested.clone();
        let delete_btn = ToolbarButton::with_children(vec![
            Box::new(Icon::new("delete", 24, None)),
            Box::new(Text::new("Delete".to_string()).with_font_size(14.0))
            ])
            .with_on_pressed({
                let delete_flag = delete_requested_clone.clone();
                // Use FuncSignal because we need side effects every time
                nptk::core::signal::MaybeSignal::signal(Box::new(FuncSignal::new(move || {
                    if let Ok(mut f) = delete_flag.lock() {
                        *f = true;
                    }
                    Update::DRAW
                })))
            })
            .with_tooltip("Delete")
            .with_status_tip("Delete the selected items");

        let view_btn = MenuButton::with_child(
            Container::new(vec![
                Box::new(Icon::new("view-list-details", 24, None)),
                Box::new(Text::new("View".to_string()).with_font_size(14.0))
            ])
            .with_layout_style(LayoutStyle {
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: nalgebra::Vector2::new(LengthPercentage::length(4.0), LengthPercentage::length(0.0)),
                ..Default::default()
            })
        )
        .with_items_builder({
            let view_mode_signal = view_mode_signal.clone();
            move || {
                let current_mode = *view_mode_signal.get();
                let signal = view_mode_signal.clone();
                vec![
                    MenuItem::new(MenuCommand::ViewIcon, "Icon View")
                        .with_checked(current_mode == FileListViewMode::Icon)
                        .with_action({
                            let signal = signal.clone();
                            move || { signal.set(FileListViewMode::Icon); Update::DRAW }
                        }),
                    MenuItem::new(MenuCommand::ViewList, "List View")
                        .with_checked(current_mode == FileListViewMode::List)
                        .with_action({
                            let signal = signal.clone();
                            move || { signal.set(FileListViewMode::List); Update::DRAW }
                        }),
                    MenuItem::new(MenuCommand::ViewSmallIcon, "Compact View")
                        .with_checked(current_mode == FileListViewMode::Compact)
                        .with_action({
                            let signal = signal.clone();
                            move || { signal.set(FileListViewMode::Compact); Update::DRAW }
                        }),
                    MenuItem::new(MenuCommand::ViewDetails, "Table View")
                        .with_checked(current_mode == FileListViewMode::Table)
                        .with_action({
                            let signal = signal.clone();
                            move || { signal.set(FileListViewMode::Table); Update::DRAW }
                        }),
                ]
            }
        })
        .with_tooltip("Change View");
        // .with_status_tip("Switch between List, Icon, and Details views"); // MenuButton doesn't support status tip yet

        let toolbar = Toolbar::new()
            .with_child(back_btn)
            .with_child(forward_btn)
            .with_child(up_btn)
            .with_separator()
            .with_child(home_btn)
            .with_separator()
            .with_child(new_folder_btn)
            .with_child(delete_btn)
            .with_separator()
            .with_child(rename_btn)
            .with_child(properties_btn)
            .with_separator()
            .with_child(view_btn);

        let wrapper = Self {
            inner: toolbar,
            navigation,
            operation_tx: operation_tx.clone(),
            navigation_tx: nav_tx.clone(),
            navigation_rx: Some(nav_rx),
            navigation_path_signal,
            selected_paths_signal,
            can_go_back: nptk::core::signal::state::StateSignal::new(false),
            can_go_forward: nptk::core::signal::state::StateSignal::new(false),
            has_selection: nptk::core::signal::state::StateSignal::new(false),
            signals_hooked: false,
            new_folder_requested,
            properties_requested,
            delete_requested,
            rename_requested,
            view_mode_signal,
        };

        (wrapper, nav_tx)
    }

    /// Get the navigation action sender for external use (e.g., from location bar)
    #[allow(dead_code)]
    pub fn navigation_tx(&self) -> &mpsc::UnboundedSender<NavigationAction> {
        &self.navigation_tx
    }

    /// Get the operation sender for external use
    #[allow(dead_code)]
    pub fn operation_tx(&self) -> &mpsc::UnboundedSender<FileOperationRequest> {
        &self.operation_tx
    }

    #[allow(dead_code)]
    pub fn take_navigation_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<NavigationAction>> {
        self.navigation_rx.take()
    }

}

#[async_trait(?Send)]
impl Widget for ToolbarWrapper {

    fn layout_style(&self, _context: &nptk::core::layout::LayoutContext) -> nptk::core::layout::StyleNode {
        self.inner.layout_style(_context)
    }

    async fn update(
        &mut self,
        layout: &nptk::core::layout::LayoutNode,
        context: nptk::core::app::context::AppContext,
        info: &mut nptk::core::app::info::AppInfo,
    ) -> nptk::core::app::update::Update {
        let mut update = Update::empty();

        // Hook signals on first update
        if !self.signals_hooked {
            context.hook_signal(&self.can_go_back);
            context.hook_signal(&self.can_go_forward);
            context.hook_signal(&self.has_selection);
            context.hook_signal(&self.navigation_path_signal);
            context.hook_signal(&self.selected_paths_signal);
            context.hook_signal(&self.view_mode_signal);
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
        
        // Handle new folder button press
        if let Ok(mut flag) = self.new_folder_requested.lock() {
            if *flag {
                *flag = false;
                let current = (*self.navigation_path_signal.get()).clone();
                // Use PromptCreateDirectory instead of hardcoded logic
                let _ = self.operation_tx.send(FileOperationRequest::PromptCreateDirectory(current));
                update.insert(Update::LAYOUT | Update::DRAW);
            }
        }

        // Handle properties button press - read selected paths from signal
        if let Ok(mut flag) = self.properties_requested.lock() {
            if *flag {
                *flag = false;
                let selected_paths = (*self.selected_paths_signal.get()).clone();
                if !selected_paths.is_empty() {
                    let _ = self.operation_tx.send(FileOperationRequest::Properties(selected_paths));
                    update.insert(Update::LAYOUT | Update::DRAW);
                }
            }
        }

        // Handle rename button press
        if let Ok(mut flag) = self.rename_requested.lock() {
            if *flag {
                *flag = false;
                let selected_paths = (*self.selected_paths_signal.get()).clone();
                if selected_paths.len() == 1 {
                    let _ = self.operation_tx.send(FileOperationRequest::PromptRename(selected_paths[0].clone()));
                    update.insert(Update::LAYOUT | Update::DRAW);
                }
            }
        }

        // Handle delete button - read selected paths from signal directly
        if let Ok(mut flag) = self.delete_requested.lock() {
            if *flag {
                *flag = false;
                let selected_paths = (*self.selected_paths_signal.get()).clone();
                if !selected_paths.is_empty() {
                    let _ = self.operation_tx.send(FileOperationRequest::Delete(selected_paths));
                    update.insert(Update::LAYOUT | Update::DRAW);
                }
            }
        }



        // Update button states reactively from navigation
        if let Ok(nav) = self.navigation.lock() {
            self.can_go_back.set(nav.can_go_back());
            self.can_go_forward.set(nav.can_go_forward());
        }

        // Update has_selection signal reactively from selected_paths_signal
        let selected_paths = (*self.selected_paths_signal.get()).clone();
        self.has_selection.set(!selected_paths.is_empty());

        // Update inner toolbar
        update |= self.inner.update(layout, context, info).await;
        update
    }

    fn render(
        &mut self,
        graphics: &mut dyn nptk::core::vgi::Graphics,
        layout: &nptk::core::layout::LayoutNode,
        info: &mut nptk::core::app::info::AppInfo,
        context: nptk::core::app::context::AppContext,
    ) {
        self.inner.render(graphics, layout, info, context)
    }

    fn render_postfix(
        &mut self,
        graphics: &mut dyn nptk::core::vgi::Graphics,
        layout: &nptk::core::layout::LayoutNode,
        info: &mut nptk::core::app::info::AppInfo,
        context: nptk::core::app::context::AppContext,
    ) {
        self.inner.render_postfix(graphics, layout, info, context)
    }
}

impl nptk::core::widget::WidgetLayoutExt for ToolbarWrapper {
    fn set_layout_style(&mut self, layout_style: impl Into<nptk::core::signal::MaybeSignal<nptk::core::layout::LayoutStyle>>) {
        self.inner.set_layout_style(layout_style)
    }
}
