use nptk::prelude::*;
use std::path::PathBuf;
use tokio::sync::mpsc;
use async_trait::async_trait;
use nptk::core::signal::state::StateSignal;
use nptk::core::vg::kurbo::Shape;

/// A status bar widget that displays:
/// 1. Navigation info (path + selection count)
/// 2. Temporary status messages (with timeout)
/// 3. Hover status tips (from framework)
pub struct FileStatusBar {
    inner: Container,
    current_path: StateSignal<PathBuf>,
    selected_paths: StateSignal<Vec<PathBuf>>,
    status_text: StateSignal<String>,
    status_message_rx: Option<mpsc::UnboundedReceiver<String>>,
    status_message_timeout: Option<std::time::Instant>,
    signals_hooked: bool,
}

impl FileStatusBar {
    pub fn new(
        current_path: StateSignal<PathBuf>,
        selected_paths: StateSignal<Vec<PathBuf>>,
    ) -> Self {
        let status_text = StateSignal::new("Ready".to_string());
        let status_text_clone = status_text.clone();
        
        let container = Container::new(vec![
            Box::new(Text::new(status_text_clone.maybe()).with_font_size(14.0)),
        ])
        .with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::length(24.0)),
            padding: nptk::core::layout::Rect { 
                left: LengthPercentage::length(5.0), 
                right: LengthPercentage::length(5.0), 
                top: LengthPercentage::length(0.0), 
                bottom: LengthPercentage::length(0.0) 
            },
            align_items: Some(AlignItems::Center),
            ..Default::default()
        });

        Self {
            inner: container,
            current_path,
            selected_paths,
            status_text,
            status_message_rx: None,
            status_message_timeout: None,
            signals_hooked: false,
        }
    }

    pub fn with_message_receiver(mut self, rx: mpsc::UnboundedReceiver<String>) -> Self {
        self.status_message_rx = Some(rx);
        self
    }
    
    fn update_status_from_navigation(&mut self) {
         // Check if timeout expired for status messages
        if let Some(timeout) = self.status_message_timeout {
            if timeout.elapsed() > std::time::Duration::from_secs(3) {
                self.status_message_timeout = None;
                // Timeout expired, fall through to show normal status
            } else {
                return; // Timeout active, keep showing message
            }
        }
        
        // No temporary message - show current path (with selection count if applicable)
        let nav_path = (*self.current_path.get()).clone();
        let path_str = nav_path.to_string_lossy().to_string();
        let selection_count = (*self.selected_paths.get()).len();
        
        let status_msg = if selection_count > 0 {
            format!("{} - {} item(s) selected", path_str, selection_count)
        } else {
            path_str
        };
        
        // Only update if status actually changed to avoid unnecessary updates
        let current_status = (*self.status_text.get()).clone();
        if current_status != status_msg {
            self.status_text.set(status_msg);
        }
    }
}

#[async_trait(?Send)]
impl Widget for FileStatusBar {
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
            context.hook_signal(&mut self.status_text);
            context.hook_signal(&mut self.current_path);
            context.hook_signal(&mut self.selected_paths);
            self.signals_hooked = true;
        }

        // Poll status messages from operations (these are temporary messages)
        let mut has_active_temporary_message = false;
        if let Some(ref mut rx) = self.status_message_rx {
             while let Ok(msg) = rx.try_recv() {
                self.status_text.set(msg);
                self.status_message_timeout = Some(std::time::Instant::now());
                has_active_temporary_message = true;
                update.insert(Update::DRAW);
            }
        }
        
        // Check if we have an active temporary message (within timeout)
        if !has_active_temporary_message {
            if let Some(timeout) = self.status_message_timeout {
                if timeout.elapsed() <= std::time::Duration::from_secs(3) {
                    has_active_temporary_message = true;
                }
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
                // Check if status text actually changed to trigger draw? 
                // update_status_from_navigation sets signal, which triggers global update loop if hooked, 
                // but we might want to be explicit.
                 update.insert(Update::DRAW); // TODO: Optimize this
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
        // Draw background (optional, could be done via theme/properties)
        let palette = context.palette();
        let bg = palette.color(nptk::core::theme::ColorRole::Window);
        let border = palette.color(nptk::core::theme::ColorRole::ThreedShadow1);
        
        let rect = nptk::core::vg::kurbo::Rect::new(
            layout.layout.location.x as f64,
            layout.layout.location.y as f64,
            (layout.layout.location.x + layout.layout.size.width) as f64,
            (layout.layout.location.y + layout.layout.size.height) as f64,
        );
        
        graphics.fill(
            nptk::core::vg::peniko::Fill::NonZero,
            nptk::core::vg::kurbo::Affine::IDENTITY,
            &nptk::core::vg::peniko::Brush::Solid(bg),
            None,
            &rect.into_path(0.1)
        );
        
        // Top border
        let border_line = nptk::core::vg::kurbo::Line::new(
            (rect.x0, rect.y0),
            (rect.x1, rect.y0),
        );
         graphics.stroke(
            &nptk::core::vg::kurbo::Stroke::new(1.0),
            nptk::core::vg::kurbo::Affine::IDENTITY,
            &nptk::core::vg::peniko::Brush::Solid(border),
            None,
            &border_line.into_path(0.1)
        );
        
        self.inner.render(graphics, layout, info, context)
    }
}

impl nptk::core::widget::WidgetLayoutExt for FileStatusBar {
    fn set_layout_style(&mut self, layout_style: impl Into<nptk::core::signal::MaybeSignal<nptk::core::layout::LayoutStyle>>) {
        self.inner.set_layout_style(layout_style)
    }
}
