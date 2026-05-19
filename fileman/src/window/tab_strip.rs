use async_trait::async_trait;
use nptk::prelude::*;
use nptk_widgets_extra::tabs_container::{TabItem, TabPosition, TabsContainer};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use super::file_operation::FileOperationRequest;
use super::tabs::TabModel;

fn tab_label(path: &PathBuf, index: usize) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("Tab {}", index + 1))
}

pub struct TabStrip {
    tabs: Arc<Mutex<TabModel>>,
    operation_tx: mpsc::UnboundedSender<FileOperationRequest>,
    inner: TabsContainer,
    last_signature: String,
    last_active_sent: Option<usize>,
}

impl TabStrip {
    fn fixed_layout_style() -> LayoutStyle {
        LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::length(32.0)),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            ..Default::default()
        }
    }

    pub fn new(
        tabs: Arc<Mutex<TabModel>>,
        operation_tx: mpsc::UnboundedSender<FileOperationRequest>,
    ) -> Self {
        Self {
            tabs,
            operation_tx,
            inner: TabsContainer::new()
                .with_position(TabPosition::Top)
                .with_tab_size(32.0)
                .with_layout_style(Self::fixed_layout_style()),
            last_signature: String::new(),
            last_active_sent: None,
        }
    }

    fn snapshot_signature(tab_model: &TabModel) -> String {
        let mut s = format!("{}|", tab_model.active);
        for p in &tab_model.paths {
            s.push_str(&p.to_string_lossy());
            s.push('|');
        }
        s
    }

    fn rebuild_if_needed(&mut self) {
        let snapshot = match self.tabs.lock() {
            Ok(t) => t.clone(),
            Err(_) => return,
        };
        let signature = Self::snapshot_signature(&snapshot);
        if signature == self.last_signature {
            return;
        }

        let mut tabs_container = TabsContainer::new()
            .with_position(TabPosition::Top)
            .with_tab_size(32.0)
            .with_history(true, 16)
            .with_layout_style(Self::fixed_layout_style());
        for (idx, path) in snapshot.paths.iter().enumerate() {
            let close_tx = self.operation_tx.clone();
            let title = tab_label(path, idx);
            tabs_container = tabs_container.with_tab(
                TabItem::new(
                    format!("tab-{}", idx),
                    title,
                    Container::new(vec![]).with_layout_style(LayoutStyle {
                        size: Vector2::new(Dimension::percent(1.0), Dimension::length(0.0)),
                        ..Default::default()
                    }),
                )
                .with_close_callback(move || {
                    let _ = close_tx.send(FileOperationRequest::CloseTabAt(idx));
                    Update::DRAW
                }),
            );
        }

        let new_tx = self.operation_tx.clone();
        tabs_container = tabs_container.with_action_button(move || {
            let _ = new_tx.send(FileOperationRequest::NewTab);
            Update::DRAW
        });
        tabs_container.set_active_tab(snapshot.active);

        self.inner = tabs_container;
        self.last_signature = signature;
    }
}

#[async_trait(?Send)]
impl Widget for TabStrip {
    fn layout_style(&self, context: &nptk::core::layout::LayoutContext) -> nptk::core::layout::StyleNode {
        self.inner.layout_style(context)
    }

    async fn update(
        &mut self,
        layout: &nptk::core::layout::LayoutNode,
        context: AppContext,
        info: &mut nptk::core::app::info::AppInfo,
    ) -> Update {
        self.rebuild_if_needed();
        let update = self.inner.update(layout, context, info).await;
        let active = self.inner.active_tab();
        if self.last_active_sent != Some(active) {
            self.last_active_sent = Some(active);
            let _ = self.operation_tx.send(FileOperationRequest::SwitchTab(active));
        }
        update
    }

    fn render(
        &mut self,
        graphics: &mut dyn nptk::core::vgi::Graphics,
        layout: &nptk::core::layout::LayoutNode,
        info: &mut nptk::core::app::info::AppInfo,
        context: AppContext,
    ) {
        self.inner.render(graphics, layout, info, context);
    }
}
