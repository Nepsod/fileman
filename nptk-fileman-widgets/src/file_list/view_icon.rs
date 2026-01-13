use super::{FileListContent, FileListViewMode};
use nptk::widgets::file_icon::renderer::{render_cached_icon, render_fallback_icon};
use nptk::core::app::font_ctx::FontContext;
use nptk::core::app::info::AppInfo;
use nptk::core::layout::LayoutNode;
use nptk::core::signal::Signal;
use nptk::core::vg::kurbo::{Affine, Rect, RoundedRect, RoundedRectRadii, Shape, Vec2};
use nptk::core::vg::peniko::{Brush, Color, Fill};
use nptk::core::vgi::Graphics;
use nptk::core::widget::Widget;
use nptk::services::filesystem::entry::FileEntry;
use nptk::services::thumbnail::npio_adapter::{file_entry_to_uri, u32_to_thumbnail_size};
use nptk::theme::theme::Theme;
use npio::get_file_for_uri;
use std::collections::HashSet;
use std::path::PathBuf;

impl FileListContent {
    pub(super) fn calculate_icon_view_layout(
        &self,
        viewport_width: f32,
        icon_size: u32,
    ) -> (usize, f32, f32) {
        let icon_size_f = icon_size as f32;
        let cell_width = (icon_size_f + self.icon_view_spacing * 2.0).max(1.0);
        let available_width = (viewport_width - self.icon_view_padding * 2.0).max(1.0);
        let mut columns = (available_width / cell_width).floor().max(1.0) as usize;
        if columns == 0 {
            columns = 1;
        }
        // Calculate cell height: icon + spacing + text area (2-3 lines)
        // Text area: ~37.5px for 2-3 lines, plus spacing between icon and text
        let text_area_height = 12.0 * 1.25 * 2.5; // ~37.5px for 2-3 lines
        let cell_height = icon_size_f + 4.0 + text_area_height + self.icon_view_spacing; // icon + gap + text + bottom spacing
        (columns, cell_width, cell_height)
    }
    pub(super) fn get_icon_position(
        &self,
        index: usize,
        columns: usize,
        cell_width: f32,
        cell_height: f32,
    ) -> (f32, f32) {
        let col = index % columns;
        let row = index / columns;
        let x = self.icon_view_padding + col as f32 * cell_width;
        let y = self.icon_view_padding + row as f32 * cell_height;
        (x, y)
    }
    pub(super) fn render_icon_view(
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

        let icon_size = *self.icon_size.get();
        let (columns, cell_width, cell_height) =
            self.calculate_icon_view_layout(layout.layout.size.width, icon_size);

        // VIEWPORT CULLING: Calculate visible range relative to window
        let viewport_start_y = (-layout.layout.location.y).max(0.0);
        let viewport_end_y = info.size.y as f32 - layout.layout.location.y;

        let start_row = (viewport_start_y / cell_height).floor().max(0.0) as usize;
        let end_row = (viewport_end_y / cell_height).ceil() as usize + 1;

        let start_index = start_row * columns;
        let end_index = (end_row * columns).min(entry_count);

        // Collect indices of unselected and selected items in visible range
        let unselected_indices: Vec<usize> = (start_index..end_index)
            .filter(|&i| !selected_set.contains(&entries[i].path))
            .collect();
        let selected_indices: Vec<usize> = (start_index..end_index)
            .filter(|&i| selected_set.contains(&entries[i].path))
            .collect();

        // Collect visible entries to avoid borrow checker issues
        let visible_entries: Vec<FileEntry> = (start_index..end_index)
            .map(|i| entries[i].clone())
            .collect();

        // Drop the signal references to release the borrow
        drop(entries);
        drop(selected_paths);

        // Pass 1: Render unselected items in visible range
        for (_idx, i) in unselected_indices.iter().enumerate() {
            let entry_idx = i - start_index;
            let entry = &visible_entries[entry_idx];
            self.render_icon_item(
                graphics,
                theme,
                layout,
                info,
                *i,
                entry,
                columns,
                cell_width,
                cell_height,
                icon_size,
                false,
            );
        }

        // Pass 2: Render selected items in visible range (to draw on top)
        for (_idx, i) in selected_indices.iter().enumerate() {
            let entry_idx = i - start_index;
            let entry = &visible_entries[entry_idx];
            self.render_icon_item(
                graphics,
                theme,
                layout,
                info,
                *i,
                entry,
                columns,
                cell_width,
                cell_height,
                icon_size,
                true,
            );
        }
    }
    pub(super) fn render_icon_item(
        &mut self,
        graphics: &mut dyn Graphics,
        theme: &mut dyn Theme,
        layout: &LayoutNode,
        info: &mut AppInfo,
        i: usize,
        entry: &FileEntry,
        columns: usize,
        cell_width: f32,
        cell_height: f32,
        icon_size: u32,
        is_selected: bool,
    ) {
        let (x, y) = self.get_icon_position(i, columns, cell_width, cell_height);
        let cell_rect = Rect::new(
            layout.layout.location.x as f64 + x as f64,
            layout.layout.location.y as f64 + y as f64,
            layout.layout.location.x as f64 + x as f64 + cell_width as f64,
            layout.layout.location.y as f64 + y as f64 + cell_height as f64,
        );

        let font_size = 12.0;
        let (icon_rect, label_rect, display_text, max_text_width) = self.get_icon_item_layout(
            &mut info.font_context,
            entry,
            cell_rect,
            cell_width,
            icon_size as f32,
            is_selected,
        );
        let icon_x = icon_rect.x0;
        let icon_y = icon_rect.y0;

        // Step 3: Drawing

        // 1. Draw Label Backgrounds (Hover/Selection) - behind text
        // Check for hover state (check if cursor is in icon OR label area)
        let is_hovered = if let Some(cursor) = info.cursor_pos {
            let cursor_x = cursor.x as f64;
            let cursor_y = cursor.y as f64;
            // Check if cursor is in icon rectangle
            let in_icon = cursor_x >= icon_rect.x0
                && cursor_x < icon_rect.x1
                && cursor_y >= icon_rect.y0
                && cursor_y < icon_rect.y1;
            // Check if cursor is in label rectangle
            let in_label = cursor_x >= label_rect.x0
                && cursor_x < label_rect.x1
                && cursor_y >= label_rect.y0
                && cursor_y < label_rect.y1;
            in_icon || in_label
        } else {
            false
        };

        if is_hovered && !is_selected {
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

            // Draw label hover rectangle
            let label_hover_rect = RoundedRect::new(
                label_rect.x0,
                label_rect.y0,
                label_rect.x1,
                label_rect.y1,
                RoundedRectRadii::new(3.0, 3.0, 3.0, 3.0),
            );

            graphics.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                &Brush::Solid(hover_color),
                None,
                &label_hover_rect.to_path(0.1),
            );
        }

        if is_selected {
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

            // Draw label selection rectangle
            let label_selection_rect = RoundedRect::new(
                label_rect.x0,
                label_rect.y0,
                label_rect.x1,
                label_rect.y1,
                RoundedRectRadii::new(3.0, 3.0, 3.0, 3.0),
            );

            graphics.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                &Brush::Solid(color.with_alpha(0.9)),
                None,
                &label_selection_rect.to_path(0.1),
            );
        }

        // 2. Draw Icon
        // Try to get thumbnail from cache first, fall back to icon
        let mut use_thumbnail = false;
            let thumbnail_cache_key = (entry.path.clone(), icon_size);
            if let Some(thumbnail_image) = {
                let cache = self.thumbnail_cache.lock().expect("Failed to lock thumbnail_cache in view_icon");
            cache.get(&thumbnail_cache_key).cloned()
        } {
            use nptk::core::vg::peniko::{
                Blob, ImageAlphaType, ImageBrush, ImageData, ImageFormat,
            };
            let image_data = ImageData {
                data: Blob::from(thumbnail_image.data),
                format: ImageFormat::Rgba8,
                alpha_type: ImageAlphaType::Alpha,
                width: thumbnail_image.width,
                height: thumbnail_image.height,
            };
            let image_brush = ImageBrush::new(image_data);
            let scale_x = icon_size as f64 / (thumbnail_image.width as f64);
            let scale_y = icon_size as f64 / (thumbnail_image.height as f64);
            let scale = scale_x.min(scale_y);
            let transform = Affine::scale_non_uniform(scale, scale)
                .then_translate(Vec2::new(icon_x, icon_y));
            if let Some(scene) = graphics.as_scene_mut() {
                scene.draw_image(&image_brush, transform);
            }
            use_thumbnail = true;
        }

        // If no thumbnail, use icon
        if !use_thumbnail {
            // Request thumbnail generation asynchronously (non-blocking)
            if entry.is_file() {
                let mut pending = self.pending_thumbnails.lock().expect("Failed to lock pending_thumbnails in view_icon");
                // Use insert() which returns true if the value was newly inserted (atomic check-and-insert)
                if pending.insert(entry.path.clone()) {
                    let file_clone = get_file_for_uri(&file_entry_to_uri(entry)).ok();
                    let service_clone = self.thumbnail_service.clone();
                    let size = u32_to_thumbnail_size(icon_size);
                    
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
        }

        // Get icon for this entry (only use cached, don't block on loading)
        let cache_key = (entry.path.clone(), icon_size);
        let cached_icon = {
            let cache = self.icon_cache.lock().expect("Failed to lock icon_cache in view_icon");
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
                        let icon = registry_clone.get_file_icon(&*file, icon_size).await;
                        let mut cache = cache_clone.lock().expect("Failed to lock icon_cache in async task (view_icon)");
                    cache.insert(cache_key_clone, icon);
                    // Notify that cache was updated to trigger redraw
                    if cache_update_tx_clone.try_send(()).is_err() {
                        log::debug!("Cache update channel full, skipping notification");
                    }
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

        // 3. Draw Icon Overlays (Hover/Selection) - on top of icon (tint)
        if is_hovered && !is_selected {
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

            // Draw icon hover rectangle (overlay)
            let icon_hover_rect = RoundedRect::new(
                icon_rect.x0,
                icon_rect.y0,
                icon_rect.x1,
                icon_rect.y1,
                RoundedRectRadii::new(3.0, 3.0, 3.0, 3.0),
            );

            graphics.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                &Brush::Solid(hover_color.with_alpha(0.5)), // Tint alpha
                None,
                &icon_hover_rect.to_path(0.1),
            );
        }

        if is_selected {
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

            // Draw icon selection rectangle (overlay)
            let icon_selection_rect = RoundedRect::new(
                icon_rect.x0,
                icon_rect.y0,
                icon_rect.x1,
                icon_rect.y1,
                RoundedRectRadii::new(3.0, 3.0, 3.0, 3.0),
            );

            graphics.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                &Brush::Solid(color.with_alpha(0.5)), // Tint alpha
                None,
                &icon_selection_rect.to_path(0.1),
            );
        }

        // Draw filename in label rectangle
        let text_color = theme
            .get_property(
                self.widget_id(),
                &nptk::theme::properties::ThemeProperty::ColorText,
            )
            .or_else(|| {
                theme.get_default_property(&nptk::theme::properties::ThemeProperty::ColorText)
            })
            .unwrap_or(Color::BLACK);

        // Text position: Start at the left edge of the max_text_width area.
        // We use max_text_width as the wrap width, and ask Parley to center align.
        // So we must position the "box" we are drawing into at the center of the cell.
        let text_x = cell_rect.x0 + (cell_width as f64 - max_text_width as f64) / 2.0;
        let text_y = label_rect.y0 + 2.0; // label_padding

        // Clipping:
        // - Unselected: Clip to cell bounds (minus padding) to prevent overlap.
        // - Selected: Clip horizontally to cell, but extend bottom to allow full text visibility.
        let text_clip_rect = if is_selected {
            Rect::new(
                cell_rect.x0 + self.icon_view_padding as f64,
                cell_rect.y0 + self.icon_view_padding as f64,
                cell_rect.x1 - self.icon_view_padding as f64,
                label_rect.y1 + self.icon_view_padding as f64, // Extend to full label height
            )
        } else {
            Rect::new(
                cell_rect.x0 + self.icon_view_padding as f64,
                cell_rect.y0 + self.icon_view_padding as f64,
                cell_rect.x1 - self.icon_view_padding as f64,
                cell_rect.y1 - self.icon_view_padding as f64,
            )
        };

        // Apply clipping for text rendering to prevent overflow
        use nptk::core::vg::peniko::Mix;
        #[allow(deprecated)]
        graphics.push_layer(
            Mix::Clip,
            1.0,
            Affine::IDENTITY,
            &text_clip_rect.to_path(0.1),
        );

        let transform = Affine::translate((text_x, text_y));

        // Render text with wrapping enabled.
        // We use max_text_width as the wrap_width. This ensures long filenames wrap at the cell boundary.
        let wrap_width = Some(max_text_width);

        // Render text with optional line limit.
        // Dolphin-like behavior:
        // - Not Selected: Limit to 2 lines.
        // - Selected: Show all lines (unlimited).
        let max_lines = if !is_selected { Some(2) } else { None };

        // Always center align.
        // Parley will center the text within wrap_width (which is max_text_width).
        // Since we positioned text_x at the start of max_text_width area, the text will be visually centered in the cell.
        let center_align = true;

        self.text_render_context.render_text_with_max_lines(
            &mut info.font_context,
            graphics,
            &display_text,
            None,
            font_size,
            Brush::Solid(text_color),
            transform,
            true,
            wrap_width,
            max_lines,
            center_align,
        );

        // If not selected and text was truncated (more than 2 lines), draw "..." indicator.
        // This applies to all names (with or without special characters).
        // Manual ellipsis drawing removed as variables are no longer available.
        // Truncation is handled by render_text_with_max_lines and clipping.

        // Pop clipping layer
        graphics.pop_layer();
    }
    pub(super) fn get_icon_item_layout(
        &mut self,
        font_cx: &mut FontContext,
        entry: &FileEntry,
        cell_rect: Rect,
        cell_width: f32,
        icon_size: f32,
        is_selected: bool,
    ) -> (Rect, Rect, String, f32) {
        // Create cache key
        let cache_key = (
            entry.path.clone(),
            FileListViewMode::Icon,
            (cell_width * 100.0) as u32, // Use cell_width as key component
            is_selected,                 // selection state affects line count/label height
        );

        // Check cache first
        if let Some((icon_rect, label_rect, display_text, max_text_width)) =
            self.layout_cache.get(&cache_key)
        {
            // Translate cached rects to current cell position
            let cached_icon_x = icon_rect.x0;
            let cached_icon_y = icon_rect.y0;
            let target_icon_x = cell_rect.x0 + (cell_width as f64 - icon_size as f64) / 2.0;
            let target_icon_y = cell_rect.y0 + self.icon_view_padding as f64;

            let dx = target_icon_x - cached_icon_x;
            let dy = target_icon_y - cached_icon_y;

            let translated_icon_rect = Rect::new(
                icon_rect.x0 + dx,
                icon_rect.y0 + dy,
                icon_rect.x1 + dx,
                icon_rect.y1 + dy,
            );
            let translated_label_rect = Rect::new(
                label_rect.x0 + dx,
                label_rect.y0 + dy,
                label_rect.x1 + dx,
                label_rect.y1 + dy,
            );

            return (
                translated_icon_rect,
                translated_label_rect,
                display_text.clone(),
                *max_text_width,
            );
        }

        // Cache miss - calculate layout
        // Step 1: Calculate Icon Rectangle
        // Icon is centered horizontally, with padding from top
        let icon_x = cell_rect.x0 + (cell_width as f64 - icon_size as f64) / 2.0;
        let icon_y = cell_rect.y0 + self.icon_view_padding as f64;

        let icon_rect = Rect::new(
            icon_x,
            icon_y,
            icon_x + icon_size as f64,
            icon_y + icon_size as f64,
        );

        // Step 2: Prepare text for wrapping
        let font_size = 12.0;
        let line_height = font_size * 1.2;
        let max_text_width = (cell_width - self.icon_view_padding * 2.0).max(10.0);

        let name = &entry.name;
        let (text_with_breaks, has_natural_breaks) = {
            let is_continuous = name.chars().all(|c| c.is_alphanumeric());
            let mut result = String::with_capacity(name.len() + name.len() / 8);
            let mut segment_len: usize = 0;

            for c in name.chars() {
                result.push(c);
                let is_special = !c.is_alphanumeric() && !c.is_whitespace();
                if is_special {
                    result.push('\u{200B}');
                    segment_len = 0;
                } else if c.is_whitespace() {
                    segment_len = 0;
                } else {
                    segment_len += 1;
                    if is_continuous && segment_len >= 10 {
                        result.push('\u{200B}');
                        segment_len = 0;
                    }
                }
            }
            (result, !is_continuous)
        };

        // Measure text layout
        let (measured_width, line_count) = self.text_render_context.measure_text_layout(
            font_cx,
            &text_with_breaks,
            None,
            font_size,
            Some(max_text_width),
        );

        // Calculate label dimensions
        let label_padding = 2.0;
        let label_spacing = 4.0;
        let label_y_start = icon_rect.y1 + label_spacing;

        let displayed_line_count = if is_selected {
            line_count
        } else {
            line_count.min(2)
        };
        let per_line_height = line_height as f64 + 2.0;
        let label_height = (displayed_line_count as f64 * per_line_height).max(per_line_height);

        let max_label_width = (cell_width as f64 - 2.0 * label_padding as f64).max(0.0);
        let base_width = measured_width as f64;
        let label_width =
            if line_count == 1 && !has_natural_breaks && measured_width > max_text_width {
                (base_width.min(max_text_width as f64 * 1.5)).min(max_label_width)
            } else {
                base_width.min(max_text_width as f64).min(max_label_width)
            };

        let label_x = cell_rect.x0 + (cell_width as f64 - label_width) / 2.0;
        let label_y = label_y_start;

        let label_rect = Rect::new(
            label_x - label_padding,
            label_y - label_padding,
            label_x + label_width + label_padding,
            label_y + label_height + label_padding,
        );

        // Store in cache
        self.layout_cache.insert(
            cache_key,
            (
                icon_rect,
                label_rect,
                text_with_breaks.clone(),
                max_text_width,
            ),
        );

        (icon_rect, label_rect, text_with_breaks, max_text_width)
    }
}
