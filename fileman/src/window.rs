use nptk::prelude::*;
use nptk_fileman_widgets::file_list::FileList;
use nptk_fileman_widgets::FilemanSidebar;
use crate::app::AppState;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;

/// Wrapper widget that manages FileList and connects it to navigation state
struct FileListWrapper {
    file_list: FileList,
    navigation: Arc<Mutex<crate::navigation::NavigationState>>,
    last_path: PathBuf,
    navigation_rx: Option<mpsc::UnboundedReceiver<PathBuf>>,
}

impl FileListWrapper {
    fn new(
        initial_path: PathBuf,
        navigation: Arc<Mutex<crate::navigation::NavigationState>>,
        navigation_rx: mpsc::UnboundedReceiver<PathBuf>,
    ) -> Self {
        Self {
            file_list: FileList::new(initial_path.clone()),
            navigation,
            last_path: initial_path,
            navigation_rx: Some(navigation_rx),
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

        // Poll navigation events from sidebar
        if let Some(ref mut rx) = self.navigation_rx {
            // Use try_recv to poll non-blockingly
            while let Ok(path) = rx.try_recv() {
                if let Ok(mut nav) = self.navigation.lock() {
                    nav.navigate_to(path.clone());
                    self.file_list.set_path(path.clone());
                    self.last_path = path;
                    update.insert(Update::LAYOUT | Update::DRAW);
                }
            }
        }

        // Check if navigation path has changed and update FileList if needed
        if let Ok(nav) = self.navigation.lock() {
            let current_nav_path = nav.get_current_path();
            if current_nav_path != self.last_path {
                self.file_list.set_path(current_nav_path.clone());
                self.last_path = current_nav_path;
                update.insert(Update::LAYOUT | Update::DRAW);
            }
        }
        
        // Update the wrapped FileList
        update |= self.file_list.update(layout, context, info);
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

pub fn build_window(_context: AppContext, state: AppState) -> impl Widget {
    let navigation = state.navigation.lock().unwrap();
    let initial_path = navigation.get_current_path();
    let nav_clone = state.navigation.clone();
    drop(navigation);

    // Create FilemanSidebar
    let mut sidebar = FilemanSidebar::new()
        .with_places(true)
        .with_bookmarks(true)
        .with_width(200.0);
    
    // Take the navigation receiver for FileListWrapper
    let nav_rx = sidebar.take_navigation_receiver()
        .expect("FilemanSidebar should provide navigation receiver");

    // Create FileList wrapper that syncs with navigation state
    let file_list = FileListWrapper::new(initial_path, nav_clone, nav_rx);

    // Create toolbar with navigation buttons
    // TODO: Wire up button callbacks properly - for now just placeholder buttons
    let toolbar = Toolbar::new()
        .with_child(ToolbarButton::new(Text::new("←".to_string())))
        .with_child(ToolbarButton::new(Text::new("→".to_string())))
        .with_child(ToolbarButton::new(Text::new("↑".to_string())))
        .with_separator()
        .with_child(ToolbarButton::new(Text::new("Home".to_string())));

    // Create location bar (simple TextInput) - placeholder for now
    let location_bar = TextInput::new()
        .with_placeholder("Location...".to_string());

    // Create statusbar (simple container with text)
    let status_text = StateSignal::new("Ready".to_string());
    let statusbar = Container::new(vec![
        Box::new(Text::new(status_text.maybe())),
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

    // Build main layout
    Container::new(vec![
        // Toolbar area
        Box::new(Container::new(vec![
            Box::new(toolbar),
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
