use nptk::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use async_trait::async_trait;
use nptk::core::signal::state::StateSignal;
use nptk::core::signal::MaybeSignal;
use nptk::widgets::breadcrumbs::{Breadcrumbs, BreadcrumbItem};
use nptk::widgets::text_input::TextInput;

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
}

impl FileLocationBar {
    pub fn new(current_path: StateSignal<PathBuf>) -> Self {
        let path_val = (*current_path.get()).clone();
        let initial_items = path_to_breadcrumb_items(&path_val);
        let breadcrumb_items = StateSignal::new(initial_items);
        let text_value = StateSignal::new(path_val.to_string_lossy().to_string());
        
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
            
        let container = Container::new(vec![
            Box::new(breadcrumbs),
            Box::new(text_input),
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
        }
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
            context.hook_signal(&mut self.current_path);
            context.hook_signal(&mut self.breadcrumb_items);
            context.hook_signal(&mut self.text_value);
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
