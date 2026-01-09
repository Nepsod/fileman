use std::sync::{Arc, Mutex};

use nalgebra::Vector2;
use nptk::core::app::context::AppContext;
use nptk::core::app::info::AppInfo;
use nptk::core::app::update::Update;
use nptk::core::layout::{Dimension, LayoutNode, LayoutStyle, StyleNode, LengthPercentage};
use nptk::core::menu::{MenuTemplate, MenuItem, MenuCommand};
use nptk::core::signal::{state::StateSignal, MaybeSignal, Signal};
use nptk::core::text_render::TextRenderContext;
use nptk::core::vg::kurbo::{Affine, Point, Rect, Shape, Stroke, Vec2};
use nptk::core::vg::peniko::{Brush, Color, Fill};
use nptk::core::vgi::Graphics;
use nptk::core::widget::{BoxedWidget, Widget, WidgetLayoutExt};
use nptk::core::window::{ElementState, MouseButton};
use nptk::services::filesystem::entry::{FileEntry, FileType};
use nptk::services::filesystem::model::{FileSystemEvent, FileSystemModel};
use npio::service::icon::IconRegistry;
use npio::{ThumbnailService, ThumbnailEvent, ThumbnailImage, get_file_for_uri, register_backend};
use npio::backend::local::LocalBackend;
use nptk::services::thumbnail::npio_adapter::{file_entry_to_uri, u32_to_thumbnail_size, uri_to_path, thumbnail_size_to_u32};
use nptk::theme::id::WidgetId;
use nptk::theme::theme::Theme;
use std::collections::HashSet;
use tokio::{sync::broadcast, time::{Duration, Instant}};

mod actions;
mod properties;
mod view_compact;
mod view_icon;
mod view_list;


/// Simple operation request type for use within FileList widget
/// This is converted to the full FileOperationRequest in FileListWrapper
pub enum FileListOperation {
    Delete(Vec<PathBuf>),
}

use nptk::widgets::scroll_container::{ScrollContainer, ScrollDirection};
use nptk::core::signal::eval::EvalSignal;
use npio::service::filesystem::mime_registry::MimeRegistry;
use std::path::PathBuf;
// Import widgets needed for confirmation dialog
use nptk::widgets::container::Container;
use nptk::widgets::button::Button;
use nptk::widgets::text::Text;

/// View mode for the file list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileListViewMode {
    /// List view (icon + text in rows)
    List,
    /// Large icon view (grid layout with icons and labels below)
    Icon,
    /// Compact view (Tiles view: Icon left, Text right, grid layout)
    Compact,
}

/// A widget that displays a list of files.
pub struct FileList {
    // State
    current_path: StateSignal<PathBuf>,
    entries: StateSignal<Vec<FileEntry>>,
    selected_paths: StateSignal<Vec<PathBuf>>,
    view_mode: StateSignal<FileListViewMode>,
    icon_size: StateSignal<u32>,

    // Model
    fs_model: Arc<FileSystemModel>,
    _event_rx: Arc<Mutex<broadcast::Receiver<FileSystemEvent>>>,

    // Layout
    layout_style: MaybeSignal<LayoutStyle>,

    // Child widgets
    scroll_container: BoxedWidget,

    // Track if signals are hooked
    signals_hooked: bool,
}

impl FileList {
    fn apply_with(mut self, f: impl FnOnce(&mut Self)) -> Self {
        f(&mut self);
        self
    }

    /// Create a new file list widget.
    pub fn new(initial_path: PathBuf) -> Self {
        Self::new_with_operations(initial_path, None)
    }

    /// Create a new file list widget with optional operation channel for file operations.
    /// 
    /// `operation_tx` - if provided, file operations (like delete) will be sent via this channel.
    pub fn new_with_operations(
        initial_path: PathBuf,
        operation_tx: Option<tokio::sync::mpsc::UnboundedSender<FileListOperation>>,
    ) -> Self {
        let fs_model = Arc::new(FileSystemModel::new(initial_path.clone()).unwrap());
        let event_rx = Arc::new(Mutex::new(fs_model.subscribe_events()));

        // Initial load
        let _ = fs_model.refresh(&initial_path);

        let current_path = StateSignal::new(initial_path.clone());
        let entries = StateSignal::new(Vec::new());
        let selected_paths = StateSignal::new(Vec::new());
        let view_mode = StateSignal::new(FileListViewMode::List);
        let icon_size = StateSignal::new(48);

        // Create icon registry
        let icon_registry =
            Arc::new(IconRegistry::new().unwrap_or_else(|_| IconRegistry::default()));

        // Register npio backend if not already registered
        // Note: This is idempotent - registering multiple times is safe
        let backend = Arc::new(LocalBackend::new());
        register_backend(backend);

        // Create thumbnail service
        let thumbnail_service = Arc::new(ThumbnailService::new());
        let thumbnail_event_rx = thumbnail_service.subscribe();
        
        // Create channel for cache update notifications
        let (cache_update_tx, cache_update_rx) = tokio::sync::mpsc::unbounded_channel();

        // Create content widget
        let content = FileListContent::new(
            entries.clone(),
            selected_paths.clone(),
            current_path.clone(),
            view_mode.clone(),
            icon_size.clone(),
            fs_model.clone(),
            icon_registry.clone(),
            thumbnail_service.clone(),
            thumbnail_event_rx,
            cache_update_tx,
            cache_update_rx,
            operation_tx,
        );

        // Create scroll container (Both directions to support icon view)
        let scroll_container = ScrollContainer::new()
            .with_scroll_direction(ScrollDirection::Both)
            .with_virtual_scrolling(true, 30.0)
            .with_child(content);

        Self {
            current_path,
            entries,
            selected_paths,
            view_mode,
            icon_size,
            fs_model,
            _event_rx: event_rx,
            layout_style: LayoutStyle {
                size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
                ..Default::default()
            }
            .into(),
            scroll_container: Box::new(scroll_container),
            signals_hooked: false,
        }
    }

    /// Set the current path.
    pub fn set_path(&mut self, path: PathBuf) {
        self.current_path.set(path.clone());
        // Trigger reload in model
        let _ = self.fs_model.refresh(&path);
    }

    /// Get the current path.
    pub fn get_current_path(&self) -> PathBuf {
        (*self.current_path.get()).clone()
    }

    /// Get the currently selected paths.
    pub fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected_paths.get().clone()
    }

    /// Get the first selected path (for backward compatibility).
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_paths.get().first().cloned()
    }

    /// Clear the selection.
    pub fn clear_selection(&mut self) {
        self.selected_paths.set(Vec::new());
    }

    /// Select all entries.
    pub fn select_all(&mut self) {
        let entries = self.entries.get();
        let paths: Vec<PathBuf> = entries.iter().map(|e| e.path.clone()).collect();
        self.selected_paths.set(paths);
    }

    /// Set the view mode.
    pub fn set_view_mode(&mut self, mode: FileListViewMode) {
        self.view_mode.set(mode);
    }

    /// Set the icon size for icon view.
    pub fn set_icon_size(&mut self, size: u32) {
        self.icon_size.set(size);
    }

    /// Set the view mode (builder pattern).
    pub fn with_view_mode(self, mode: FileListViewMode) -> Self {
        self.apply_with(|this| this.view_mode.set(mode))
    }

    /// Set the icon size (builder pattern).
    pub fn with_icon_size(self, size: u32) -> Self {
        self.apply_with(|this| this.icon_size.set(size))
    }
}

impl Widget for FileList {
    fn widget_id(&self) -> WidgetId {
        WidgetId::new("nptk-widgets", "FileList")
    }

    fn layout_style(&self) -> StyleNode {
        StyleNode {
            style: self.layout_style.get().clone(),
            children: vec![self.scroll_container.layout_style()],
        }
    }

    fn update(&mut self, layout: &LayoutNode, context: AppContext, info: &mut AppInfo) -> Update {
        // Hook signals on first update to make them reactive
        if !self.signals_hooked {
            context.hook_signal(&mut self.entries);
            context.hook_signal(&mut self.current_path);
            context.hook_signal(&mut self.selected_paths);
            context.hook_signal(&mut self.view_mode);
            context.hook_signal(&mut self.icon_size);
            self.signals_hooked = true;
        }

        let mut update = Update::empty();

        // Poll filesystem events
        if let Ok(mut rx) = self._event_rx.try_lock() {
            while let Ok(event) = rx.try_recv() {
                match event {
                    FileSystemEvent::DirectoryLoaded { path, entries } => {
                        if path == *self.current_path.get() {
                            self.entries.set(entries);
                            update.insert(Update::LAYOUT | Update::DRAW);
                        }
                    },
                    _ => {
                        // For other events, we might want to refresh if they affect current path
                        // But for now, let's just rely on DirectoryLoaded
                    },
                }
            }
        }

        // Update child (ScrollContainer)
        if !layout.children.is_empty() {
            update |= self
                .scroll_container
                .update(&layout.children[0], context.clone(), info);
        }

        update
    }

    fn render(
        &mut self,
        graphics: &mut dyn Graphics,
        theme: &mut dyn Theme,
        layout: &LayoutNode,
        info: &mut AppInfo,
        context: AppContext,
    ) {
        // Render ScrollContainer
        if !layout.children.is_empty() {
            self.scroll_container
                .render(graphics, theme, &layout.children[0], info, context);
        }
    }
}

impl WidgetLayoutExt for FileList {
    fn set_layout_style(&mut self, layout_style: impl Into<MaybeSignal<LayoutStyle>>) {
        self.layout_style = layout_style.into();
    }
}

/// Inner widget that renders the actual list content.
struct FileListContent {
    entries: StateSignal<Vec<FileEntry>>,
    selected_paths: StateSignal<Vec<PathBuf>>,
    current_path: StateSignal<PathBuf>,
    view_mode: StateSignal<FileListViewMode>,
    icon_size: StateSignal<u32>,
    fs_model: Arc<FileSystemModel>,
    icon_registry: Arc<IconRegistry>,
    thumbnail_service: Arc<ThumbnailService>,

    item_height: f32,
    text_render_context: TextRenderContext,
    thumbnail_size: u32,

    // Input state
    last_click_time: Option<Instant>,
    last_click_index: Option<usize>,
    anchor_index: Option<usize>, // For Shift+Click range selection

    // Icon cache per entry (to avoid repeated lookups)
    icon_cache: Arc<
        Mutex<std::collections::HashMap<(PathBuf, u32), Option<npio::service::icon::CachedIcon>>>,
    >,

    // Track pending thumbnail requests to avoid duplicate requests
    pending_thumbnails: Arc<Mutex<HashSet<PathBuf>>>,

    // Thumbnail cache: (path, size) -> ThumbnailImage
    thumbnail_cache: Arc<Mutex<std::collections::HashMap<(PathBuf, u32), ThumbnailImage>>>,

    // Thumbnail event receiver
    thumbnail_event_rx: Arc<Mutex<tokio::sync::broadcast::Receiver<ThumbnailEvent>>>,

    // Update manager for triggering redraws from async tasks
    update_manager: Arc<Mutex<Option<nptk::core::app::update::UpdateManager>>>,
    
    // Channel to notify when caches are updated (for triggering redraws)
    cache_update_tx: tokio::sync::mpsc::UnboundedSender<()>,
    cache_update_rx: Arc<Mutex<tokio::sync::mpsc::UnboundedReceiver<()>>>,

    // Drag selection state
    drag_start: Option<Point>,
    current_drag_pos: Option<Point>,
    is_dragging: bool,

    // Layout cache to avoid expensive recalculations on every frame
    // Key: (path, view_mode, cell_width/icon_size)
    // Value: (icon_rect, label_rect, display_text, max_text_width)
    layout_cache: std::collections::HashMap<
        (PathBuf, FileListViewMode, u32, bool),
        (Rect, Rect, String, f32),
    >,
    last_layout_width: f32,

    // Icon view constants
    icon_view_padding: f32,
    icon_view_spacing: f32,

    // SVG Scene cache to avoid re-parsing SVGs every frame
    // Key: SVG source string (or hash of it)
    // Value: (Scene, width, height)
    svg_scene_cache: std::collections::HashMap<String, (nptk::core::vg::Scene, f64, f64)>,
    mime_registry: MimeRegistry,
    pending_action: Arc<Mutex<Option<PendingAction>>>,
    operation_tx: Option<tokio::sync::mpsc::UnboundedSender<FileListOperation>>,
    last_cursor: Option<Point>,
    menu_was_open: bool, // Track if menu was open in previous update to detect when it closes
    pending_delete_confirmation: Arc<Mutex<Option<Vec<PathBuf>>>>, // Paths waiting for delete confirmation
}

#[derive(Clone)]
struct PendingAction {
    paths: Vec<PathBuf>,
    app_id: Option<String>,
    properties: bool,
    delete: bool, // If true, this is a delete action
}

impl FileListContent {
    fn new(
        entries: StateSignal<Vec<FileEntry>>,
        selected_paths: StateSignal<Vec<PathBuf>>,
        current_path: StateSignal<PathBuf>,
        view_mode: StateSignal<FileListViewMode>,
        icon_size: StateSignal<u32>,
        fs_model: Arc<FileSystemModel>,
        icon_registry: Arc<IconRegistry>,
        thumbnail_service: Arc<ThumbnailService>,
        thumbnail_event_rx: tokio::sync::broadcast::Receiver<ThumbnailEvent>,
        cache_update_tx: tokio::sync::mpsc::UnboundedSender<()>,
        cache_update_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
        operation_tx: Option<tokio::sync::mpsc::UnboundedSender<FileListOperation>>,
    ) -> Self {
        Self {
            entries,
            selected_paths,
            current_path,
            view_mode,
            icon_size,
            fs_model,
            icon_registry,
            thumbnail_service,
            item_height: 30.0,
            text_render_context: TextRenderContext::new(),
            thumbnail_size: 128,
            last_click_time: None,
            last_click_index: None,
            anchor_index: None,
            icon_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
            pending_thumbnails: Arc::new(Mutex::new(HashSet::new())),
            thumbnail_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
            thumbnail_event_rx: Arc::new(Mutex::new(thumbnail_event_rx)),
            update_manager: Arc::new(Mutex::new(None)),
            cache_update_tx,
            cache_update_rx: Arc::new(Mutex::new(cache_update_rx)),
            drag_start: None,
            current_drag_pos: None,
            is_dragging: false,
            layout_cache: std::collections::HashMap::new(),
            last_layout_width: 1000.0,
            icon_view_padding: 2.0,
            icon_view_spacing: 22.0,
            svg_scene_cache: std::collections::HashMap::new(),
            mime_registry: MimeRegistry::load_default(),
            pending_action: Arc::new(Mutex::new(None)),
            operation_tx,
            last_cursor: None,
            menu_was_open: false,
            pending_delete_confirmation: Arc::new(Mutex::new(None)),
        }
        .with_thumbnail_size(128)
    }

    pub fn with_thumbnail_size(mut self, size: u32) -> Self {
        self.thumbnail_size = size;
        self
    }

    fn is_selected(&self, path: &PathBuf) -> bool {
        self.selected_paths.get().contains(path)
    }

    fn update_drag_selection(&mut self, selection_rect: Rect, toggle: bool, layout_width: f32) {
        let entries = self.entries.get();
        let view_mode = *self.view_mode.get();
        let icon_size = *self.icon_size.get();

        let mut new_selection = if toggle {
            self.selected_paths.get().clone()
        } else {
            Vec::new()
        };

        // Helper to check intersection
        let check_intersection = |item_rect: Rect| -> bool {
            let intersection = selection_rect.intersect(item_rect);
            intersection.width() > 0.0 && intersection.height() > 0.0
        };

        if view_mode == FileListViewMode::Icon {
            let (columns, cell_width, cell_height) =
                self.calculate_icon_view_layout(layout_width, icon_size);

            for (i, entry) in entries.iter().enumerate() {
                let (x, y) = self.get_icon_position(i, columns, cell_width, cell_height);
                // We use the full cell rect for intersection to make it easier to select
                let cell_rect = Rect::new(
                    x as f64,
                    y as f64,
                    (x + cell_width) as f64,
                    (y + cell_height) as f64,
                );

                if check_intersection(cell_rect) {
                    if !new_selection.contains(&entry.path) {
                        new_selection.push(entry.path.clone());
                    }
                } else if !toggle {
                    // If not toggling, we strictly set selection to what's in the rect
                    // So if it was selected but not in rect, it's removed (already handled by init empty Vec)
                }
            }
        } else if view_mode == FileListViewMode::Compact {
            let (columns, cell_width, cell_height, spacing) =
                self.calculate_compact_view_layout(layout_width);

            for (i, entry) in entries.iter().enumerate() {
                let col = i % columns;
                let row = i / columns;
                let x = self.icon_view_padding + col as f32 * (cell_width + spacing);
                let y = self.icon_view_padding + row as f32 * (cell_height + spacing);

                let cell_rect = Rect::new(
                    x as f64,
                    y as f64,
                    (x + cell_width) as f64,
                    (y + cell_height) as f64,
                );

                if check_intersection(cell_rect) {
                    if !new_selection.contains(&entry.path) {
                        new_selection.push(entry.path.clone());
                    }
                }
            }
        } else {
            // List view
            for (i, entry) in entries.iter().enumerate() {
                let y = i as f32 * self.item_height;
                let row_rect = Rect::new(
                    0.0,
                    y as f64,
                    layout_width as f64,
                    (y + self.item_height) as f64,
                );

                if check_intersection(row_rect) {
                    if !new_selection.contains(&entry.path) {
                        new_selection.push(entry.path.clone());
                    }
                }
            }
        }

        self.selected_paths.set(new_selection);
    }

    /// Show a confirmation dialog asking if the user is sure they want to delete the selected files
    pub(super) fn show_delete_confirmation_dialog(&self, paths: &[PathBuf], context: AppContext) {
        if paths.is_empty() {
            return;
        }

        // Build message text
        let message = if paths.len() == 1 {
            let path = &paths[0];
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("<unnamed>");
            format!("Are you sure you want to delete \"{}\"?", name)
        } else {
            format!("Are you sure you want to delete {} selected item(s)?", paths.len())
        };

        // Create the dialog widget with message and buttons
        let pending_delete = self.pending_delete_confirmation.clone();
        let paths_to_delete = paths.to_vec();

        // Message text widget
        let message_text = Text::new(message);
        
        // Cancel button - just closes dialog (popup closes automatically on click outside)
        let cancel_btn = Button::new(Text::new("Cancel".to_string()))
            .with_on_pressed(MaybeSignal::value(Update::DRAW));
        
        // Delete button - confirms deletion
        let delete_btn = Button::new(Text::new("Delete".to_string()))
            .with_on_pressed({
                let pending_delete_btn = pending_delete.clone();
                let paths_btn = paths_to_delete.clone();
                MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                    // Set pending delete confirmation - will be processed in update()
                    if let Ok(mut pending) = pending_delete_btn.lock() {
                        *pending = Some(paths_btn.clone());
                    }
                    Update::DRAW
                })))
            });

        // Build dialog content with message and buttons
        let dialog_content = Container::new(vec![
            Box::new(message_text),
            Box::new(Container::new(vec![
                Box::new(cancel_btn),
                Box::new(delete_btn),
            ]).with_layout_style(LayoutStyle {
                flex_direction: nptk::core::layout::FlexDirection::Row,
                gap: Vector2::new(LengthPercentage::length(8.0), LengthPercentage::length(0.0)),
                justify_content: Some(nptk::core::layout::JustifyContent::FlexEnd),
                size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
                ..Default::default()
            })),
        ]).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::length(400.0), Dimension::auto()),
            flex_direction: nptk::core::layout::FlexDirection::Column,
            padding: nptk::core::layout::Rect {
                left: LengthPercentage::length(16.0),
                right: LengthPercentage::length(16.0),
                top: LengthPercentage::length(16.0),
                bottom: LengthPercentage::length(16.0),
            },
            gap: Vector2::new(LengthPercentage::length(0.0), LengthPercentage::length(16.0)),
            ..Default::default()
        });

        // Show popup at cursor position or center of screen
        let pos = self
            .last_cursor
            .map(|p| (p.x as i32, p.y as i32))
            .unwrap_or((300, 200));
        context
            .popup_manager
            .create_popup_at(Box::new(dialog_content), "Confirm Delete", (400, 150), pos);
    }
}

impl Widget for FileListContent {
    fn widget_id(&self) -> WidgetId {
        WidgetId::new("nptk-widgets", "FileListContent")
    }

    fn layout_style(&self) -> StyleNode {
        let view_mode = *self.view_mode.get();
        let count = self.entries.get().len();
        let width = self.last_layout_width.max(1.0);

        let height = if view_mode == FileListViewMode::Icon {
            let icon_size = *self.icon_size.get();
            let (columns, _, cell_height) = self.calculate_icon_view_layout(width, icon_size);
            let rows = (count as f32 / columns as f32).ceil();
            (rows * cell_height + self.icon_view_padding * 2.0).max(100.0)
        } else if view_mode == FileListViewMode::Compact {
            let (columns, _, cell_height, spacing) = self.calculate_compact_view_layout(width);
            let rows = (count as f32 / columns as f32).ceil();
            // Height = rows * cell + (rows - 1) * spacing + padding
            // Approx: rows * (cell + spacing) - spacing + padding
            (rows * (cell_height + spacing) - spacing + self.icon_view_padding * 2.0).max(100.0)
        } else {
            (count as f32 * self.item_height).max(100.0)
        };

        StyleNode {
            style: LayoutStyle {
                size: Vector2::new(Dimension::percent(1.0), Dimension::length(height)),
                ..Default::default()
            },
            children: vec![],
        }
    }

    fn update(&mut self, layout: &LayoutNode, context: AppContext, info: &mut AppInfo) -> Update {
        // Store update manager for async tasks to trigger redraws
        {
            let mut update_mgr = self.update_manager.lock().unwrap();
            *update_mgr = Some(context.update());
        }
        
        let mut update = Update::empty();
        
        // Poll cache update notifications (non-blocking)
        if let Ok(mut rx) = self.cache_update_rx.try_lock() {
            while rx.try_recv().is_ok() {
                update.insert(Update::DRAW);
            }
        }

        if let Some(cursor) = info.cursor_pos {
            self.last_cursor = Some(Point::new(cursor.x, cursor.y));
        }

        // Track viewport width changes to keep height estimation accurate and invalidate cached layouts.
        let current_width = layout.layout.size.width.max(1.0);
        if (current_width - self.last_layout_width).abs() > f32::EPSILON {
            self.last_layout_width = current_width;
            self.layout_cache.clear();
        }

        // Poll thumbnail events
        if let Ok(mut rx) = self.thumbnail_event_rx.try_lock() {
            while let Ok(event) = rx.try_recv() {
                match event {
                    ThumbnailEvent::ThumbnailReady { uri, size, .. } => {
                        // Convert URI to path for pending tracking
                        if let Some(entry_path) = uri_to_path(&uri) {
                            log::debug!("Thumbnail ready for {:?}", entry_path);
                            let mut pending = self.pending_thumbnails.lock().unwrap();
                            pending.remove(&entry_path);
                            
                            // Fetch and cache the thumbnail image (non-blocking spawn)
                            let service_clone = self.thumbnail_service.clone();
                            let cache_clone = self.thumbnail_cache.clone();
                            let path_clone = entry_path.clone();
                            let size_u32 = thumbnail_size_to_u32(size);
                            
                            if let Ok(file) = get_file_for_uri(&uri) {
                                let update_mgr_clone = self.update_manager.clone();
                                let cache_update_tx_clone = self.cache_update_tx.clone();
                                tokio::spawn(async move {
                                    if let Ok(thumbnail_image) = service_clone
                                        .get_thumbnail_image(&*file, size, None)
                                        .await
                                    {
                                        let mut cache = cache_clone.lock().unwrap();
                                        cache.insert((path_clone, size_u32), thumbnail_image);
                                        
                                        // Trigger redraw when thumbnail is cached
                                        if let Ok(mut update_mgr) = update_mgr_clone.lock() {
                                            if let Some(ref update_manager) = *update_mgr {
                                                update_manager.insert(Update::DRAW);
                                            }
                                        }
                                        
                                        // Also send notification via channel (backup mechanism)
                                        let _ = cache_update_tx_clone.send(());
                                    }
                                });
                            }
                            
                            update.insert(Update::DRAW);
                        }
                    },
                    ThumbnailEvent::ThumbnailFailed {
                        uri, error_message, ..
                    } => {
                        if let Some(entry_path) = uri_to_path(&uri) {
                            log::warn!(
                                "Thumbnail generation failed for {:?}: {}",
                                entry_path,
                                error_message
                            );
                            let mut pending = self.pending_thumbnails.lock().unwrap();
                            pending.remove(&entry_path);
                        }
                    },
                }
            }
        }

        if let Some(cursor) = info.cursor_pos {
            let local_y = cursor.y as f32 - layout.layout.location.y;
            let local_x = cursor.x as f32 - layout.layout.location.x;
            let in_bounds = local_x >= 0.0
                && local_x < layout.layout.size.width
                && local_y >= 0.0
                && local_y < layout.layout.size.height;

            let mut index: Option<usize> = None;
            let mut target_path: Option<PathBuf> = None;
            let mut range_paths: Option<Vec<PathBuf>> = None;
            let mut file_type: Option<FileType> = None;

            if in_bounds {
                let view_mode = *self.view_mode.get();
                index = if view_mode == FileListViewMode::Icon {
                    let icon_size = *self.icon_size.get();
                    let (columns, cell_width, cell_height) =
                        self.calculate_icon_view_layout(layout.layout.size.width, icon_size);
                    let col = (local_x / cell_width).floor() as usize;
                    let row = (local_y / cell_height).floor() as usize;
                    let idx = row * columns + col;

                    let entry_opt = {
                        let entries = self.entries.get();
                        if idx < entries.len() {
                            Some(entries[idx].clone())
                        } else {
                            None
                        }
                    };

                    if let Some(entry) = entry_opt {
                        let cell_x = col as f32 * cell_width;
                        let cell_y = row as f32 * cell_height;
                        let cell_rect = Rect::new(
                            cell_x as f64,
                            cell_y as f64,
                            (cell_x + cell_width) as f64,
                            (cell_y + cell_height) as f64,
                        );
                        let is_selected = self.is_selected(&entry.path);
                        let (icon_rect, label_rect, _, _) = self.get_icon_item_layout(
                            &mut info.font_context,
                            &entry,
                            cell_rect,
                            cell_width,
                            icon_size as f32,
                            is_selected,
                        );

                        let cursor_x = local_x as f64;
                        let cursor_y = local_y as f64;

                        if (cursor_x >= icon_rect.x0
                            && cursor_x < icon_rect.x1
                            && cursor_y >= icon_rect.y0
                            && cursor_y < icon_rect.y1)
                            || (cursor_x >= label_rect.x0
                                && cursor_x < label_rect.x1
                                && cursor_y >= label_rect.y0
                                && cursor_y < label_rect.y1)
                        {
                            Some(idx)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else if view_mode == FileListViewMode::Compact {
                    let (columns, cell_width, cell_height, spacing) =
                        self.calculate_compact_view_layout(layout.layout.size.width);
                    let col = ((local_x - self.icon_view_padding) / (cell_width + spacing)).floor()
                        as usize;
                    let row = ((local_y - self.icon_view_padding) / (cell_height + spacing)).floor()
                        as usize;

                    let cell_x = self.icon_view_padding + col as f32 * (cell_width + spacing);
                    let cell_y = self.icon_view_padding + row as f32 * (cell_height + spacing);

                    if local_x >= cell_x
                        && local_x < cell_x + cell_width
                        && local_y >= cell_y
                        && local_y < cell_y + cell_height
                    {
                        let idx = row * columns + col;

                        let entry_opt = {
                            let entries = self.entries.get();
                            if idx < entries.len() {
                                Some(entries[idx].clone())
                            } else {
                                None
                            }
                        };

                        if let Some(entry) = entry_opt {
                            let (mut icon_rect, mut label_rect) = self.get_compact_item_layout(
                                &mut info.font_context,
                                &entry,
                                cell_height,
                                cell_width,
                            );
                            icon_rect = icon_rect + Vec2::new(cell_x as f64, cell_y as f64);
                            label_rect = label_rect + Vec2::new(cell_x as f64, cell_y as f64);

                            let cursor_x = local_x as f64;
                            let cursor_y = local_y as f64;

                            if (cursor_x >= icon_rect.x0
                                && cursor_x < icon_rect.x1
                                && cursor_y >= icon_rect.y0
                                && cursor_y < icon_rect.y1)
                                || (cursor_x >= label_rect.x0
                                    && cursor_x < label_rect.x1
                                    && cursor_y >= label_rect.y0
                                    && cursor_y < label_rect.y1)
                            {
                                Some(idx)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    let idx = (local_y / self.item_height) as usize;
                    let entries = self.entries.get();
                    if idx < entries.len() {
                        Some(idx)
                    } else {
                        None
                    }
                };

                if let Some(index) = index {
                    let entries = self.entries.get();
                    if index < entries.len() {
                        let entry = &entries[index];
                        target_path = Some(entry.path.clone());
                        file_type = Some(entry.file_type);

                        if info.modifiers.shift_key() {
                            let anchor = self.anchor_index.unwrap_or(0);
                            let start = anchor.min(index);
                            let end = anchor.max(index);
                            range_paths = Some(
                                entries[start..=end]
                                    .iter()
                                    .map(|e| e.path.clone())
                                    .collect::<Vec<_>>(),
                            );
                        }
                    }
                }

                if let Some(target_path) = target_path {
                    let ctrl_pressed = info.modifiers.control_key();

                    for (_, btn, el) in &info.buttons {
                        if *btn == MouseButton::Right && *el == ElementState::Pressed {
                            let mut current_selection = self.selected_paths.get().to_vec();
                            if !current_selection.contains(&target_path) {
                                if ctrl_pressed {
                                    current_selection.push(target_path.clone());
                                } else {
                                    current_selection = vec![target_path.clone()];
                                }
                                self.selected_paths.set(current_selection.clone());
                                update.insert(Update::DRAW);
                            }

                            let pending = self.pending_action.clone();
                            let paths_for_action = current_selection.clone();
                            let paths_for_open = paths_for_action.clone();

                            let open_label = self.open_label_for_path(&target_path);

                            // Build menu items using unified system
                            let mut core_items = vec![
                                MenuItem::new(MenuCommand::Custom(0x2001), open_label.clone())
                                    .with_action({
                                        let pending = pending.clone();
                                        let paths_for_open = paths_for_open.clone();
                                        move || {
                                            if let Ok(mut pending_lock) = pending.lock() {
                                                *pending_lock = Some(PendingAction {
                                                    paths: paths_for_open.clone(),
                                                    app_id: None,
                                                    properties: false,
                                                    delete: false,
                                                });
                                            }
                                            Update::DRAW
                                        }
                                    }),
                            ];

                            // Add "Open With" submenu if needed
                            let open_with_items = self.build_open_with_items(&target_path, paths_for_action.clone());
                            if !open_with_items.is_empty() {
                                let open_with_template = MenuTemplate::from_items(
                                    "open_with",
                                    open_with_items,
                                );
                                core_items.push(
                                    MenuItem::new(MenuCommand::Custom(0x2002), "Open With")
                                        .with_submenu(open_with_template),
                                );
                            }

                            // Add Delete item
                            let pending_delete = self.pending_action.clone();
                            let delete_paths = paths_for_action.clone();
                            core_items.push(
                                MenuItem::new(MenuCommand::FileDelete, "Delete")
                                    .with_action(move || {
                                        if let Ok(mut pending_lock) = pending_delete.lock() {
                                            *pending_lock = Some(PendingAction {
                                                paths: delete_paths.clone(),
                                                app_id: None,
                                                properties: false,
                                                delete: true,
                                            });
                                        }
                                        Update::DRAW
                                    }),
                            );

                            // Add Properties item
                            let pending_props = self.pending_action.clone();
                            let props_paths = paths_for_action.clone();
                            core_items.push(
                                MenuItem::new(MenuCommand::Custom(0x2006), "Properties")
                                    .with_action(move || {
                                        if let Ok(mut pending_lock) = pending_props.lock() {
                                            *pending_lock = Some(PendingAction {
                                                paths: props_paths.clone(),
                                                app_id: None,
                                                properties: true,
                                                delete: false,
                                            });
                                        }
                                        Update::DRAW
                                    }),
                            );

                            // Build groups with separators
                            let mut all_items = core_items;
                            all_items.push(MenuItem::separator());
                            all_items.push(
                                MenuItem::new(MenuCommand::Custom(0x2003), "Share (placeholder)")
                                    .with_action(|| Update::empty()),
                            );
                            all_items.push(MenuItem::separator());
                            all_items.push(
                                MenuItem::new(MenuCommand::Custom(0x2004), "Extensions (placeholder)")
                                    .with_action(|| Update::empty()),
                            );
                            all_items.push(MenuItem::separator());
                            all_items.push(
                                MenuItem::new(MenuCommand::Custom(0x2005), "View options (placeholder)")
                                    .with_action(|| Update::empty()),
                            );

                            let menu_template = MenuTemplate::from_items("file_context_menu", all_items);
                            
                            if let Some(cursor_pos) = info.cursor_pos {
                                let cursor = Point::new(cursor_pos.x, cursor_pos.y);
                                context.menu_manager.show(menu_template, cursor);
                                update.insert(Update::DRAW);
                            }
                        }

                        if *btn == MouseButton::Left && *el == ElementState::Pressed {
                            let mut selected = self.selected_paths.get().clone();
                            let is_currently_selected = selected.contains(&target_path);

                            if let Some(range_paths) = &range_paths {
                                if ctrl_pressed {
                                    let mut selected_set: HashSet<PathBuf> =
                                        selected.iter().cloned().collect();
                                    for path in range_paths {
                                        selected_set.insert(path.clone());
                                    }
                                    selected = selected_set.into_iter().collect();
                                } else {
                                    selected = range_paths.clone();
                                }
                            } else if ctrl_pressed {
                                if is_currently_selected {
                                    selected.retain(|p| p != &target_path);
                                } else {
                                    selected.push(target_path.clone());
                                }
                                self.anchor_index = Some(index.unwrap_or(0));
                            } else {
                                selected = vec![target_path.clone()];
                                self.anchor_index = Some(index.unwrap_or(0));
                            }

                            self.selected_paths.set(selected);
                            update.insert(Update::DRAW);

                            let now = Instant::now();
                            if let Some(last_time) = self.last_click_time {
                                if let Some(last_index) = self.last_click_index {
                                    if Some(last_index) == index
                                        && now.duration_since(last_time)
                                            < Duration::from_millis(500)
                                    {
                                        if let Some(ftype) = file_type {
                                            if ftype == FileType::Directory {
                                                self.current_path.set(target_path.clone());
                                                let _ = self.fs_model.refresh(&target_path);
                                                self.selected_paths.set(Vec::new());
                                                update.insert(Update::LAYOUT);
                                            }
                                        }
                                    }
                                }
                            }

                            self.last_click_time = Some(now);
                            self.last_click_index = index;
                        }
                    }
                } else {
                    for (_, btn, el) in &info.buttons {
                        if *btn == MouseButton::Left && *el == ElementState::Pressed {
                            self.drag_start = Some(Point::new(local_x as f64, local_y as f64));
                            self.current_drag_pos =
                                Some(Point::new(local_x as f64, local_y as f64));
                            self.is_dragging = false;

                            if !info.modifiers.control_key() {
                                self.selected_paths.set(Vec::new());
                                update.insert(Update::DRAW);
                            }
                        }
                    }
                }
            }

            // Drag handling outside bounds to keep tracking when cursor leaves the window.
            if let Some(start_pos) = self.drag_start {
                let mut released = false;
                for (_, btn, el) in &info.buttons {
                    if *btn == MouseButton::Left && *el == ElementState::Released {
                        released = true;
                        break;
                    }
                }

                if released {
                    self.drag_start = None;
                    self.current_drag_pos = None;
                    self.is_dragging = false;
                    update.insert(Update::DRAW);
                } else {
                    let current_pos = Point::new(local_x as f64, local_y as f64);
                    self.current_drag_pos = Some(current_pos);

                    if !self.is_dragging {
                        let dx = current_pos.x - start_pos.x;
                        let dy = current_pos.y - start_pos.y;
                        if dx.abs() > 5.0 || dy.abs() > 5.0 {
                            self.is_dragging = true;
                        }
                    }

                    if self.is_dragging {
                        let min_x = start_pos.x.min(current_pos.x);
                        let min_y = start_pos.y.min(current_pos.y);
                        let max_x = start_pos.x.max(current_pos.x);
                        let max_y = start_pos.y.max(current_pos.y);

                        let selection_rect = Rect::new(min_x, min_y, max_x, max_y);

                        self.update_drag_selection(
                            selection_rect,
                            info.modifiers.control_key(),
                            layout.layout.size.width,
                        );
                        update.insert(Update::DRAW);
                    }
                }
            }
        }

        // Check menu state to detect when menu closes
        let menu_is_open = context.menu_manager.is_open();
        
        // Process any pending action set by context menu callbacks.
        // Menu item actions set pending_action when clicked, and we should process it
        // immediately (menu item actions return Update::DRAW which triggers this update cycle).
        // However, we need to prevent stale actions from being processed when clicking elsewhere.
        // Strategy: Only process pending_action if:
        // 1. Menu is currently open (action was just triggered by menu item click), OR
        // 2. Menu just closed AND we had a pending action (menu item was clicked before menu closed)
        // Clear pending_action if menu closes without an action being processed
        let should_process = menu_is_open || (self.menu_was_open && !menu_is_open);
        
        if should_process {
            if let Ok(mut pending) = self.pending_action.lock() {
                if let Some(action) = pending.take() {
                    // Action was set - process it immediately
                    if action.delete {
                        // Delete action - show confirmation dialog first
                        self.show_delete_confirmation_dialog(&action.paths, context);
                        update.insert(Update::DRAW);
                    } else if let Some(app_id) = action.app_id {
                        for path in action.paths.iter() {
                            if let Err(err) = self.mime_registry.launch(&app_id, path) {
                                log::warn!(
                                    "Failed to launch {} with {}: {}",
                                    path.display(),
                                    app_id,
                                    err
                                );
                            }
                        }
                    } else if action.properties {
                        self.show_properties_popup(&action.paths, context);
                    } else {
                        if action.paths.len() == 1 {
                            let path = &action.paths[0];
                            if path.is_dir() {
                                self.current_path.set(path.clone());
                                let _ = self.fs_model.refresh(path);
                                self.selected_paths.set(Vec::new());
                                update.insert(Update::LAYOUT | Update::DRAW);
                            } else {
                                FileListContent::launch_path(self.mime_registry.clone(), path.clone());
                            }
                        } else {
                            // Multi-selection: launch all files, skip directories.
                            for path in action.paths.iter() {
                                if path.is_dir() {
                                    continue;
                                }
                                FileListContent::launch_path(self.mime_registry.clone(), path.clone());
                            }
                        }
                    }
                } else if !menu_is_open && self.menu_was_open {
                    // Menu closed without pending action - this is normal (menu closed without item click)
                    // No action to process or clear
                }
            }
        } else if !menu_is_open {
            // Menu is not open and wasn't open before - clear any stale pending_action
            // This prevents actions from being processed when clicking elsewhere after menu was closed
            if let Ok(mut pending) = self.pending_action.lock() {
                if pending.is_some() {
                    log::debug!("Clearing stale pending_action - no menu context");
                    *pending = None;
                }
            }
        }
        
        self.menu_was_open = menu_is_open;

        // Process confirmed delete operations (user clicked "Delete" in confirmation dialog)
        if let Ok(mut pending_delete) = self.pending_delete_confirmation.lock() {
            if let Some(paths) = pending_delete.take() {
                // User confirmed - proceed with deletion
                if let Some(ref op_tx) = self.operation_tx {
                    if let Err(e) = op_tx.send(FileListOperation::Delete(paths.clone())) {
                        log::error!("Failed to send delete operation: {}", e);
                    } else {
                        log::info!("Delete operation confirmed and sent for paths: {:?}", paths);
                        update.insert(Update::DRAW);
                    }
                } else {
                    log::warn!("Delete confirmed but no operation channel available");
                }
            }
        }

        update
    }

    fn render(
        &mut self,
        graphics: &mut dyn Graphics,
        theme: &mut dyn Theme,
        layout: &LayoutNode,
        info: &mut AppInfo,
        _context: AppContext,
    ) {
        // DEBUG: Log render calls to investigate frequency
        // use std::sync::atomic::{AtomicU64, Ordering};
        // static RENDER_COUNTER: AtomicU64 = AtomicU64::new(0);
        // let count = RENDER_COUNTER.fetch_add(1, Ordering::Relaxed);
        // if count % 60 == 0 {
        //     // println!("FileList render called {} times", count);
        // }

        let view_mode = *self.view_mode.get();

        if view_mode == FileListViewMode::Icon {
            self.render_icon_view(graphics, theme, layout, info);
        } else if view_mode == FileListViewMode::Compact {
            self.render_compact_view(graphics, theme, layout, info);
        } else {
            self.render_list_view(graphics, theme, layout, info);
        }

        // Draw drag selection rectangle
        if self.is_dragging {
            if let (Some(start), Some(current)) = (self.drag_start, self.current_drag_pos) {
                let min_x = start.x.min(current.x);
                let min_y = start.y.min(current.y);
                let max_x = start.x.max(current.x);
                let max_y = start.y.max(current.y);

                let rect = Rect::new(min_x, min_y, max_x, max_y);

                let selection_color = theme
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

                // Draw selection fill
                graphics.fill(
                    Fill::NonZero,
                    Affine::translate((
                        layout.layout.location.x as f64,
                        layout.layout.location.y as f64,
                    )),
                    &Brush::Solid(selection_color.with_alpha(0.2)),
                    None,
                    &rect.to_path(0.1),
                );

                // Draw selection border
                graphics.stroke(
                    &Stroke::new(1.0),
                    Affine::translate((
                        layout.layout.location.x as f64,
                        layout.layout.location.y as f64,
                    )),
                    &Brush::Solid(selection_color.with_alpha(0.8)),
                    None,
                    &rect.to_path(0.1),
                );
            }
        }
    }
}

