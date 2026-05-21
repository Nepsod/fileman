use crate::actions::*;
use crate::clipboard::FileClipboard;
use crate::config::FilemanConfig;
use crate::devices::VolumeMount;
use crate::drag::{drop_target_style, DraggedFilePaths, MarqueeDrag};
use crate::jobs::{count_paste_conflicts, ConflictResolution, PasteJobSettings, run_paste_batch};
use crate::location_bar::breadcrumb_segments;
use crate::navigation::NavigationState;
use crate::operations::{
    create_directory, create_file, delete_path, duplicate_path, move_to_trash, PasteResult,
    rename_path, unique_name_in_parent,
};
use crate::properties::PropertiesDialog;
use crate::search::{SearchMatch, SearchScope};
use crate::settings::{SettingsDraft, SettingsField};
use crate::sort::{SortColumn, SortOrder};
use crate::tabs::TabModel;
use crate::toolbar_input::{ToolbarLineInput, ToolbarLineInputEvent};
use crate::ui_icons::ThemeIconButton;
use crate::undo::UndoStack;
use crate::view_mode::{clamp_icon_size, ViewMode, ICON_ZOOM_STEP, MAX_ICON_SIZE, MIN_ICON_SIZE};
use crate::window::format::{
    delete_confirmation_message, format_modified, format_size, path_to_file_uri, quick_access_places,
};
use crate::window::{
    ContextMenuTarget, FilemanWindow, PendingDelete, PendingPasteChoice, PendingRename,
};
use crate::icons;
use nptk::file_icons::FileIconPresentation;
use nptk::gpui::{self as gpui, uniform_list, Entity, ScrollStrategy, Subscription, UniformListScrollHandle, *};
use nptk::gpui_tokio::Tokio;
use nptk::std::collections::HashSet;
use nptk::std::ops::Range;
use nptk::std::path::{Path, PathBuf};
use nptk::std::sync::atomic::{AtomicBool, Ordering};
use nptk::std::sync::mpsc;
use nptk::std::sync::Arc;
use nptk::std::time::Duration;
use nptk::theme::ActiveTheme;
use nptk::ui::{
    Checkbox, ContextMenu, DropdownMenu, DropdownStyle, ListItem, ToggleState, WithScrollbar,
    prelude::*,
};
use npio::{get_file_for_uri, FileInfo, FileType};

type ViewContext<'a, T> = gpui::Context<'a, T>;





impl FilemanWindow {
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
                move |this, dragged: &DraggedFilePaths, _, cx| {
                    this.drop_into_directory(&destination, dragged, cx);
                }
            }))
            .on_drop(cx.listener({
                let destination = destination.clone();
                move |this, paths: &ExternalPaths, _, cx| {
                    this.drop_external_into_directory(&destination, paths, cx);
                }
            }))
    }

    pub(crate) fn apply_list_selection_click(
        &mut self,
        entry_index: usize,
        entry_name: &str,
        shift: bool,
        control: bool,
        cx: &mut ViewContext<Self>,
    ) {
        if control {
            if !self.selected_files.remove(entry_name) {
                self.selected_files.insert(entry_name.to_string());
            }
            self.selection_anchor = Some(entry_index);
        } else if shift {
            if let Some(anchor) = self.selection_anchor {
                self.select_visible_range(anchor, entry_index);
            } else {
                self.selected_files.clear();
                self.selected_files.insert(entry_name.to_string());
                self.selection_anchor = Some(entry_index);
            }
        } else {
            self.selected_files.clear();
            self.selected_files.insert(entry_name.to_string());
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
    ) {
        let names = self.visible_entry_names();
        if names.is_empty() {
            return;
        }

        let (Some(start_index), Some(end_index)) =
            self.marquee_index_range(origin_list, pointer_list, names.len())
        else {
            return;
        };

        if !extend_selection {
            self.selected_files.clear();
        }

        for index in start_index..=end_index {
            if let Some(name) = names.get(index) {
                self.selected_files.insert(name.clone());
            }
        }

        self.selection_anchor = Some(start_index);
        self.list_focus_index = Some(end_index);
    }

    pub(crate) fn attach_marquee_handlers(
        &self,
        layer: Stateful<Div>,
        cx: &mut ViewContext<Self>,
    ) -> Stateful<Div> {
        layer
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if event.button == MouseButton::Left {
                    this.begin_marquee_drag(
                        event.position,
                        event.modifiers.control || event.modifiers.platform,
                        window,
                        cx,
                    );
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
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
                    this.update_marquee_drag(event.position, extend_selection, cx);
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
        if self.using_subfolder_search() {
            return;
        }

        if !extend_selection {
            self.clear_selection(cx);
        }

        let clamped_origin = self.marquee_clamp_pointer_to_viewport(origin);
        let origin_list = self.marquee_list_point(clamped_origin);
        let (autoscroll_vertical, autoscroll_horizontal) =
            self.marquee_autoscroll_axes(clamped_origin);
        self.marquee_drag = Some(MarqueeDrag {
            origin,
            pointer: clamped_origin,
            origin_list,
            pointer_list: origin_list,
            extend_selection,
            active: false,
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
        self.dismiss_context_menu();
        self.selected_files.clear();
        self.selection_anchor = None;
        self.list_focus_index = None;
        cx.notify();
    }

    pub(crate) fn files_list_item_height(&self) -> Pixels {
        match self.view_mode {
            ViewMode::Compact => px(40.0),
            ViewMode::Table => px(36.0),
            ViewMode::List => px(36.0),
            ViewMode::Icon => px(96.0),
        }
    }

    pub(crate) fn finish_marquee_drag(&mut self, cx: &mut ViewContext<Self>) {
        self.marquee_cancel_subscription.take();
        self.marquee_autoscroll_task.take();
        if self.marquee_drag.is_some() {
            self.marquee_drag = None;
            cx.notify();
        }
    }

    pub(crate) fn handle_file_list_key(&mut self, event: &KeyDownEvent, cx: &mut ViewContext<Self>) {
        let key = event.keystroke.key.as_str();
        let shift = event.keystroke.modifiers.shift;
        let control = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;

        match key {
            "up" | "arrowup" => self.move_list_focus(-1, shift, cx),
            "down" | "arrowdown" => self.move_list_focus(1, shift, cx),
            "home" => {
                let names = self.visible_entry_names();
                if names.is_empty() {
                    return;
                }
                if shift {
                    self.select_visible_range(self.selection_anchor.unwrap_or(0), 0);
                } else {
                    self.selected_files.clear();
                    self.selected_files.insert(names[0].clone());
                    self.selection_anchor = Some(0);
                }
                self.list_focus_index = Some(0);
                self.files_scroll_handle
                    .scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
            "end" => {
                let names = self.visible_entry_names();
                if names.is_empty() {
                    return;
                }
                let last = names.len() - 1;
                if shift {
                    self.select_visible_range(self.selection_anchor.unwrap_or(0), last);
                } else {
                    self.selected_files.clear();
                    self.selected_files.insert(names[last].clone());
                    self.selection_anchor = Some(last);
                }
                self.list_focus_index = Some(last);
                self.files_scroll_handle
                    .scroll_to_item(last, ScrollStrategy::Bottom);
                cx.notify();
            }
            "enter" => self.open_focused_or_selection(cx),
            "space" if !control => self.open_focused_or_selection(cx),
            _ => {}
        }
    }


    pub(crate) fn icon_grid_columns(&self) -> usize {
        const ICON_CELL_WIDTH: f32 = 88.0;
        const ICON_GRID_GAP: f32 = 8.0;
        let panel_width: f32 = self.marquee_viewport_bounds().size.width.into();
        let panel_width = if panel_width > 0.0 {
            panel_width
        } else {
            let total_width = self.config.window.last_window_width.max(800) as f32;
            let sidebar_width = self.config.window.splitter_pos as f32;
            (total_width - sidebar_width - 64.0).max(200.0)
        };
        (panel_width / (ICON_CELL_WIDTH + ICON_GRID_GAP))
            .floor()
            .max(1.0) as usize
    }

    pub(crate) fn invert_selection(&mut self, cx: &mut ViewContext<Self>) {
        if self.using_subfolder_search() {
            let keys: Vec<String> = self
                .search_matches
                .iter()
                .map(|search_match| Self::selection_key_for_path(&search_match.path))
                .collect();
            for key in keys {
                if self.selected_files.contains(&key) {
                    self.selected_files.remove(&key);
                } else {
                    self.selected_files.insert(key);
                }
            }
        } else {
            let visible_names: Vec<String> = self
                .visible_files()
                .into_iter()
                .filter_map(|file_info| file_info.get_name().map(str::to_string))
                .collect();
            for name in visible_names {
                if self.selected_files.contains(&name) {
                    self.selected_files.remove(&name);
                } else {
                    self.selected_files.insert(name);
                }
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

    pub(crate) fn marquee_index_range(
        &self,
        origin_list: Point<Pixels>,
        pointer_list: Point<Pixels>,
        item_count: usize,
    ) -> (Option<usize>, Option<usize>) {
        if item_count == 0 {
            return (None, None);
        }

        let clamp_index = |index: usize| index.min(item_count.saturating_sub(1));

        if self.view_mode == ViewMode::Icon {
            let columns = self.icon_grid_columns().max(1);
            let padding = px(8.0);
            let gap = px(8.0);
            let cell_width = px(88.0) + gap;
            let cell_height = self.files_list_item_height() + gap;

            let index_at = |point: Point<Pixels>| -> usize {
                let row = ((point.y - padding) / cell_height).floor().max(0.0) as usize;
                let column = ((point.x - padding) / cell_width).floor().max(0.0) as usize;
                row * columns + column
            };

            let top_left = Point::new(
                origin_list.x.min(pointer_list.x),
                origin_list.y.min(pointer_list.y),
            );
            let bottom_right = Point::new(
                origin_list.x.max(pointer_list.x),
                origin_list.y.max(pointer_list.y),
            );
            return (
                Some(clamp_index(index_at(top_left))),
                Some(clamp_index(index_at(bottom_right))),
            );
        }

        let start_index = clamp_index(self.marquee_list_index_for_list_y(
            origin_list.y.min(pointer_list.y),
            item_count,
        ));
        let end_index = clamp_index(self.marquee_list_index_for_list_y(
            origin_list.y.max(pointer_list.y),
            item_count,
        ));
        (Some(start_index), Some(end_index))
    }

    pub(crate) fn marquee_list_index_for_list_y(&self, list_y: Pixels, item_count: usize) -> usize {
        if item_count == 0 {
            return 0;
        }

        let item_height = self.marquee_list_row_height();
        if item_height <= px(0.) {
            return 0;
        }

        let index = (list_y / item_height).floor().max(0.0) as usize;
        index.min(item_count - 1)
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
        self.uniform_list_row_height
            .filter(|height| *height > px(0.))
            .unwrap_or_else(|| self.files_list_item_height())
    }

    pub(crate) fn marquee_local_point(&self, window_point: Point<Pixels>) -> Point<Pixels> {
        let bounds = self.marquee_viewport_bounds();
        point(
            window_point.x - bounds.origin.x,
            window_point.y - bounds.origin.y,
        )
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

    pub(crate) fn move_list_focus(&mut self, delta: isize, extend: bool, cx: &mut ViewContext<Self>) {
        let names = self.visible_entry_names();
        if names.is_empty() {
            return;
        }

        let current_index = self
            .list_focus_index
            .unwrap_or_else(|| names.len().saturating_sub(1));
        let next_index = if delta < 0 {
            current_index.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (current_index + delta as usize).min(names.len() - 1)
        };

        self.list_focus_index = Some(next_index);
        self.files_scroll_handle
            .scroll_to_item(next_index, ScrollStrategy::Nearest);

        if extend {
            let anchor = self.selection_anchor.unwrap_or(current_index);
            self.select_visible_range(anchor, next_index);
        } else {
            self.selected_files.clear();
            self.selected_files.insert(names[next_index].clone());
            self.selection_anchor = Some(next_index);
        }
        cx.notify();
    }

    pub(crate) fn open_focused_or_selection(&mut self, cx: &mut ViewContext<Self>) {
        if !self.selected_files.is_empty() {
            self.open_primary_selection(cx);
            return;
        }

        let names = self.visible_entry_names();
        let Some(index) = self.list_focus_index else {
            return;
        };
        let Some(name) = names.get(index) else {
            return;
        };

        if self.using_subfolder_search() {
            let path = PathBuf::from(name);
            if path.is_dir() {
                self.navigate_to(path, true, cx);
            } else {
                self.launch_file(&path, cx);
            }
            return;
        }

        let full_path = self.current_path.join(name);
        if full_path.is_dir() {
            self.navigate_to(full_path, true, cx);
        } else {
            self.launch_file(&full_path, cx);
        }
    }

    pub(crate) fn refresh_uniform_list_row_height(&mut self, item_count: usize) {
        if item_count == 0 {
            self.uniform_list_row_height = None;
            return;
        }

        let state = self.files_scroll_handle.0.borrow();
        let Some(item_size) = state.last_item_size else {
            return;
        };
        let measured = px(item_size.contents.height.as_f32() / item_count as f32);
        if measured > px(0.) {
            self.uniform_list_row_height = Some(measured);
        }
    }

    pub(crate) fn register_marquee_window_listeners(&self, window: &mut Window, cx: &mut ViewContext<Self>) {
        let view = cx.entity();
        let view_for_mouse_up = view.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture {
                let _ = view_for_mouse_up.update(cx, |this, cx| {
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
                let _ = view_for_mouse_move.update(cx, |this, cx| {
                    if let Some(extend_selection) =
                        this.marquee_drag.as_ref().map(|marquee| marquee.extend_selection)
                    {
                        this.update_marquee_drag(window.mouse_position(), extend_selection, cx);
                    }
                });
            } else {
                let _ = view_for_mouse_move.update(cx, |this, cx| {
                    if this.marquee_drag.is_some() {
                        this.finish_marquee_drag(cx);
                    }
                });
            }
        });
    }

    pub(crate) fn select_all_visible(&mut self, cx: &mut ViewContext<Self>) {
        if self.using_subfolder_search() {
            self.selected_files = self
                .search_matches
                .iter()
                .map(|search_match| Self::selection_key_for_path(&search_match.path))
                .collect();
        } else {
            self.selected_files = self
                .visible_files()
                .into_iter()
                .filter_map(|file_info| file_info.get_name().map(str::to_string))
                .collect();
        }
        cx.notify();
    }

    pub(crate) fn select_visible_range(&mut self, anchor_index: usize, index: usize) {
        let names = self.visible_entry_names();
        if names.is_empty() {
            return;
        }
        let start = anchor_index.min(index).min(names.len() - 1);
        let end = anchor_index.max(index).min(names.len() - 1);
        self.selected_files.clear();
        for name in &names[start..=end] {
            self.selected_files.insert(name.clone());
        }
    }

    pub(crate) fn selection_key_for_path(path: &Path) -> String {
        path.to_string_lossy().into_owned()
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
            (
                marquee.autoscroll_vertical,
                marquee.autoscroll_horizontal,
                marquee.extend_selection,
            )
        };

        if vertical == 0 && horizontal == 0 {
            return;
        }

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

        self.update_marquee_drag(pointer, extend_selection, cx);
    }

    pub(crate) fn update_marquee_drag(
        &mut self,
        pointer: Point<Pixels>,
        extend_selection: bool,
        cx: &mut ViewContext<Self>,
    ) {
        let clamped_pointer = self.marquee_clamp_pointer_to_viewport(pointer);
        let pointer_list = self.marquee_list_point(clamped_pointer);
        let (autoscroll_vertical, autoscroll_horizontal) =
            self.marquee_autoscroll_axes(clamped_pointer);
        let (origin_list, pointer_list, should_apply) = {
            let Some(marquee) = self.marquee_drag.as_mut() else {
                return;
            };

            marquee.pointer = clamped_pointer;
            marquee.pointer_list = pointer_list;
            marquee.autoscroll_vertical = autoscroll_vertical;
            marquee.autoscroll_horizontal = autoscroll_horizontal;
            if !marquee.active
                && crate::drag::marquee_exceeds_threshold(marquee.origin, clamped_pointer)
            {
                marquee.active = true;
            }

            (
                marquee.origin_list,
                marquee.pointer_list,
                marquee.active,
            )
        };

        if should_apply {
            self.apply_marquee_selection(origin_list, pointer_list, extend_selection);
        }
        cx.notify();
    }

    pub(crate) fn visible_entry_names(&self) -> Vec<String> {
        if self.using_subfolder_search() {
            self.search_matches
                .iter()
                .map(|search_match| Self::selection_key_for_path(&search_match.path))
                .collect()
        } else {
            self.visible_files()
                .into_iter()
                .filter_map(|file_info| file_info.get_name().map(str::to_string))
                .collect()
        }
    }

    pub(crate) fn visible_files(&self) -> Vec<&FileInfo> {
        let search = self.search_query.to_ascii_lowercase();
        self.files
            .iter()
            .filter(|file_info| {
                let name = file_info.get_name().unwrap_or("");
                if name.is_empty() {
                    return false;
                }
                if !self.show_hidden && name.starts_with('.') {
                    return false;
                }
                if !search.is_empty() && !name.to_ascii_lowercase().contains(&search) {
                    return false;
                }
                true
            })
            .collect()
    }

}
