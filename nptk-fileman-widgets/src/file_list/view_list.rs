use super::FileListContent;
use nptk::widgets::file_icon::renderer::{render_cached_icon, render_fallback_icon};
use nptk::core::app::info::AppInfo;
use nptk::core::layout::LayoutNode;
use nptk::core::signal::Signal;
use nptk::core::vg::kurbo::{Affine, Rect, Shape};
use nptk::core::vg::peniko::{Brush, Color, Fill};
use nptk::core::vgi::Graphics;
use nptk::core::widget::Widget;
use nptk::services::thumbnail::npio_adapter::{file_entry_to_uri, u32_to_thumbnail_size};
use nptk::theme::theme::Theme;
use npio::get_file_for_uri;
use std::collections::HashSet;
use std::path::PathBuf;

impl FileListContent {
    pub(super) fn render_list_view(
        &mut self,
        graphics: &mut dyn Graphics,
        theme: &mut dyn Theme,
        layout: &LayoutNode,
        info: &mut AppInfo,
    ) {
        let entries = self.entries.get();
        let selected_paths = self.selected_paths.get();
        let selected_set: HashSet<&PathBuf> = selected_paths.iter().collect();
        let entry_count = entries.len();

        // Draw background
        let bg_rect = Rect::new(
            layout.layout.location.x as f64,
            layout.layout.location.y as f64,
            (layout.layout.location.x + layout.layout.size.width) as f64,
            (layout.layout.location.y + layout.layout.size.height) as f64,
        );

        let bg_color = theme
            .get_property(
                self.widget_id(),
                &nptk::theme::properties::ThemeProperty::ColorBackground,
            )
            .or_else(|| {
                theme.get_default_property(&nptk::theme::properties::ThemeProperty::ColorBackground)
            })
            .unwrap_or_else(|| theme.window_background());

        graphics.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            &Brush::Solid(bg_color),
            None,
            &bg_rect.to_path(0.1),
        );

        if entry_count == 0 {
            return;
        }

        // VIEWPORT CULLING: Calculate visible range
        // VIEWPORT CULLING: Calculate visible range relative to window
        let viewport_start_y = (-layout.layout.location.y).max(0.0);
        let viewport_end_y = info.size.y as f32 - layout.layout.location.y;

        let start_index = (viewport_start_y / self.item_height).floor().max(0.0) as usize;
        let end_index = ((viewport_end_y / self.item_height).ceil() as usize + 1).min(entry_count);

        // Only render visible items
        for i in start_index..end_index {
            let entry = &entries[i];
            let y = layout.layout.location.y + i as f32 * self.item_height;
            let row_rect = Rect::new(
                layout.layout.location.x as f64,
                y as f64,
                (layout.layout.location.x + layout.layout.size.width) as f64,
                (y + self.item_height) as f64,
            );

            // Check for hover state
            let is_hovered = if let Some(cursor) = info.cursor_pos {
                let cursor_x = cursor.x as f64;
                let cursor_y = cursor.y as f64;
                cursor_x >= row_rect.x0
                    && cursor_x < row_rect.x1
                    && cursor_y >= row_rect.y0
                    && cursor_y < row_rect.y1
            } else {
                false
            };

            // Draw hover background (if not selected)
            if is_hovered && !selected_set.contains(&entry.path) {
                let hover_color = theme
                    .get_property(
                        self.widget_id(),
                        &nptk::theme::properties::ThemeProperty::ColorMenuHovered,
                    )
                    .or_else(|| {
                        theme.get_default_property(
                            &nptk::theme::properties::ThemeProperty::ColorMenuHovered,
                        )
                    })
                    .unwrap_or_else(|| Color::from_rgb8(240, 240, 240));

                graphics.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    &Brush::Solid(hover_color),
                    None,
                    &row_rect.to_path(0.1),
                );
            }

            // Draw selection background
            if selected_set.contains(&entry.path) {
                let color = theme
                    .get_property(
                        self.widget_id(),
                        &nptk::theme::properties::ThemeProperty::ColorBackgroundSelected,
                    )
                    .or_else(|| {
                        theme.get_default_property(
                            &nptk::theme::properties::ThemeProperty::ColorBackgroundSelected,
                        )
                    })
                    .unwrap_or_else(|| Color::from_rgb8(100, 150, 255));

                graphics.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    &Brush::Solid(color.with_alpha(0.3)),
                    None,
                    &row_rect.to_path(0.1),
                );
            }

            // Try to get thumbnail first, fall back to icon (view_list uses icons, not thumbnails)
            let icon_size = 20.0;
            let icon_rect = Rect::new(
                row_rect.x0 + 5.0,
                row_rect.y0 + 5.0,
                row_rect.x0 + 25.0,
                row_rect.y1 - 5.0,
            );

            // Request thumbnail generation asynchronously (non-blocking)
            // Thumbnails will be rendered when ready via event system
            if entry.is_file() {
                let mut pending = self.pending_thumbnails.lock().unwrap();
                // Use insert() which returns true if the value was newly inserted (atomic check-and-insert)
                if pending.insert(entry.path.clone()) {
                    let file_clone = get_file_for_uri(&file_entry_to_uri(entry)).ok();
                    let service_clone = self.thumbnail_service.clone();
                    let size = u32_to_thumbnail_size(self.thumbnail_size);
                    
                    // Spawn async task to generate thumbnail (non-blocking)
                    let semaphore_clone = self.async_task_semaphore.clone();
                    tokio::spawn(async move {
                        // Acquire semaphore permit to limit concurrent tasks
                        let _permit = semaphore_clone.acquire().await.ok();
                        if let Some(f) = file_clone {
                            let _ = service_clone
                                .get_or_generate_thumbnail(&*f, size, None)
                                .await;
                        }
                        // Permit is automatically released when dropped
                    });
                }
            }

            // Get icon for this entry (only use cached, don't block on loading)
            let cache_key = (entry.path.clone(), icon_size as u32);
            let cached_icon = {
                let cache = self.icon_cache.lock().unwrap();
                cache.get(&cache_key).and_then(|opt| opt.clone())
            };
            
            // If icon not cached, request it asynchronously (non-blocking)
            if cached_icon.is_none() {
                let cache_clone = self.icon_cache.clone();
                let registry_clone = self.icon_registry.clone();
                let entry_clone = entry.clone();
                let cache_key_clone = cache_key.clone();
                let cache_update_tx_clone = self.cache_update_tx.clone();
                let semaphore_clone = self.async_task_semaphore.clone();
                tokio::spawn(async move {
                    // Acquire semaphore permit to limit concurrent tasks
                    let _permit = semaphore_clone.acquire().await.ok();
                    let uri = file_entry_to_uri(&entry_clone);
                    if let Ok(file) = get_file_for_uri(&uri) {
                        let icon = registry_clone.get_file_icon(&*file, icon_size as u32).await;
                        let mut cache = cache_clone.lock().unwrap();
                        cache.insert(cache_key_clone, icon);
                        // Notify that cache was updated to trigger redraw
                        let _ = cache_update_tx_clone.send(());
                    }
                    // Permit is automatically released when dropped
                });
            }

            if let Some(icon) = cached_icon {
                render_cached_icon(
                    graphics,
                    theme,
                    self.widget_id(),
                    icon,
                    icon_rect,
                    &entry,
                    &mut self.svg_scene_cache,
                );
            } else {
                render_fallback_icon(
                    graphics,
                    theme,
                    self.widget_id(),
                    icon_rect,
                    &entry,
                );
            }

            // Draw text
            let text_color = theme
                .get_property(
                    self.widget_id(),
                    &nptk::theme::properties::ThemeProperty::ColorText,
                )
                .or_else(|| {
                    theme.get_default_property(&nptk::theme::properties::ThemeProperty::ColorText)
                })
                .unwrap_or(Color::BLACK);

            let transform = Affine::translate((row_rect.x0 + 35.0, row_rect.y0 + 5.0));

            self.text_render_context.render_text(
                &mut info.font_context,
                graphics,
                &entry.name,
                None,
                16.0,
                Brush::Solid(text_color),
                transform,
                true,
                Some(row_rect.width() as f32 - 40.0),
            );
        }

        // DEBUG: Log timing every 60 frames
        // use std::sync::atomic::{AtomicU64, Ordering};
        // static FRAME_COUNT: AtomicU64 = AtomicU64::new(0);
        // let frame = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
        // if frame % 60 == 0 {
        // }
    }
}
