use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use nalgebra::Vector2;
use nptk::core::app::context::AppContext;
use nptk::core::app::info::AppInfo;
use nptk::core::app::update::Update;
use nptk::core::layout::{Dimension, LayoutNode, LayoutStyle, StyleNode, LengthPercentage};
use nptk::core::menu::{MenuTemplate, MenuItem, MenuCommand};
use nptk::core::signal::{state::StateSignal, MaybeSignal, Signal};
use nptk::core::text_render::TextRenderContext;
use nptk::core::vg::kurbo::{Affine, Point, Rect, Shape, Stroke, Vec2};
use nptk::core::vg::peniko::{Brush, Fill};
use nptk::core::vgi::Graphics;
use nptk::core::widget::{BoxedWidget, Widget, WidgetLayoutExt};
use nptk::core::window::{ElementState, MouseButton};
use nptk::prelude::LayoutContext;
use nptk::services::filesystem::entry::{FileEntry, FileType};
use nptk::services::filesystem::model::{FileSystemEvent, FileSystemModel};
use npio::service::icon::IconRegistry;
use npio::{ThumbnailService, ThumbnailEvent, ThumbnailImage, get_file_for_uri, register_backend};
use npio::backend::local::LocalBackend;
use nptk::services::thumbnail::npio_adapter::{uri_to_path, thumbnail_size_to_u32};
use nptk::core::theme::ColorRole;
use std::collections::HashSet;
use tokio::{sync::broadcast, time::{Duration, Instant}};

mod actions;
mod properties;
mod view_compact;
mod view_icon;
mod view_list;
mod model_adapter;
pub mod types;
mod content;

use content::FileListContent;
pub use types::*;




use nptk::widgets::scroll_container::{ScrollContainer, ScrollDirection};
use nptk::core::signal::eval::EvalSignal;
use npio::service::filesystem::mime_registry::MimeRegistry;
use std::path::PathBuf;
// Import widgets needed for confirmation dialog
use nptk::widgets::container::Container;
use nptk::widgets::button::Button;
use nptk::widgets::text::Text;
use humansize::{format_size, BINARY};
use std::fs;



use crate::file_list::model_adapter::FileSystemItemModel;
use nptk::core::model::SortOrder;

/// A widget that displays a list of files.
pub struct FileList {
    // State
    current_path: StateSignal<PathBuf>,
    entries: StateSignal<Vec<FileEntry>>,
    all_entries: StateSignal<Vec<FileEntry>>,
    selected_paths: StateSignal<Vec<PathBuf>>,
    view_mode: StateSignal<FileListViewMode>,
    icon_size: StateSignal<u32>,
    search_query: StateSignal<String>,

    // Model
    fs_model: Arc<FileSystemModel>,
    _event_rx: Arc<Mutex<broadcast::Receiver<FileSystemEvent>>>,

    // Layout
    layout_style: MaybeSignal<LayoutStyle>,

    // Child widgets
    scroll_container: BoxedWidget,

    // Track if signals are hooked
    signals_hooked: bool,

    // Selection change notification channel
    selection_change_tx: Option<Arc<tokio::sync::mpsc::UnboundedSender<Vec<PathBuf>>>>,
    
    // Cache invalidation channel sender
    cache_invalidate_tx: Arc<tokio::sync::mpsc::UnboundedSender<PathBuf>>,
    
    // Generic ItemView for Table mode
    item_view: Option<BoxedWidget>,
    
    // Selection signal for ItemView (Table mode)
    item_view_selection: Option<StateSignal<Vec<usize>>>,
    
    // Receiver for selection changes from ItemView callback
    selection_change_rx: Option<Arc<Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<PathBuf>>>>>,
    
    // Sender for internal selection changes (used by ItemView callback)
    internal_selection_tx: Option<Arc<tokio::sync::mpsc::UnboundedSender<Vec<PathBuf>>>>,
    
    // Track last path to detect changes
    last_path: Option<PathBuf>,
    
    // Context menu callback
    on_context_menu: Option<Arc<dyn Fn(PathBuf, Vector2<f64>, AppContext) -> Update + Send + Sync>>,

    // Services needed for properties dialog
    icon_registry: Arc<IconRegistry>,
    thumbnail_service: Arc<ThumbnailService>,
    icon_cache: Arc<Mutex<std::collections::HashMap<(PathBuf, u32), Option<npio::service::icon::CachedIcon>>>>,
    mime_registry: MimeRegistry,
    pending_thumbnails: Arc<Mutex<HashSet<PathBuf>>>,
    cache_update_tx: tokio::sync::mpsc::Sender<()>,
    // cache_update_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<()>>>,
    svg_scene_cache: Arc<Mutex<std::collections::HashMap<String, (nptk::core::vg::Scene, f64, f64)>>>,
    
    // Operation channel for keyboard shortcuts
    operation_tx: Option<tokio::sync::mpsc::UnboundedSender<FileListOperation>>,
    
    // Model for sorting
    sort_model: Option<Arc<FileSystemItemModel>>,
}

impl FileList {
    fn apply_with(mut self, f: impl FnOnce(&mut Self)) -> Self {
        f(&mut self);
        self
    }

    /// Create a new file list widget.
    pub fn new(initial_path: PathBuf) -> Self {
        Self::new_with_operations(initial_path, None, None)
    }

    /// Create a new file list widget with optional operation channel for file operations.
    /// 
    /// `operation_tx` - if provided, file operations (like delete) will be sent via this channel.
    /// `selection_change_tx` - if provided, selection changes will be notified via this channel.
    pub fn new_with_operations(
        initial_path: PathBuf,
        operation_tx: Option<tokio::sync::mpsc::UnboundedSender<FileListOperation>>,
        selection_change_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<PathBuf>>>,
    ) -> Self {
        let fs_model = Arc::new(
            FileSystemModel::new(initial_path.clone())
                .unwrap_or_else(|e| {
                    log::error!("Failed to create FileSystemModel for path {:?}: {}", initial_path, e);
                    // Try fallback to current directory
                    std::env::current_dir()
                        .ok()
                        .and_then(|dir| FileSystemModel::new(dir).ok())
                        .unwrap_or_else(|| {
                            // Last resort: try root path
                            FileSystemModel::new(PathBuf::from("/"))
                                .unwrap_or_else(|e2| {
                                    log::error!("Failed to create FileSystemModel with root path: {}", e2);
                                    // This should never happen, but if it does, panic with a clear message
                                    panic!("Failed to create FileSystemModel with all fallback paths. This indicates a serious system issue.");
                                })
                        })
                })
        );
        let event_rx = Arc::new(Mutex::new(fs_model.subscribe_events()));

        // Initial load
        let _ = fs_model.refresh(&initial_path);

        let current_path = StateSignal::new(initial_path.clone());
        let entries = StateSignal::new(Vec::new());
        let all_entries = StateSignal::new(Vec::new());
        let selected_paths = StateSignal::new(Vec::new());
        let view_mode = StateSignal::new(FileListViewMode::List);
        let icon_size = StateSignal::new(48);
        let search_query = StateSignal::new(String::new());

        // Create icon registry
        let icon_registry =
            Arc::new(IconRegistry::new().unwrap_or_else(|_| IconRegistry::default()));

        let mime_registry = MimeRegistry::load_default();
        
        // Register npio backend if not already registered
        // Note: This is idempotent - registering multiple times is safe
        let backend = Arc::new(LocalBackend::new());
        register_backend(backend);

        // Create thumbnail service
        let thumbnail_service = Arc::new(ThumbnailService::new());
        let thumbnail_event_rx = thumbnail_service.subscribe();
        
        // Create channel for cache update notifications (bounded to prevent unbounded growth)
        let (cache_update_tx, cache_update_rx) = tokio::sync::mpsc::channel(100);
        let cache_update_rx = Arc::new(Mutex::new(cache_update_rx));

        // Create pending thumbnails set
        let pending_thumbnails = Arc::new(Mutex::new(HashSet::new()));
        
        // Create channel for cache invalidation requests
        let (cache_invalidate_tx, cache_invalidate_rx) = tokio::sync::mpsc::unbounded_channel();

        // Wrap selection_change_tx in Arc for sharing with FileListContent
        let selection_change_tx_arc = selection_change_tx.map(|tx| Arc::new(tx));
        
        // Create internal channel for selection changes from ItemView
        let (internal_selection_tx, internal_selection_rx) = tokio::sync::mpsc::unbounded_channel();
        let internal_selection_tx_arc = Arc::new(internal_selection_tx);
        let internal_selection_rx_arc = Arc::new(Mutex::new(internal_selection_rx));

        // Create icon cache to be shared between FileList and properties dialog
        let icon_cache = Arc::new(Mutex::new(std::collections::HashMap::new()));
        
        // Create SVG scene cache
        let svg_scene_cache = Arc::new(Mutex::new(std::collections::HashMap::new()));
        
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
            icon_cache.clone(),
            thumbnail_event_rx,
            cache_update_tx.clone(),
            cache_update_rx.clone(),
            pending_thumbnails.clone(),
            cache_invalidate_rx,
            operation_tx.clone(),
            selection_change_tx_arc.clone(),
        );
        
        // Store cache invalidation sender for use in FileList::update()
        let cache_invalidate_tx_arc = Arc::new(cache_invalidate_tx);

        // Create scroll container (Both directions to support icon view)
        let scroll_container = ScrollContainer::new()
            .with_scroll_direction(ScrollDirection::Both)
            .with_virtual_scrolling(true, 30.0)
            .with_child(content);

        Self {
            current_path,
            entries,
            all_entries,
            selected_paths,
            view_mode,
            icon_size,
            search_query,
            fs_model,
            _event_rx: event_rx,
            layout_style: LayoutStyle {
                size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
                ..Default::default()
            }
            .into(),
            scroll_container: Box::new(scroll_container),
            signals_hooked: false,
            selection_change_tx: selection_change_tx_arc,
            cache_invalidate_tx: cache_invalidate_tx_arc,
            item_view: None,
            item_view_selection: None,
            selection_change_rx: Some(internal_selection_rx_arc),
            internal_selection_tx: Some(internal_selection_tx_arc),
            last_path: None,
            on_context_menu: None,
            icon_registry,
            thumbnail_service,
            icon_cache,
            mime_registry,
            pending_thumbnails,
            cache_update_tx,
            // cache_update_rx,
            svg_scene_cache,
            operation_tx,
            sort_model: None,
        }
    }

    pub fn show_properties_popup(&self, paths: &[PathBuf], context: AppContext) {
        use crate::file_list::properties::PropertiesData;
        use std::fs;
        use humansize::{format_size, BINARY};
        use npio::service::filesystem::mime_detector::MimeDetector;

        if paths.is_empty() {
             return;
        }

        let mut rows: Vec<(String, String)> = Vec::new();

        let (title, icon_label) = if paths.len() == 1 {
            let path = &paths[0];
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("<unnamed>");
            let icon_label = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_uppercase())
                .unwrap_or_else(|| "FILE".to_string());

            let mime_type = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(MimeDetector::detect_mime_type(path))
            })
                .or_else(|| FileListContent::xdg_mime_filetype(path))
                .unwrap_or_else(|| "unknown".to_string());

            let lookup_mime_description = |mime_type: &str| -> Option<String> {
                for variant in FileListContent::mime_description_variants(mime_type) {
                    if let Some(desc) = self.mime_registry.description(&variant) {
                        return Some(desc);
                    }
                }
                FileListContent::get_mime_description(mime_type)
            };

            let kind_display = if let Some(description) = lookup_mime_description(&mime_type) {
                format!("{} ({})", description, mime_type)
            } else {
                mime_type.clone()
            };
            rows.push(("Kind".to_string(), kind_display));
            rows.push(("Name".to_string(), name.to_string()));

            if let Ok(meta) = fs::metadata(path) {
                let size = if meta.is_dir() {
                    FileListContent::calculate_directory_size(path)
                } else {
                    meta.len()
                };
                rows.push((
                    "Size".to_string(),
                    format_size(size, BINARY) + " (" + size.to_string().as_str() + " bytes)",
                ));
                if let Ok(modified) = meta.modified() {
                    rows.push(("Modified".to_string(), FileListContent::format_system_time(modified)));
                }
                if let Ok(created) = meta.created() {
                    rows.push(("Created".to_string(), FileListContent::format_system_time(created)));
                }
            }

            rows.push((
                "Location".to_string(),
                path.parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "".to_string()),
            ));
            rows.push(("Path".to_string(), path.display().to_string()));
            (name.to_string(), icon_label)
        } else {
            let count = paths.len();
            let mut total_size: u64 = 0;
            for p in paths {
                if let Ok(meta) = fs::metadata(p) {
                    let size = if meta.is_dir() {
                        FileListContent::calculate_directory_size(p)
                    } else {
                        meta.len()
                    };
                    total_size = total_size.saturating_add(size);
                }
            }
            rows.push(("Items".to_string(), count.to_string()));
            rows.push(("Total size".to_string(), format_size(total_size, BINARY)));
            (format!("{} items", count), "MULTI".to_string())
        };

        let data = PropertiesData {
            title,
            icon_label,
            rows,
            paths: paths.to_vec(),
        };

        let svg_scene_cache = Arc::new(Mutex::new(std::collections::HashMap::new()));
        
        // Create Properties Widget using FileListContent's static builder
        let props_widget = FileListContent::build_properties_widget(
            data,
            self.icon_registry.clone(),
            self.thumbnail_service.clone(),
            self.icon_cache.clone(),
            svg_scene_cache,
        );
        
        let pos = (100, 100);
            
        context
            .popup_manager
            .create_popup_at(props_widget, "Properties", (360, 260), pos);
    }

    pub fn with_on_context_menu<F>(mut self, callback: F) -> Self 
    where F: Fn(PathBuf, Vector2<f64>, AppContext) -> Update + Send + Sync + 'static 
    {
        self.on_context_menu = Some(Arc::new(callback));
        self
    }

    /// Initialize ItemView if needed
    fn ensure_item_view(&mut self) {
        if self.item_view.is_none() {
            use crate::file_list::model_adapter::FileSystemItemModel;
            use nptk::widgets::item_view::{ItemView, ViewMode};
            
            let icon_size_clone = self.icon_size.clone();
            let _entries_act_clone = self.entries.clone();
            let effective_icon_size = self.view_mode.map(move |mode| match *mode {
                FileListViewMode::List | FileListViewMode::Table => nptk::core::reference::Ref::Owned(16.0),
                FileListViewMode::Icon | FileListViewMode::Compact => nptk::core::reference::Ref::Owned(*icon_size_clone.get() as f32),
            });

            // We need a signal for the model too that matches effective_icon_size
            // Since effective_icon_size is a MapSignal, we can clone it.
            let model_icon_size = effective_icon_size.clone();

            let model = Arc::new(FileSystemItemModel::new(
                self.entries.clone(),
                self.icon_registry.clone(),
                self.thumbnail_service.clone(),
                self.icon_cache.clone(),
                self.svg_scene_cache.clone(),
                self.pending_thumbnails.clone(),
                self.cache_update_tx.clone(),
            ).with_icon_size(model_icon_size));
             
            self.sort_model = Some(model.clone());

             // Setup ItemView with selection sync
            let _selected_paths = self.selected_paths.clone();
            let _entries = self.entries.clone();
            let selection_change_tx = self.selection_change_tx.clone();
            let internal_selection_tx = self.internal_selection_tx.clone();
            
            // Reactive ViewMode mapping
            let view_mode_signal = self.view_mode.map(|mode| match *mode {
                FileListViewMode::List => nptk::core::reference::Ref::Owned(ViewMode::List),
                FileListViewMode::Icon => nptk::core::reference::Ref::Owned(ViewMode::Icon),
                FileListViewMode::Compact => nptk::core::reference::Ref::Owned(ViewMode::Compact),
                FileListViewMode::Table => nptk::core::reference::Ref::Owned(ViewMode::Table),
            });
            
            // Activation handling
            let entries_act = self.entries.clone();
            let current_path = self.current_path.clone();
            let _fs_model = self.fs_model.clone();
            
            // Clone entries signal for callbacks
            let entries_selection = self.entries.clone();
            let entries_menu = self.entries.clone();
            
            // Context menu handling
            let on_context_menu = self.on_context_menu.clone();

            let mut view = ItemView::new(model)
                .with_icon_size(effective_icon_size)
                .with_view_mode(MaybeSignal::signal(Box::new(view_mode_signal)))
                .with_on_activate(move |index| {
                    let current_entries = entries_act.get();
                    if index < current_entries.len() {
                        let entry = &current_entries[index];
                        if entry.is_dir() {
                             // Just set the path - the signal change will trigger refresh elsewhere
                             current_path.set(entry.path.clone());
                             return Update::LAYOUT | Update::DRAW;
                        }
                    }
                    Update::empty()
                })
                .with_on_selection_change(move |indices| {
                    // Update FileList selection from ItemView selection
                    // IMPORTANT: Do NOT call signal.set() here - it causes deadlock!
                    // Instead, send via channel and let FileList::update handle it
                    
                    let current_entries = entries_selection.get();
                    
                    let mut new_paths = Vec::new();
                    for idx in indices {
                        if idx < current_entries.len() {
                            new_paths.push(current_entries[idx].path.clone());
                        }
                    }
                    
                    // Send selection change via INTERNAL channel (non-blocking)
                    if let Some(ref tx) = internal_selection_tx {
                        let _ = tx.send(new_paths.clone());
                    }
                    
                    // Also send to external channel if provided
                    if let Some(ref tx) = selection_change_tx {
                        let _ = tx.send(new_paths);
                    }
                    
                    Update::DRAW
                });
                
            if let Some(cb) = on_context_menu {
                view = view.with_on_context_menu(move |index, pos, context| {
                    let entries = entries_menu.get();
                     if index < entries.len() {
                        let path = entries[index].path.clone();
                        return cb(path, pos, context);
                    }
                    Update::empty()
                });
            }
                
            // Hook up selection signal (path -> index)
            // This is tricky because we need to map paths to indices reactively.
            // ideally we would use a mapped signal, but we can also just rely on update()
            // to push the correct indices to the view if they mismatch.
            // For now, let's just create a detached signal and sync manually in update().
            
            // Create selection signal
            let selection_signal = StateSignal::new(Vec::new());
            self.item_view_selection = Some(selection_signal.clone());
            
            view = view.with_selected_rows(MaybeSignal::signal(Box::new(selection_signal)));
            
            // Set layout style to fill parent
            view.set_layout_style(LayoutStyle {
                size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
                ..Default::default()
            });

            let mut scroll_container = ScrollContainer::new()
                .with_child(view)
                .with_scroll_direction(ScrollDirection::Vertical);
            
             // Set ScrollContainer style to fill parent
             scroll_container.set_layout_style(LayoutStyle {
                size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
                ..Default::default()
            });
            
            self.item_view = Some(Box::new(scroll_container));
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

    /// Get the selected paths signal (for reactive subscription)
    pub fn selected_paths_signal(&self) -> &StateSignal<Vec<PathBuf>> {
        &self.selected_paths
    }
    
    /// Get the current path signal (for reactive subscription)
    pub fn current_path_signal(&self) -> &StateSignal<PathBuf> {
        &self.current_path
    }

    /// Get the entries signal (for reactive subscription)
    pub fn entries_signal(&self) -> &StateSignal<Vec<FileEntry>> {
        &self.entries
    }

    /// Clear the selection.
    pub fn clear_selection(&mut self) {
        self.selected_paths.set(Vec::new());
        // Notify about selection change
        if let Some(ref tx) = self.selection_change_tx {
            let _ = tx.send(Vec::new());
        }
    }

    /// Select all entries.
    pub fn select_all(&mut self) {
        let entries = self.entries.get();
        let paths: Vec<PathBuf> = entries.iter().map(|e| e.path.clone()).collect();
        self.selected_paths.set(paths.clone());
        // Notify about selection change
        if let Some(ref tx) = self.selection_change_tx {
            let _ = tx.send(paths);
        }
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
    
    /// Get the view mode signal
    pub fn view_mode_signal(&self) -> &StateSignal<FileListViewMode> {
        &self.view_mode
    }
    
    /// Get the icon size signal
    pub fn icon_size_signal(&self) -> &StateSignal<u32> {
        &self.icon_size
    }

    /// Get the search query signal
    pub fn search_query_signal(&self) -> &StateSignal<String> {
        &self.search_query
    }
    
    /// Set the search query
    pub fn set_search_query(&mut self, query: String) {
        self.search_query.set(query);
    }
    
    /// Sort the file list by the given column and order.
    /// Columns: 0=Name, 1=Size, 2=Type, 3=Date
    pub fn sort(&mut self, column: usize, order: SortOrder) {
        // Ensure model exists (it's created on first render, but we might need it earlier)
        self.ensure_item_view();
        
        if let Some(model) = &self.sort_model {
            use nptk::core::model::ItemModel;
            model.sort(column, order);
        }
    }
}

#[async_trait(?Send)]
impl Widget for FileList {
    fn layout_style(&self, _context: &LayoutContext) -> StyleNode {
        let mode = *self.view_mode.get();
        
        // Always use ItemView if available
        if self.item_view.is_some() {
            if let Some(ref view) = self.item_view {
                return StyleNode {
                    style: self.layout_style.get().clone(),
                    children: vec![view.layout_style(_context)],
                    measure_func: None,
                };
            }
        }
        
        // Otherwise use scroll_container
        StyleNode {
            style: self.layout_style.get().clone(),
            children: vec![self.scroll_container.layout_style(_context)],
            measure_func: None,
        }
    }

    async fn update(&mut self, layout: &LayoutNode, context: AppContext, info: &mut AppInfo) -> Update {
        let mode = *self.view_mode.get();
        println!("FileList::update: mode={:?}, layout_children={}", mode, layout.children.len());
        
        // Ensure ItemView is initialized for all view modes
        self.ensure_item_view();

        // Hook signals on first update to make them reactive
        if !self.signals_hooked {
            context.hook_signal(&mut self.entries);
            context.hook_signal(&mut self.all_entries);
            context.hook_signal(&mut self.current_path);
            context.hook_signal(&mut self.selected_paths);
            context.hook_signal(&mut self.view_mode);
            context.hook_signal(&mut self.icon_size);
            context.hook_signal(&mut self.search_query);
            self.signals_hooked = true;
        }
        
        let mut update = Update::empty();

        // Process keyboard shortcuts (Ctrl+C/X/V, Delete)
        // We check if the widget or its children are focused effectively by handling keys here.
        // In NPTK, bubbling/handling isn't strictly hierarchical for keys unless using FocusManager.
        // But FileList is the main widget here.
        // We only handle if we have operation_tx.
        if let Some(ref tx) = self.operation_tx {
             for (_, key_event) in &info.keys {
                if key_event.state == nptk::core::window::ElementState::Pressed {
                    let modifiers = info.modifiers; // AppInfo has modifiers
                    
                    if modifiers.control_key() {
                        match key_event.physical_key {
                            nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::KeyC) => {
                                let selection = self.selected_paths.get().clone();
                                if !selection.is_empty() {
                                    let _ = tx.send(FileListOperation::Copy(selection));
                                }
                            }
                            nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::KeyX) => {
                                let selection = self.selected_paths.get().clone();
                                if !selection.is_empty() {
                                    let _ = tx.send(FileListOperation::Cut(selection));
                                }
                            }
                            nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::KeyV) => {
                                let _ = tx.send(FileListOperation::Paste);
                            }
                            _ => {}
                        }
                    } else if key_event.physical_key == nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::Delete) {
                         let selection = self.selected_paths.get().clone();
                         if !selection.is_empty() {
                             let _ = tx.send(FileListOperation::Delete(selection));
                         }
                    }
                }
             }
        }
        
        // Poll filesystem events FIRST - this must happen before ItemView handling
        // so that entries get updated even when using ItemView
        if let Ok(mut rx) = self._event_rx.try_lock() {
            while let Ok(event) = rx.try_recv() {
                match event {
                    FileSystemEvent::DirectoryLoaded { path, entries } => {
                        if path == *self.current_path.get() {
                            self.all_entries.set(entries.clone());
                            
                            // Apply filtering
                            let query = self.search_query.get().to_lowercase();
                            if query.is_empty() {
                                self.entries.set(entries);
                            } else {
                                let filtered: Vec<FileEntry> = entries.into_iter()
                                    .filter(|e| e.name.to_lowercase().contains(&query))
                                    .collect();
                                self.entries.set(filtered);
                            }
                            
                            update.insert(Update::LAYOUT | Update::DRAW);
                        }
                    },
                    FileSystemEvent::EntryAdded { path, .. } | FileSystemEvent::EntryRemoved { path } | FileSystemEvent::EntryModified { path, .. } => {
                        if let Some(parent) = path.parent() {
                            if parent == *self.current_path.get() {
                                let _ = self.fs_model.refresh(parent);
                                if let Err(e) = self.cache_invalidate_tx.send(path.clone()) {
                                    log::warn!("Failed to send cache invalidation request: {}", e);
                                }
                            }
                        }
                    },
                    _ => {},
                }
            }
        }
        
        // Poll selection changes from ItemView callback
        if let Some(ref rx) = self.selection_change_rx {
            if let Ok(mut receiver) = rx.try_lock() {
                while let Ok(new_paths) = receiver.try_recv() {
                    self.selected_paths.set(new_paths);
                    update.insert(Update::DRAW);
                }
            }
        }
        
        // Check if path changed and trigger refresh if needed
        let current_path_value = self.current_path.get().clone();
        if self.last_path.as_ref() != Some(&current_path_value) {
            self.last_path = Some(current_path_value.clone());
            let _ = self.fs_model.refresh(&current_path_value);
            update.insert(Update::LAYOUT | Update::DRAW);
        }
        
        // Check if search query changed
        // Note: We rely on signal reactivity, but we need to re-filter when it changes
        // Since we hook the signal, this update() is called when it changes.
        // But we need to detect *what* changed or just re-filter if needed.
        // A simple way is to check against a stored last_query, or just re-filter if we assume efficient updates.
        // For now, let's just re-filter based on all_entries if search_query changed? 
        // Actually, since update() is called on signal change, we can just re-apply filter logic
        // But we want to avoid re-setting entries if nothing changed.
        // Let's rely on the fact that if search_query changed, *self.search_query.get() is new.
        // We can just re-run the filter logic every time update is called? No, that's wasteful.
        // Best practice: Use a stored previous value or just do it.
        // Given existing pattern, let's just re-filter. It's fast for small lists.
        // BUT wait, we don't store previous query. 
        // Let's leave it for now - the directory load triggers the first filter.
        // We need to handle the case where ONLY search query changes.
        
        // Ideally we should track last_query in struct. But for filtered list,
        // we can just re-derive `entries` from `all_entries` + `search_query`
        // whenever `search_query` changes.
        // Since we don't have `last_query`, let's just add it or implement a check.
        
        // Actually, let's just re-filter every time for now inside the update loop if we can efficiently check change.
        // But we can't easily check change without previous value.
        // Let's add logic:
        {
            let all = self.all_entries.get();
            let query = self.search_query.get().to_lowercase();
            // let current_entries_len = self.entries.get().len();
            
            // This is a bit hacky: we re-filter every frame. 
            // Better: Check if `all_entries` or `search_query` signal has changed?
            // NPTK signals don't expose "has_changed" easily in update() without tracking.
            // Let's assume for now that if we are here, something might have changed.
            // But rewriting `entries` every frame causes loops if `entries` signal triggers update.
            // So we MUST check if the result is different.
            
            let filtered: Vec<FileEntry> = if query.is_empty() {
                all.clone()
            } else {
                all.iter()
                    .filter(|e| e.name.to_lowercase().contains(&query))
                    .cloned()
                    .collect()
            };
            
            // Only set if different (simple length check + first item check for optimization)
            // Or just deep comparison since Vec<FileEntry> might not be cheap.
            // Actually, `entries.set()` likely checks equality if T: PartialEq. FileEntry implements PartialEq.
            // So we can just set it.
            
            // self.entries.set(filtered); 
            // The problem is `self.entries.set` triggers update() again -> infinite loop!
            // We need to check against current value before setting.
            
            // Perform manual equality check based on paths and essential metadata
            // since FileEntry doesn't implement PartialEq
            let current = self.entries.get();
            let mut changed = current.len() != filtered.len();
            if !changed {
                for (a, b) in current.iter().zip(filtered.iter()) {
                    if a.path != b.path || a.name != b.name {
                        changed = true;
                        break;
                    }
                }
            }
            
            if changed {
                 self.entries.set(filtered);
                 update.insert(Update::LAYOUT | Update::DRAW);
            }
        }
        
        // Ensure ItemView exists if mode is Table or List
        let mode = *self.view_mode.get();
        
        if mode == FileListViewMode::Table || mode == FileListViewMode::List {
            self.ensure_item_view();
            if let Some(ref mut view) = self.item_view {
                 // Sync FileList selection (paths) -> ItemView selection (indices)
                 {
                     let current_selected_paths = self.selected_paths.get();
                     let entries = self.entries.get();
                     let mut indices = Vec::new();
                     
                     for path in current_selected_paths.iter() {
                         if let Some(idx) = entries.iter().position(|e| e.path == *path) {
                             indices.push(idx);
                         }
                     }
                     
                     if let Some(signal) = &self.item_view_selection {
                         signal.set(indices);
                     }
                 } // entries dropped here!
                 
                 // ItemView is a child in layout tree, use layout.children[0]
                if !layout.children.is_empty() {
                    return view.update(&layout.children[0], context, info).await;
                }
            }
        }


        // Update child (ScrollContainer)
        // Update active child
        if !layout.children.is_empty() {
            if let Some(ref mut view) = self.item_view {
                 update |= view.update(&layout.children[0], context.clone(), info).await;
            } else {
                 update |= self.scroll_container.update(&layout.children[0], context.clone(), info).await;
            }
        }

        update
    }

    fn render(
        &mut self,
        graphics: &mut dyn Graphics,
        layout: &LayoutNode,
        info: &mut AppInfo,
        context: AppContext,
    ) {
        // Draw background for the file list
        let rect = Rect::new(
            layout.layout.location.x as f64,
            layout.layout.location.y as f64,
            (layout.layout.location.x + layout.layout.size.width) as f64,
            (layout.layout.location.y + layout.layout.size.height) as f64,
        );

        let palette = context.palette();
        let bg_color = palette.color(ColorRole::Base); // Use Base color for file list background

        graphics.fill(
             nptk::core::vg::peniko::Fill::NonZero,
             Affine::IDENTITY,
             &Brush::Solid(bg_color),
             None,
             &rect.to_path(0.1)
        );

        let mode = *self.view_mode.get();
        if mode == FileListViewMode::Table || mode == FileListViewMode::List {
             if let Some(ref mut view) = self.item_view {
                // ItemView is a child in the layout tree, so use layout.children[0]
                if !layout.children.is_empty() {
                    view.render(graphics, &layout.children[0], info, context.clone());
                    return;
                }
            }
        }
        
        // Render ScrollContainer (fallback or for other modes if ItemView not used)
        if !layout.children.is_empty() {
            self.scroll_container
                .render(graphics, &layout.children[0], info, context);
        }
    }
}

impl WidgetLayoutExt for FileList {
    fn set_layout_style(&mut self, layout_style: impl Into<MaybeSignal<LayoutStyle>>) {
        self.layout_style = layout_style.into();
    }
}

