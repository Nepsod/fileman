use nptk::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use async_trait::async_trait;
use nptk::core::signal::state::StateSignal;
use nptk::widgets::breadcrumbs::{Breadcrumbs, BreadcrumbItem};
use nptk::widgets::text_input::TextInput;
use nptk::core::app::focus::FocusId;

/// Helper function to convert PathBuf to breadcrumb items
fn path_to_breadcrumb_items(path: &std::path::Path) -> Vec<BreadcrumbItem> {
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

/// A reusable location bar widget combining breadcrumbs and text input.
pub struct FileLocationBar {
    inner: Container,
    current_path: StateSignal<PathBuf>,
    breadcrumb_items: StateSignal<Vec<BreadcrumbItem>>,
    text_value: StateSignal<String>,
    last_synced_path: PathBuf,
    on_navigate: Option<Box<dyn Fn(PathBuf) -> Update + Send + Sync>>,
    signals_hooked: bool,
    internal_rx: Option<mpsc::UnboundedReceiver<PathBuf>>,
    focus_rx: Option<mpsc::UnboundedReceiver<()>>,
    text_input_focus_id: FocusId,
    // Search
    search_query: StateSignal<String>,
    search_active: StateSignal<bool>,
    search_input_focus_id: Option<FocusId>,
}

impl FileLocationBar {
    pub fn new(current_path: StateSignal<PathBuf>) -> Self {
        let path_val = (*current_path.get()).clone();
        let initial_items = path_to_breadcrumb_items(&path_val);
        let breadcrumb_items = StateSignal::new(initial_items);
        let text_value = StateSignal::new(path_val.to_string_lossy().to_string());
        
        let search_query = StateSignal::new(String::new());
        let search_active = StateSignal::new(false);
        
        let (tx, rx) = mpsc::unbounded_channel();
        let tx = Arc::new(tx);
        
        // Breadcrumbs
        let tx_crumb = tx.clone();
        let breadcrumbs = Breadcrumbs::new()
            .with_items_signal(breadcrumb_items.clone())
            .with_on_click(move |item| {
                if let Some(id) = &item.id {
                    let path = PathBuf::from(id);
                     let _ = tx_crumb.send(path);
                     return Update::DRAW;
                }
                Update::empty()
            })
            .with_layout_style(LayoutStyle {
                size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
                ..Default::default()
            });

        // Text Input
        let text_input = TextInput::new()
            .with_text_signal(text_value.clone())
            .with_placeholder("Path...".to_string())
            .with_layout_style(LayoutStyle {
                size: Vector2::new(Dimension::auto(), Dimension::length(30.0)),
                flex_grow: 1.0, 
                min_size: Vector2::new(Dimension::length(200.0), Dimension::auto()),
                ..Default::default()
            });
        
        let focus_id = text_input.focus_id();

        // Search UI
        let search_query_clone = search_query.clone();
        // let search_active_clone = search_active.clone(); // Removed unused clone

        // Search Input
        let search_input_style = search_active.map(|active| {
            let display = if *active { Display::Flex } else { Display::None };
            nptk::prelude::Ref::Owned(LayoutStyle {
                size: Vector2::new(Dimension::length(200.0), Dimension::length(30.0)),
                display,
                ..Default::default()
            })
        });

        let search_input = TextInput::new()
            .with_text_signal(search_query.clone())
            .with_placeholder("Search...".to_string())
            .with_layout_style(MaybeSignal::signal(Box::new(search_input_style)));

        let search_input_focus_id = Some(search_input.focus_id());

        // Search Toggle Button
        use nptk::widgets::button::Button;
        use nptk::widgets::text::Text;
        
        // Simple text button for now, should be icon later
        let search_toggle = Button::new(
             // Use conditional text or icon
             Text::new(MaybeSignal::signal(Box::new(search_active.map(|active| {
                 nptk::prelude::Ref::Owned(if *active { "Cancel" } else { "Search" }.to_string())
             }))))
        )
        .with_on_pressed({
            let active = search_active.clone();
            let query = search_query_clone.clone();
            MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                let new_state = !*active.get();
                active.set(new_state);
                if !new_state {
                    query.set(String::new()); // Clear query when closing
                }
                Update::LAYOUT | Update::DRAW
            })))
        })
        .with_layout_style(LayoutStyle {
             margin: nptk::core::layout::Rect {
                 left: LengthPercentageAuto::length(4.0),
                 right: LengthPercentageAuto::length(0.0),
                 top: LengthPercentageAuto::length(0.0),
                 bottom: LengthPercentageAuto::length(0.0),
             },
             ..Default::default()
        });
            
        let container = Container::new(vec![
            Box::new(breadcrumbs),
            Box::new(text_input),
            Box::new(search_input),
            Box::new(search_toggle),
        ]).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
            flex_direction: FlexDirection::Row,
            gap: Vector2::new(LengthPercentage::length(8.0), LengthPercentage::length(0.0)),
            align_items: Some(AlignItems::Center),
            ..Default::default()
        });
        
        Self {
            inner: container,
            current_path,
            breadcrumb_items,
            text_value,
            last_synced_path: path_val,
            on_navigate: None,
            signals_hooked: false,
            internal_rx: Some(rx),
            focus_rx: None,
            text_input_focus_id: focus_id,
            search_query,
            search_active,
            search_input_focus_id,
        }
    }
    
    /// Get the search query signal
    pub fn search_query_signal(&self) -> &StateSignal<String> {
        &self.search_query
    }

    /// Get the search active signal
    pub fn search_active_signal(&self) -> &StateSignal<bool> {
        &self.search_active
    }

    /// Get the search input focus ID
    pub fn search_input_focus_id(&self) -> Option<FocusId> {
        self.search_input_focus_id
    }

    /// Set search query signal (builder pattern)
    pub fn with_search_query_signal(mut self, signal: StateSignal<String>) -> Self {
        self.search_query = signal;
        self
    }
    
    pub fn with_focus_receiver(mut self, rx: mpsc::UnboundedReceiver<()>) -> Self {
        self.focus_rx = Some(rx);
        self
    }
    
    
    pub fn with_on_navigate<F>(mut self, callback: F) -> Self
    where
        F: Fn(PathBuf) -> Update + Send + Sync + 'static,
    {
        self.on_navigate = Some(Box::new(callback));
        self
    }
}

#[async_trait(?Send)]
impl Widget for FileLocationBar {
    fn layout_style(&self, context: &nptk::core::layout::LayoutContext) -> nptk::core::layout::StyleNode {
        self.inner.layout_style(context)
    }

    async fn update(
        &mut self,
        layout: &nptk::core::layout::LayoutNode,
        context: nptk::core::app::context::AppContext,
        info: &mut nptk::core::app::info::AppInfo,
    ) -> nptk::core::app::update::Update {
        let mut update = Update::empty();
        
        if !self.signals_hooked {
            context.hook_signal(&self.current_path);
            context.hook_signal(&self.breadcrumb_items);
            context.hook_signal(&self.text_value);
            self.signals_hooked = true;
        }
        
        // Sync path changes to UI
        let path = (*self.current_path.get()).clone();
        if path != self.last_synced_path {
            self.last_synced_path = path.clone();
            
            // Update breadcrumbs
            let new_items = path_to_breadcrumb_items(&path);
            self.breadcrumb_items.set(new_items);
            
            // Update text
            self.text_value.set(path.to_string_lossy().to_string());
            
            update.insert(Update::LAYOUT | Update::DRAW);
        }
        
        // Handle internal navigation events
        if let Some(ref mut rx) = self.internal_rx {
            while let Ok(path) = rx.try_recv() {
                if let Some(callback) = &self.on_navigate {
                    update |= callback(path);
                }
            }
        }
        
        // Handle focus requests
        // Extract ID to avoid borrowing self mutably during loop if we used a method
        let focus_id = self.text_input_focus_id;
        
        if let Some(ref mut rx) = self.focus_rx {
             while let Ok(_) = rx.try_recv() {
                 context.set_focus(Some(focus_id));
                 update |= Update::DRAW;
             }
        }
        
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
}

impl nptk::core::widget::WidgetLayoutExt for FileLocationBar {
    fn set_layout_style(&mut self, layout_style: impl Into<nptk::core::signal::MaybeSignal<nptk::core::layout::LayoutStyle>>) {
        self.inner.set_layout_style(layout_style)
    }
}
