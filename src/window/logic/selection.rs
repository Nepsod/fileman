use crate::icon_label_layout::{
    icon_view_label_layout, IconViewLabelLayout, ICON_LABEL_MAX_LINES_UNSELECTED,
};
use crate::view_mode::{compact_view_layout, icon_view_layout, icon_view_tile_row_stride};
use crate::window::logic::selection_math::{
    file_entry_is_visible, list_row_index_at_list_y, prune_selection_indices, tile_index_range,
    tile_rectangle_selection_indices, tile_row_count, tile_row_range_for_viewport,
    tile_slot_at_list_point, TileGridMode, TILE_VIEWPORT_OVERSCAN_ROWS,
};
use crate::window::imports::*;

impl FilemanWindow {
    pub(crate) fn invalidate_icon_label_layout_cache(&mut self) {
        self.icon_label_layout_cache_key = None;
        self.tile_visible_index_range = None;
        self.last_tile_scroll_top_bits = None;
    }

    pub(crate) fn rebuild_visible_file_indices(&mut self) {
        self.visible_file_indices = self
            .files
            .iter()
            .enumerate()
            .filter_map(|(index, file_info)| {
                let name = file_info.get_name().unwrap_or("");
                file_entry_is_visible(name, &self.search_query, self.show_hidden)
                    .then_some(index)
            })
            .collect();
    }

    pub(crate) fn visible_file_count(&self) -> usize {
        if self.using_subfolder_search() {
            self.search_matches.len()
        } else {
            self.visible_file_indices.len()
        }
    }

    pub(crate) fn visible_file_at(&self, visible_index: usize) -> Option<&FileInfo> {
        let file_index = *self.visible_file_indices.get(visible_index)?;
        self.files.get(file_index)
    }

    pub(crate) fn tile_grid_columns(&self) -> usize {
        match self.view_mode {
            ViewMode::Icon => icon_view_layout(self.icon_size, self.files_panel_width()).columns,
            ViewMode::Compact => compact_view_layout(self.files_panel_width()).columns,
            _ => 1,
        }
        .max(1)
    }

    pub(crate) fn tile_grid_row_stride_px(&self) -> f32 {
        match self.view_mode {
            ViewMode::Icon => {
                let layout = icon_view_layout(self.icon_size, self.files_panel_width());
                icon_view_tile_row_stride(layout.cell_height)
            }
            ViewMode::Compact => compact_view_layout(self.files_panel_width()).row_stride,
            _ => 1.0,
        }
    }

    pub(crate) fn update_tile_visible_index_range(&mut self, item_count: usize) {
        if item_count == 0 || !self.uses_tile_grid() {
            self.tile_visible_index_range = None;
            return;
        }

        let scroll_top = (-self.marquee_scroll_offset().y).as_f32();
        let scroll_bits = scroll_top.to_bits();
        let viewport_height = self
            .marquee_viewport_bounds()
            .size
            .height
            .as_f32()
            .max(1.0);
        let columns = self.tile_grid_columns();
        let row_count = tile_row_count(item_count, columns);
        let row_stride = self.tile_grid_row_stride_px();
        let row_range = tile_row_range_for_viewport(
            scroll_top,
            viewport_height,
            row_stride,
            row_count,
            TILE_VIEWPORT_OVERSCAN_ROWS,
        );

        if self.last_tile_scroll_top_bits == Some(scroll_bits)
            && self.tile_visible_index_range.is_some()
        {
            return;
        }

        self.last_tile_scroll_top_bits = Some(scroll_bits);
        self.tile_visible_index_range = Some(tile_index_range(columns, row_range, item_count));
    }

    pub(crate) fn tile_visible_index_range(&self, item_count: usize) -> std::ops::Range<usize> {
        self.tile_visible_index_range
            .clone()
            .unwrap_or(0..item_count)
    }

    fn icon_label_layout_cache_fingerprint(&self, entry_count: usize) -> (usize, u32, u32, u64, u64) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let panel_width = self.files_panel_width();
        let mut selected_hasher = DefaultHasher::new();
        let mut selected_indices: Vec<_> = self.selected_indices.iter().collect();
        selected_indices.sort();
        for index in selected_indices {
            index.hash(&mut selected_hasher);
        }
        let mut visible_names_hasher = DefaultHasher::new();
        for name in self.visible_entry_names() {
            name.hash(&mut visible_names_hasher);
        }
        (
            entry_count,
            self.icon_size,
            panel_width.to_bits(),
            selected_hasher.finish(),
            visible_names_hasher.finish(),
        )
    }

    pub(crate) fn sync_icon_label_layout_cache(&mut self, window: &Window, cx: &App) {
        if self.view_mode != ViewMode::Icon {
            self.icon_label_layout_cache.clear();
            self.icon_label_layout_cache_key = None;
            return;
        }

        let names = self.visible_entry_names();
        let cache_key = self.icon_label_layout_cache_fingerprint(names.len());
        if self.icon_label_layout_cache_key == Some(cache_key) {
            return;
        }
        self.icon_label_layout_cache_key = Some(cache_key);
        let layout = icon_view_layout(self.icon_size, self.files_panel_width());
        let label_max_width_px =
            (layout.cell_width - ICON_VIEW_PADDING_PX * 2.0).max(10.0);
        self.icon_label_layout_cache
            .resize(names.len(), IconViewLabelLayout::fallback(label_max_width_px));
        let item_count = names.len();
        self.update_tile_visible_index_range(item_count);
        let visible_range = self.tile_visible_index_range(item_count);
        for index in visible_range {
            let Some(name) = names.get(index) else {
                continue;
            };
            let max_lines = if self.selected_indices.contains(&index) {
                None
            } else {
                Some(ICON_LABEL_MAX_LINES_UNSELECTED)
            };
            self.icon_label_layout_cache[index] = icon_view_label_layout(
                name,
                label_max_width_px,
                max_lines,
                window,
                cx,
            );
        }
    }

    pub(crate) fn cached_icon_label_layout(&self, entry_index: usize) -> Option<IconViewLabelLayout> {
        self.icon_label_layout_cache.get(entry_index).copied()
    }
    pub(crate) fn apply_directory_drop_target(
        &self,
        row: Stateful<Div>,
        destination: PathBuf,
        cx: &mut ViewContext<Self>,
    ) -> Stateful<Div> {
        row.drag_over::<DraggedFilePaths>(|style, _, _, cx| drop_target_style(style, cx))
            .drag_over::<ExternalPaths>(|style, _, _, cx| drop_target_style(style, cx))
            .can_drop({
                let destination = destination.clone();
                move |payload, _, _| {
                    payload
                        .downcast_ref::<DraggedFilePaths>()
                        .is_some_and(|dragged| {
                            crate::drag::is_valid_drop_destination(&dragged.paths, &destination)
                        })
                        || payload
                            .downcast_ref::<ExternalPaths>()
                            .is_some_and(|paths| {
                                crate::drag::is_valid_drop_destination(paths.paths(), &destination)
                            })
                }
            })
            .on_drop(cx.listener({
                let destination = destination.clone();
                move |this, dragged: &DraggedFilePaths, window, cx| {
                    this.drop_into_directory(&destination, dragged, window, cx);
                }
            }))
            .on_drop(cx.listener({
                let destination = destination.clone();
                move |this, paths: &ExternalPaths, window, cx| {
                    this.drop_external_into_directory(&destination, paths, window, cx);
                }
            }))
    }

    pub(crate) fn apply_list_selection_click(
        &mut self,
        entry_index: usize,
        shift: bool,
        control: bool,
        cx: &mut ViewContext<Self>,
    ) {
        if control {
            if !self.selected_indices.remove(&entry_index) {
                self.selected_indices.insert(entry_index);
            }
            self.selection_anchor = Some(entry_index);
        } else if shift {
            if let Some(anchor) = self.selection_anchor {
                self.select_visible_range(anchor, entry_index);
            } else {
                self.selected_indices.clear();
                self.selected_indices.insert(entry_index);
                self.selection_anchor = Some(entry_index);
            }
        } else {
            self.selected_indices.clear();
            self.selected_indices.insert(entry_index);
            self.selection_anchor = Some(entry_index);
        }
        self.list_focus_index = Some(entry_index);
        cx.notify();
    }

    pub(crate) fn apply_marquee_selection(
        &mut self,
        origin_list: Point<Pixels>,
        pointer_list: Point<Pixels>,
        extend_selection: bool,
        window: Option<&Window>,
        cx: &App,
    ) {
        if let Some(window) = window {
            if self.view_mode == ViewMode::Icon {
                self.sync_icon_label_layout_cache(window, cx);
            }
        }
        let names = self.visible_entry_names();
        if names.is_empty() {
            return;
        }

        if !extend_selection {
            self.selected_indices.clear();
        }

        let selected_indices =
            self.marquee_selected_indices(origin_list, pointer_list, names.len());
        let selection_anchor = selected_indices.first().copied();
        let selection_focus = selected_indices.last().copied();
        for index in selected_indices {
            self.selected_indices.insert(index);
        }

        if let Some(anchor) = selection_anchor {
            self.selection_anchor = Some(anchor);
        }
        if let Some(focus) = selection_focus {
            self.list_focus_index = Some(focus);
        }
    }

    fn marquee_selected_indices(
        &self,
        origin_list: Point<Pixels>,
        pointer_list: Point<Pixels>,
        item_count: usize,
    ) -> Vec<usize> {
        if item_count == 0 {
            return Vec::new();
        }

        let selection_left = origin_list.x.min(pointer_list.x);
        let selection_right = origin_list.x.max(pointer_list.x);
        let selection_top = origin_list.y.min(pointer_list.y);
        let selection_bottom = origin_list.y.max(pointer_list.y);

        if matches!(self.view_mode, ViewMode::Icon | ViewMode::Compact) {
            return (0..item_count)
                .filter(|&index| self.tile_marquee_intersects_index(index, selection_left, selection_right, selection_top, selection_bottom))
                .collect();
        }

        let item_height = self.marquee_list_row_height();
        if item_height <= px(0.) {
            return Vec::new();
        }

        (0..item_count)
            .filter(|&index| {
                let row_top = item_height * index as f32;
                let row_bottom = row_top + item_height;
                row_top < selection_bottom && row_bottom > selection_top
            })
            .collect()
    }

    fn point_in_bounds(point: Point<Pixels>, bounds: Bounds<Pixels>) -> bool {
        point.x >= bounds.left()
            && point.x <= bounds.right()
            && point.y >= bounds.top()
            && point.y <= bounds.bottom()
    }

    fn visible_entry_count(&self) -> usize {
        if self.using_subfolder_search() {
            self.search_matches.len()
        } else {
            self.visible_files().len()
        }
    }

    fn tile_slot_at_list_point(&self, list_point: Point<Pixels>, item_count: usize) -> Option<usize> {
        let mode = match self.view_mode {
            ViewMode::Icon => TileGridMode::Icon,
            ViewMode::Compact => TileGridMode::Compact,
            _ => return None,
        };
        tile_slot_at_list_point(
            mode,
            list_point.x.as_f32(),
            list_point.y.as_f32(),
            item_count,
            self.icon_size,
            self.files_panel_width(),
        )
    }

    fn list_point_on_tile_interactive_part(
        &self,
        list_point: Point<Pixels>,
        item_count: usize,
    ) -> bool {
        let Some(index) = self.tile_slot_at_list_point(list_point, item_count) else {
            return false;
        };
        let (icon_bounds, label_bounds) = self.tile_interactive_bounds(index);
        Self::point_in_bounds(list_point, icon_bounds)
            || Self::point_in_bounds(list_point, label_bounds)
    }

    fn list_row_index_at_list_point(
        &self,
        list_point: Point<Pixels>,
        item_count: usize,
    ) -> Option<usize> {
        if item_count == 0 || self.uses_tile_grid() {
            return None;
        }

        if list_point.y < px(0.) {
            return None;
        }

        let row_height = self.marquee_list_row_height();
        if row_height <= px(0.) {
            return None;
        }

        list_row_index_at_list_y(list_point.y.as_f32(), row_height.as_f32(), item_count)
    }

    fn marquee_starts_on_background(&self, list_point: Point<Pixels>, item_count: usize) -> bool {
        if item_count == 0 {
            return true;
        }
        if self.uses_tile_grid() {
            match self.tile_slot_at_list_point(list_point, item_count) {
                None => true,
                Some(_) => !self.list_point_on_tile_interactive_part(list_point, item_count),
            }
        } else if list_point.y < px(0.) {
            false
        } else {
            self.list_row_index_at_list_point(list_point, item_count).is_none()
        }
    }

    fn tile_marquee_intersects_index(
        &self,
        index: usize,
        selection_left: Pixels,
        selection_right: Pixels,
        selection_top: Pixels,
        selection_bottom: Pixels,
    ) -> bool {
        let intersects = |bounds: Bounds<Pixels>| {
            bounds.left() < selection_right
                && bounds.right() > selection_left
                && bounds.top() < selection_bottom
                && bounds.bottom() > selection_top
        };
        let (icon_bounds, label_bounds) = self.tile_interactive_bounds(index);
        intersects(icon_bounds) || intersects(label_bounds)
    }

    fn tile_interactive_bounds(&self, index: usize) -> (Bounds<Pixels>, Bounds<Pixels>) {
        let cell = self.tile_cell_bounds(index);
        match self.view_mode {
            ViewMode::Icon => {
                let layout = icon_view_layout(self.icon_size, self.files_panel_width());
                let icon_pixel_size = self.icon_size as f32;
                let label_max_width =
                    (layout.cell_width - ICON_VIEW_PADDING_PX * 2.0).max(10.0);
                let label_shell_padding = ICON_TILE_LABEL_SHELL_PADDING_PX;
                let label_layout = self
                    .icon_label_layout_cache
                    .get(index)
                    .copied()
                    .unwrap_or(IconViewLabelLayout::fallback(label_max_width));
                let label_hit_width = label_layout.width.min(layout.cell_width);
                let icon_left =
                    cell.origin.x + px((layout.cell_width - icon_pixel_size) / 2.0);
                let icon_top = cell.origin.y + px(ICON_VIEW_PADDING_PX);
                let icon_bounds = Bounds::from_corners(
                    point(icon_left, icon_top),
                    point(
                        icon_left + px(icon_pixel_size),
                        icon_top + px(icon_pixel_size),
                    ),
                );
                let label_top = icon_bounds.bottom() + px(ICON_ICON_LABEL_GAP_PX);
                let label_left =
                    cell.origin.x + px((layout.cell_width - label_hit_width) / 2.0);
                let label_bounds = Bounds::from_corners(
                    point(label_left, label_top),
                    point(
                        label_left + px(label_hit_width),
                        label_top + px(label_layout.height + label_shell_padding * 2.0),
                    ),
                );
                (icon_bounds, label_bounds)
            }
            ViewMode::Compact => {
                let icon_block = COMPACT_TILE_ICON_PX as f32
                    + COMPACT_TILE_PART_SHELL_PADDING_PX * 2.0;
                let inner_left = cell.origin.x + px(COMPACT_TILE_HORIZONTAL_PADDING_PX);
                let icon_top = cell.origin.y
                    + px((COMPACT_TILE_HEIGHT_PX - icon_block) / 2.0);
                let icon_bounds = Bounds::from_corners(
                    point(inner_left, icon_top),
                    point(inner_left + px(icon_block), icon_top + px(icon_block)),
                );
                let label_left = icon_bounds.right()
                    + px(COMPACT_TILE_ICON_LABEL_GAP_PX + COMPACT_TILE_PART_SHELL_PADDING_PX);
                let label_width = COMPACT_TILE_WIDTH_PX
                    - COMPACT_TILE_HORIZONTAL_PADDING_PX * 2.0
                    - icon_block
                    - COMPACT_TILE_ICON_LABEL_GAP_PX
                    - COMPACT_TILE_PART_SHELL_PADDING_PX * 2.0;
                let label_top =
                    cell.origin.y + px(COMPACT_TILE_PART_SHELL_PADDING_PX);
                let label_bounds = Bounds::from_corners(
                    point(label_left, label_top),
                    point(
                        label_left + px(label_width.max(0.0)),
                        cell.origin.y + px(COMPACT_TILE_HEIGHT_PX - COMPACT_TILE_PART_SHELL_PADDING_PX),
                    ),
                );
                (icon_bounds, label_bounds)
            }
            _ => (cell, cell),
        }
    }

    fn tile_cell_bounds(&self, index: usize) -> Bounds<Pixels> {
        let padding = px(ICON_VIEW_PADDING_PX);
        match self.view_mode {
            ViewMode::Icon => {
                let layout = icon_view_layout(self.icon_size, self.files_panel_width());
                let columns = layout.columns.max(1);
                let column = index % columns;
                let row = index / columns;
                let left = padding
                    + px(column as f32 * icon_view_tile_column_stride(layout.cell_width));
                let top =
                    padding + px(row as f32 * icon_view_tile_row_stride(layout.cell_height));
                Bounds::from_corners(
                    point(left, top),
                    point(left + px(layout.cell_width), top + px(layout.cell_height)),
                )
            }
            ViewMode::Compact => {
                let layout = compact_view_layout(self.files_panel_width());
                let columns = layout.columns.max(1);
                let column = index % columns;
                let row = index / columns;
                let cell_stride_x = layout.cell_width + layout.spacing;
                let left = padding + px(column as f32 * cell_stride_x);
                let top = padding + px(row as f32 * layout.row_stride);
                Bounds::from_corners(
                    point(left, top),
                    point(
                        left + px(layout.cell_width),
                        top + px(layout.cell_height),
                    ),
                )
            }
            _ => Bounds::new(point(px(0.), px(0.)), size(px(0.), px(0.))),
        }
    }

    fn should_handle_marquee_pointer(&self, list_point: Point<Pixels>, cx: &App) -> bool {
        if cx.has_active_drag() {
            return false;
        }
        !self.uses_tile_grid()
            || !self.list_point_on_tile_interactive_part(list_point, self.visible_entry_count())
    }

    pub(crate) fn attach_marquee_handlers(
        &self,
        layer: Stateful<Div>,
        cx: &mut ViewContext<Self>,
    ) -> Stateful<Div> {
        layer
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if event.button != MouseButton::Left {
                    return;
                }
                let clamped_origin = this.marquee_clamp_pointer_to_viewport(event.position);
                let origin_list = this.marquee_list_point(clamped_origin);
                let item_count = this.visible_entry_count();
                if !this.marquee_starts_on_background(origin_list, item_count)
                    || !this.should_handle_marquee_pointer(origin_list, cx)
                {
                    return;
                }
                this.begin_marquee_drag(
                    event.position,
                    event.modifiers.control || event.modifiers.platform,
                    window,
                    cx,
                );
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if cx.has_active_drag() {
                    if this.marquee_drag.is_some() {
                        this.finish_marquee_drag(cx);
                    }
                    return;
                }
                if this.marquee_drag.is_some()
                    && event.pressed_button != Some(MouseButton::Left)
                {
                    this.finish_marquee_drag(cx);
                    return;
                }
                if this.marquee_drag.is_some() {
                    let extend_selection = this
                        .marquee_drag
                        .as_ref()
                        .is_some_and(|marquee| marquee.extend_selection);
                    this.update_marquee_drag(event.position, extend_selection, Some(window), cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.finish_marquee_drag(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.finish_marquee_drag(cx);
                }),
            )
    }

    pub(crate) fn begin_marquee_drag(
        &mut self,
        origin: Point<Pixels>,
        extend_selection: bool,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        let clamped_origin = self.marquee_clamp_pointer_to_viewport(origin);
        let origin_list = self.marquee_list_point(clamped_origin);
        if !self.should_handle_marquee_pointer(origin_list, cx) {
            return;
        }

        let item_count = self.visible_entry_count();
        let background_pointer_down =
            self.marquee_starts_on_background(origin_list, item_count);
        let (autoscroll_vertical, autoscroll_horizontal) =
            self.marquee_autoscroll_axes(clamped_origin);
        self.marquee_drag = Some(MarqueeDrag {
            origin: clamped_origin,
            pointer: clamped_origin,
            origin_list,
            pointer_list: origin_list,
            extend_selection,
            active: false,
            background_pointer_down,
            autoscroll_vertical,
            autoscroll_horizontal,
        });
        self.start_marquee_autoscroll_task(cx);
        self.marquee_cancel_subscription = Some(cx.observe_window_activation(
            window,
            |this, _, cx| {
                if this.marquee_drag.is_some() {
                    this.finish_marquee_drag(cx);
                }
            },
        ));
        cx.notify();
    }

    pub(crate) fn clamp_pixels(value: Pixels, min: Pixels, max: Pixels) -> Pixels {
        px(value.as_f32().clamp(min.as_f32(), max.as_f32()))
    }

    pub(crate) fn clear_selection(&mut self, cx: &mut ViewContext<Self>) {
        if self.search_active {
            self.search_active = false;
            self.search_query.clear();
        }
        if self.path_edit_active {
            self.path_edit_active = false;
            self.path_input_text = self.current_path.to_string_lossy().to_string();
        }
        if self.pending_delete.is_some() {
            self.pending_delete = None;
        }
        if self.pending_rename.is_some() {
            self.pending_rename = None;
        }
        if self.inline_rename.is_some() {
            self.inline_rename = None;
        }
        if self.pending_rename_collision.is_some() {
            self.pending_rename_collision = None;
        }
        if self.pending_properties.is_some() {
            self.pending_properties = None;
        }
        if self.pending_settings.is_some() {
            self.pending_settings = None;
            self.settings_terminal_focus = false;
        }
        if self.pending_paste_choice.is_some() {
            self.pending_paste_choice = None;
        }
        if self.show_about {
            self.show_about = false;
        }
        if self.paste_cancel.is_some() {
            self.cancel_active_paste(cx);
        }
        self.dismiss_context_menu();
        self.selected_indices.clear();
        self.selection_anchor = None;
        self.list_focus_index = None;
        cx.notify();
    }

    pub(crate) fn files_panel_width(&self) -> f32 {
        let panel_width: f32 = self.marquee_viewport_bounds().size.width.into();
        if panel_width > 0.0 {
            panel_width
        } else {
            let total_width = self.config.window.last_window_width.max(800) as f32;
            let sidebar_width = self.config.window.splitter_pos as f32;
            (total_width - sidebar_width - 64.0).max(200.0)
        }
    }

    pub(crate) fn uses_tile_grid(&self) -> bool {
        matches!(self.view_mode, ViewMode::Icon | ViewMode::Compact)
    }

    pub(crate) fn files_list_item_height(&self) -> Pixels {
        match self.view_mode {
            ViewMode::Icon => {
                px(icon_view_layout(self.icon_size, self.files_panel_width()).cell_height)
            }
            ViewMode::Compact => px(COMPACT_TILE_HEIGHT_PX),
            ViewMode::Table => px(TABLE_ROW_HEIGHT_PX),
            ViewMode::List => px(LIST_ROW_HEIGHT_PX),
        }
    }

    pub(crate) fn clear_file_selection(&mut self, cx: &mut ViewContext<Self>) {
        if self.selected_indices.is_empty()
            && self.selection_anchor.is_none()
            && self.list_focus_index.is_none()
        {
            return;
        }
        self.selected_indices.clear();
        self.selection_anchor = None;
        self.list_focus_index = None;
        cx.notify();
    }

    pub(crate) fn finish_marquee_drag(&mut self, cx: &mut ViewContext<Self>) {
        self.marquee_cancel_subscription.take();
        self.marquee_autoscroll_task.take();
        let Some(marquee) = self.marquee_drag.take() else {
            return;
        };
        if !marquee.active && marquee.background_pointer_down {
            self.clear_file_selection(cx);
        } else {
            cx.notify();
        }
    }

    pub(crate) fn handle_file_list_key(&mut self, event: &KeyDownEvent, cx: &mut ViewContext<Self>) {
        let key = event.keystroke.key.as_str();
        let shift = event.keystroke.modifiers.shift;
        let control = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;

        match key {
            "up" | "arrowup" => {
                let delta = if self.uses_tile_grid() {
                    -(self.tile_grid_columns() as isize)
                } else {
                    -1
                };
                self.move_list_focus_by(delta, shift, cx);
            }
            "down" | "arrowdown" => {
                let delta = if self.uses_tile_grid() {
                    self.tile_grid_columns() as isize
                } else {
                    1
                };
                self.move_list_focus_by(delta, shift, cx);
            }
            "left" | "arrowleft" if self.uses_tile_grid() => {
                self.move_list_focus_by(-1, shift, cx);
            }
            "right" | "arrowright" if self.uses_tile_grid() => {
                self.move_list_focus_by(1, shift, cx);
            }
            "home" => {
                let visible_count = self.visible_file_count();
                if visible_count == 0 {
                    return;
                }
                if shift {
                    self.select_visible_range(self.selection_anchor.unwrap_or(0), 0);
                } else {
                    self.selected_indices.clear();
                    self.selected_indices.insert(0);
                    self.selection_anchor = Some(0);
                }
                self.list_focus_index = Some(0);
                self.scroll_list_index_into_view(0, ScrollStrategy::Top);
                cx.notify();
            }
            "end" => {
                let visible_count = self.visible_file_count();
                if visible_count == 0 {
                    return;
                }
                let last = visible_count - 1;
                if shift {
                    self.select_visible_range(self.selection_anchor.unwrap_or(0), last);
                } else {
                    self.selected_indices.clear();
                    self.selected_indices.insert(last);
                    self.selection_anchor = Some(last);
                }
                self.list_focus_index = Some(last);
                self.scroll_list_index_into_view(last, ScrollStrategy::Bottom);
                cx.notify();
            }
            "enter" => self.open_focused_or_selection(cx),
            "space" if !control => self.open_focused_or_selection(cx),
            _ => {}
        }
    }


    pub(crate) fn invert_selection(&mut self, cx: &mut ViewContext<Self>) {
        let visible_count = self.visible_file_count();
        for index in 0..visible_count {
            if self.selected_indices.contains(&index) {
                self.selected_indices.remove(&index);
            } else {
                self.selected_indices.insert(index);
            }
        }
        cx.notify();
    }

    pub(crate) fn marquee_autoscroll_axes(&self, pointer: Point<Pixels>) -> (i8, i8) {
        let bounds = self.marquee_viewport_bounds();
        if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
            return (0, 0);
        }

        let edge = px(crate::drag::MARQUEE_EDGE_THRESHOLD);
        let local_x = pointer.x - bounds.origin.x;
        let local_y = pointer.y - bounds.origin.y;
        let vertical = if local_y < edge {
            -1
        } else if local_y > bounds.size.height - edge {
            1
        } else {
            0
        };
        let horizontal = if local_x < edge {
            -1
        } else if local_x > bounds.size.width - edge {
            1
        } else {
            0
        };
        (vertical, horizontal)
    }

    pub(crate) fn marquee_clamp_pointer_to_viewport(&self, pointer: Point<Pixels>) -> Point<Pixels> {
        let bounds = self.marquee_viewport_bounds();
        if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
            return pointer;
        }

        point(
            Self::clamp_pixels(pointer.x, bounds.left(), bounds.right()),
            Self::clamp_pixels(pointer.y, bounds.top(), bounds.bottom()),
        )
    }

    pub(crate) fn marquee_list_point(&self, window_point: Point<Pixels>) -> Point<Pixels> {
        let bounds = self.marquee_viewport_bounds();
        let scroll = self.marquee_scroll_offset();
        point(
            window_point.x - bounds.origin.x - scroll.x,
            -scroll.y + (window_point.y - bounds.origin.y),
        )
    }

    pub(crate) fn marquee_list_row_height(&self) -> Pixels {
        if self.uses_tile_grid() {
            return self.files_list_item_height();
        }

        let intended = self.files_list_item_height();
        let Some(measured) = self
            .uniform_list_row_height
            .filter(|height| *height > px(0.))
        else {
            return intended;
        };

        let intended_px = intended.as_f32();
        let measured_px = measured.as_f32();
        if measured_px < intended_px * 0.5 || measured_px > intended_px * 1.5 {
            intended
        } else {
            measured
        }
    }

    /// Converts list-content coordinates to viewport-local coordinates for the marquee layer.
    pub(crate) fn marquee_viewport_point_from_list(&self, list_point: Point<Pixels>) -> Point<Pixels> {
        let scroll = self.marquee_scroll_offset();
        point(list_point.x + scroll.x, list_point.y + scroll.y)
    }

    pub(crate) fn marquee_overlay_bounds_from_list(
        &self,
        origin_list: Point<Pixels>,
        pointer_list: Point<Pixels>,
        use_list_coordinates: bool,
    ) -> (Pixels, Pixels, Pixels, Pixels) {
        let (origin, pointer) = if use_list_coordinates {
            (origin_list, pointer_list)
        } else {
            (
                self.marquee_viewport_point_from_list(origin_list),
                self.marquee_viewport_point_from_list(pointer_list),
            )
        };
        let left = origin.x.min(pointer.x);
        let top = origin.y.min(pointer.y);
        let width = (pointer.x - origin.x).abs();
        let height = (pointer.y - origin.y).abs();
        (left, top, width, height)
    }

    pub(crate) fn marquee_scroll_by(&self, delta: Point<Pixels>) {
        let scroll_handle = &self.files_scroll_handle.0.borrow().base_handle;
        let mut offset = scroll_handle.offset();
        let max_offset = scroll_handle.max_offset();
        offset.x = Self::clamp_pixels(offset.x + delta.x, -max_offset.x, px(0.));
        offset.y = Self::clamp_pixels(offset.y + delta.y, -max_offset.y, px(0.));
        scroll_handle.set_offset(offset);
    }

    pub(crate) fn marquee_scroll_offset(&self) -> Point<Pixels> {
        self.files_scroll_handle.0.borrow().base_handle.offset()
    }

    pub(crate) fn marquee_viewport_bounds(&self) -> Bounds<Pixels> {
        self.files_scroll_handle.0.borrow().base_handle.bounds()
    }

    pub(crate) fn move_list_focus_by(
        &mut self,
        index_delta: isize,
        extend: bool,
        cx: &mut ViewContext<Self>,
    ) {
        let visible_count = self.visible_file_count();
        if visible_count == 0 {
            return;
        }

        let last_index = visible_count - 1;
        let current_index = self.list_focus_index.unwrap_or(0);
        let next_index = ((current_index as isize) + index_delta).clamp(0, last_index as isize)
            as usize;

        self.list_focus_index = Some(next_index);
        self.scroll_list_index_into_view(next_index, ScrollStrategy::Nearest);

        if extend {
            let anchor = self.selection_anchor.unwrap_or(current_index);
            self.select_visible_range(anchor, next_index);
        } else {
            self.selected_indices.clear();
            self.selected_indices.insert(next_index);
            self.selection_anchor = Some(next_index);
        }
        cx.notify();
    }

    pub(crate) fn scroll_list_index_into_view(&self, index: usize, strategy: ScrollStrategy) {
        if !self.uses_tile_grid() {
            self.files_scroll_handle.scroll_to_item(index, strategy);
            return;
        }

        let columns = self.tile_grid_columns().max(1);
        let row = index / columns;
        let (item_top, item_bottom) = match self.view_mode {
            ViewMode::Icon => {
                let layout = icon_view_layout(self.icon_size, self.files_panel_width());
                let top =
                    row as f32 * icon_view_tile_row_stride(layout.cell_height) + ICON_VIEW_PADDING_PX;
                (top, top + layout.cell_height)
            }
            ViewMode::Compact => {
                let layout = compact_view_layout(self.files_panel_width());
                let top = row as f32 * layout.row_stride + ICON_VIEW_PADDING_PX;
                (top, top + layout.cell_height)
            }
            _ => unreachable!(),
        };

        let scroll_handle = &self.files_scroll_handle.0.borrow().base_handle;
        let viewport_height = scroll_handle.bounds().size.height.as_f32();
        let max_offset = scroll_handle.max_offset();
        let mut offset = scroll_handle.offset();
        let visible_top = -offset.y.as_f32();

        let new_top = match strategy {
            ScrollStrategy::Top => item_top,
            ScrollStrategy::Bottom => (item_bottom - viewport_height).max(0.0),
            ScrollStrategy::Center => {
                ((item_top + item_bottom) / 2.0 - viewport_height / 2.0).max(0.0)
            }
            ScrollStrategy::Nearest => {
                if item_top < visible_top {
                    item_top
                } else if item_bottom > visible_top + viewport_height {
                    (item_bottom - viewport_height).max(0.0)
                } else {
                    visible_top
                }
            }
        };

        offset.y = px(-new_top.clamp(0.0, max_offset.y.as_f32()));
        scroll_handle.set_offset(offset);
    }

    pub(crate) fn open_focused_or_selection(&mut self, cx: &mut ViewContext<Self>) {
        if !self.selected_indices.is_empty() {
            self.open_primary_selection(cx);
            return;
        }

        let Some(index) = self.list_focus_index else {
            return;
        };
        let Some(path) = self.path_for_visible_index(index) else {
            return;
        };

        if path.is_dir() {
            self.navigate_to(path, true, cx);
        } else {
            self.launch_file(&path, cx);
        }
    }

    pub(crate) fn refresh_uniform_list_row_height(&mut self, item_count: usize) {
        if item_count == 0 {
            self.uniform_list_row_height = None;
            return;
        }

        let _ = item_count;
        let state = self.files_scroll_handle.0.borrow();
        let Some(item_size) = state.last_item_size else {
            return;
        };
        let measured = item_size.contents.height;
        if measured > px(0.) {
            self.uniform_list_row_height = Some(measured);
        }
    }

    pub(crate) fn register_sidebar_resize_listeners(
        &self,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        let view = cx.entity();
        let view_for_mouse_up = view.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture {
                view_for_mouse_up.update(cx, |this, cx| {
                    if this.sidebar_resize_drag.is_some() {
                        this.finish_sidebar_resize(cx);
                    }
                });
            }
        });
        let view_for_mouse_move = view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }
            if event.pressed_button == Some(MouseButton::Left) {
                view_for_mouse_move.update(cx, |this, cx| {
                    if this.sidebar_resize_drag.is_some() {
                        this.update_sidebar_resize(event.position.x, cx);
                    }
                });
            }
        });
    }

    pub(crate) fn register_marquee_window_listeners(&self, window: &mut Window, cx: &mut ViewContext<Self>) {
        let view = cx.entity();
        let view_for_mouse_up = view.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture {
                view_for_mouse_up.update(cx, |this, cx| {
                    if this.marquee_drag.is_some() {
                        this.finish_marquee_drag(cx);
                    }
                });
            }
        });
        let view_for_mouse_move = view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }

            if event.pressed_button == Some(MouseButton::Left) {
                view_for_mouse_move.update(cx, |this, cx| {
                    if cx.has_active_drag() {
                        if this.marquee_drag.is_some() {
                            this.finish_marquee_drag(cx);
                        }
                        return;
                    }
                    if let Some(extend_selection) =
                        this.marquee_drag.as_ref().map(|marquee| marquee.extend_selection)
                    {
                        this.update_marquee_drag(
                            window.mouse_position(),
                            extend_selection,
                            Some(window),
                            cx,
                        );
                    }
                });
            } else {
                view_for_mouse_move.update(cx, |this, cx| {
                    if this.marquee_drag.is_some() {
                        this.finish_marquee_drag(cx);
                    }
                });
            }
        });
    }

    pub(crate) fn select_all_visible(&mut self, cx: &mut ViewContext<Self>) {
        self.selected_indices = (0..self.visible_file_count()).collect();
        cx.notify();
    }

    pub(crate) fn select_visible_range(&mut self, anchor_index: usize, index: usize) {
        if self.uses_tile_grid() {
            self.select_visible_tile_range(anchor_index, index);
            return;
        }

        let visible_count = self.visible_file_count();
        if visible_count == 0 {
            return;
        }
        let start = anchor_index.min(index).min(visible_count - 1);
        let end = anchor_index.max(index).min(visible_count - 1);
        self.selected_indices.clear();
        for visible_index in start..=end {
            self.selected_indices.insert(visible_index);
        }
    }

    pub(crate) fn path_for_visible_index(&self, visible_index: usize) -> Option<PathBuf> {
        if self.using_subfolder_search() {
            self.search_matches
                .get(visible_index)
                .map(|search_match| search_match.path.clone())
        } else {
            self.visible_file_at(visible_index).map(|file_info| {
                self.current_path
                    .join(file_info.get_name().unwrap_or(""))
            })
        }
    }

    fn visible_index_for_path(&self, path: &Path) -> Option<usize> {
        if self.using_subfolder_search() {
            self.search_matches
                .iter()
                .position(|search_match| search_match.path == path)
        } else {
            let name = path.file_name()?.to_str()?;
            self.visible_file_indices.iter().position(|&file_index| {
                self.files
                    .get(file_index)
                    .and_then(|file_info| file_info.get_name())
                    .is_some_and(|entry_name| entry_name == name)
            })
        }
    }

    pub(crate) fn remap_selection_after_rename(&mut self, source: &Path, destination: &Path) {
        let Some(old_index) = self.visible_index_for_path(source) else {
            return;
        };
        if !self.selected_indices.remove(&old_index) {
            return;
        }
        if let Some(new_index) = self.visible_index_for_path(destination) {
            self.selected_indices.insert(new_index);
        }
    }

    pub(crate) fn prune_selection_to_visible(&mut self) {
        let visible_count = self.visible_file_count();
        prune_selection_indices(
            &mut self.selected_indices,
            visible_count,
            &mut self.list_focus_index,
        );
    }

    pub(crate) fn start_marquee_autoscroll_task(&mut self, cx: &mut ViewContext<Self>) {
        self.marquee_autoscroll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;

                let continue_task = this
                    .update(cx, |this, cx| {
                        if this.marquee_drag.is_none() {
                            return false;
                        }
                        this.tick_marquee_autoscroll(cx);
                        true
                    })
                    .unwrap_or(false);

                if !continue_task {
                    break;
                }
            }
        }));
    }

    pub(crate) fn tick_marquee_autoscroll(&mut self, cx: &mut ViewContext<Self>) {
        let (vertical, horizontal, extend_selection) = {
            let Some(marquee) = self.marquee_drag.as_ref() else {
                return;
            };
            if marquee.autoscroll_vertical == 0 && marquee.autoscroll_horizontal == 0 {
                return;
            }
            (
                marquee.autoscroll_vertical,
                marquee.autoscroll_horizontal,
                marquee.extend_selection,
            )
        };

        let step = px(crate::drag::MARQUEE_AUTOSCROLL_STEP);
        self.marquee_scroll_by(point(
            step * horizontal as f32,
            step * -vertical as f32,
        ));

        let bounds = self.marquee_viewport_bounds();
        let pointer = {
            let Some(marquee) = self.marquee_drag.as_ref() else {
                return;
            };
            point(
                if horizontal > 0 {
                    bounds.right()
                } else if horizontal < 0 {
                    bounds.left()
                } else {
                    marquee.pointer.x
                },
                if vertical > 0 {
                    bounds.bottom()
                } else if vertical < 0 {
                    bounds.top()
                } else {
                    marquee.pointer.y
                },
            )
        };

        self.update_marquee_drag(pointer, extend_selection, None, cx);
    }

    pub(crate) fn update_marquee_drag(
        &mut self,
        pointer: Point<Pixels>,
        extend_selection: bool,
        window: Option<&Window>,
        cx: &mut ViewContext<Self>,
    ) {
        if cx.has_active_drag() {
            if self.marquee_drag.is_some() {
                self.finish_marquee_drag(cx);
            }
            return;
        }

        let clamped_pointer = self.marquee_clamp_pointer_to_viewport(pointer);
        let pointer_list = self.marquee_list_point(clamped_pointer);
        let (autoscroll_vertical, autoscroll_horizontal) =
            self.marquee_autoscroll_axes(clamped_pointer);
        let (origin_list, pointer_list, should_apply, became_active) = {
            let Some(marquee) = self.marquee_drag.as_mut() else {
                return;
            };

            marquee.pointer = clamped_pointer;
            marquee.pointer_list = pointer_list;
            marquee.autoscroll_vertical = autoscroll_vertical;
            marquee.autoscroll_horizontal = autoscroll_horizontal;
            let became_active = !marquee.active
                && crate::drag::marquee_exceeds_threshold(marquee.origin, clamped_pointer);
            if became_active {
                marquee.active = true;
            }

            (
                marquee.origin_list,
                marquee.pointer_list,
                marquee.active,
                became_active,
            )
        };

        if should_apply {
            if became_active && !extend_selection {
                self.selected_indices.clear();
                self.selection_anchor = None;
                self.list_focus_index = None;
            }
            self.apply_marquee_selection(
                origin_list,
                pointer_list,
                extend_selection,
                window,
                cx,
            );
        }
        cx.notify();
    }

    fn select_visible_tile_range(&mut self, anchor_index: usize, index: usize) {
        let visible_count = self.visible_file_count();
        if visible_count == 0 {
            return;
        }

        let columns = self.tile_grid_columns().max(1);
        let selected_indices =
            tile_rectangle_selection_indices(anchor_index, index, columns, visible_count);
        self.selected_indices.clear();
        for visible_index in selected_indices {
            self.selected_indices.insert(visible_index);
        }
    }

    pub(crate) fn visible_entry_names(&self) -> Vec<String> {
        if self.using_subfolder_search() {
            self.search_matches
                .iter()
                .map(|search_match| search_match.path.to_string_lossy().into_owned())
                .collect()
        } else {
            self.visible_files()
                .into_iter()
                .filter_map(|file_info| file_info.get_name().map(str::to_string))
                .collect()
        }
    }

    pub(crate) fn visible_files(&self) -> Vec<&FileInfo> {
        self.visible_file_indices
            .iter()
            .filter_map(|&index| self.files.get(index))
            .collect()
    }

}
