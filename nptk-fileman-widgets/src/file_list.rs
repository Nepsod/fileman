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
use nptk::services::filesystem::entry::{FileEntry, FileMetadata, FileType};
use nptk::services::filesystem::model::{FileSystemEvent, FileSystemModel};
use npio::service::icon::IconRegistry;
use npio::{ThumbnailService, ThumbnailEvent, ThumbnailImage, get_file_for_uri, register_backend};
use npio::backend::local::LocalBackend;
use nptk::services::thumbnail::npio_adapter::{uri_to_path, thumbnail_size_to_u32};
use nptk::core::theme::ColorRole;
use nptk::core::scroll::ScrollHandle;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tokio::{sync::broadcast, time::{Duration, Instant}};
use std::time::Duration as StdDuration;

mod actions;
use actions::launch_default_app;
mod properties;
mod view_compact;
mod view_icon;
mod view_list;
mod model_adapter;
pub mod types;
mod content;

use content::FileListContent;
pub use types::*;

/// Empty-area context menu (Paste, New Folder, New File, Refresh). Shared by `ItemView` and `FileListContent`.
fn show_file_list_background_context_menu(
    op_tx: tokio::sync::mpsc::UnboundedSender<FileListOperation>,
    pos: Vector2<f64>,
    context: AppContext,
) -> Update {
    let op_paste = op_tx.clone();
    let op_new_folder = op_tx.clone();
    let op_new_file = op_tx.clone();
    let op_refresh = op_tx;
    let bg_menu = MenuTemplate::new("file_list_background_menu")
        .add_item(
            MenuItem::new(MenuCommand::Custom(0x2101), "Paste").with_action(move || {
                let _ = op_paste.send(FileListOperation::Paste);
                Update::DRAW
            }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(0x2102), "New Folder").with_action(move || {
                let _ = op_new_folder.send(FileListOperation::PromptNewFolder);
                Update::DRAW
            }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(0x2103), "New File").with_action(move || {
                let _ = op_new_file.send(FileListOperation::PromptNewFile);
                Update::DRAW
            }),
        )
        .add_item(
            MenuItem::new(MenuCommand::Custom(0x2104), "Refresh").with_action(move || {
                let _ = op_refresh.send(FileListOperation::Refresh);
                Update::DRAW
            }),
        );
    context
        .menu_manager
        .show(bg_menu, Point::new(pos.x, pos.y));
    Update::DRAW
}




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

#[derive(Default)]
struct FileListPerfStats {
    frames: u64,
    update_total: StdDuration,
    render_total: StdDuration,
    entries_processed: u64,
    visible_rows_rendered: u64,
    refresh_triggers: u64,
    search_rebuild_triggers: u64,
    fs_invalidate_batches: u64,
    fs_event_truncated_frames: u64,
    fs_dirty_events_coalesced: u64,
    update_phase_max_us: u64,
}

fn hash_path_slice(paths: &[PathBuf]) -> u64 {
    let mut hasher = DefaultHasher::new();
    paths.len().hash(&mut hasher);
    for path in paths {
        path.hash(&mut hasher);
    }
    hasher.finish()
}

fn entries_fingerprint(entries: &[FileEntry]) -> u64 {
    let mut hasher = DefaultHasher::new();
    entries.len().hash(&mut hasher);
    if let Some(entry) = entries.first() {
        entry.path.hash(&mut hasher);
    }
    if let Some(entry) = entries.last() {
        entry.path.hash(&mut hasher);
    }
    hasher.finish()
}

/// Row height used with `ItemView` default `item_height` for scroll / visibility hints.
const ITEM_VIEW_ROW_HEIGHT_PX: f32 = 30.0;

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
    search_scope: Option<StateSignal<SearchScope>>,
    /// When false, hide entries with [`FileMetadata::is_hidden`] (dotfiles on Unix).
    show_hidden_files: StateSignal<bool>,

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

    // Recursive search: when scope is FolderAndSubfolders
    recursive_result_rx: Option<tokio::sync::mpsc::UnboundedReceiver<Vec<FileEntry>>>,
    last_recursive_search: Option<(PathBuf, String, bool)>,
    // Track the last current-folder filter key so we only recompute when needed
    last_current_folder_search: Option<(PathBuf, String, usize, bool)>,
    /// Receiver for current-folder filter results (filter runs in spawn_blocking to avoid blocking UI)
    current_folder_result_rx: Option<tokio::sync::mpsc::UnboundedReceiver<Vec<FileEntry>>>,

    // Pending search query from location bar (avoids setting signal from inside TextInput update)
    search_pending_rx: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
    perf: FileListPerfStats,
    item_view_scroll_handle: Option<Arc<ScrollHandle>>,
    last_index_sync_key: Option<(u64, u64)>,
}

impl FileList {
    fn apply_with(mut self, f: impl FnOnce(&mut Self)) -> Self {
        f(&mut self);
        self
    }

    /// Create a new file list widget.
    pub fn new(initial_path: PathBuf) -> Self {
        Self::new_with_operations(initial_path, None, None, None, None)
    }

    /// Create a new file list widget with optional operation channel for file operations.
    ///
    /// `operation_tx` - if provided, file operations (like delete) will be sent via this channel.
    /// `selection_change_tx` - if provided, selection changes will be notified via this channel.
    /// `search_query_signal` - if provided, this signal is used for search filtering (shared with location bar for live search).
    /// `search_pending_rx` - if provided with search_query_signal, search text is received here (from location bar) and applied in update to avoid reentrant signal writes.
    pub fn new_with_operations(
        initial_path: PathBuf,
        operation_tx: Option<tokio::sync::mpsc::UnboundedSender<FileListOperation>>,
        selection_change_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<PathBuf>>>,
        search_query_signal: Option<StateSignal<String>>,
        search_pending_rx: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
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
        let search_query = search_query_signal.unwrap_or_else(|| StateSignal::new(String::new()));
        let show_hidden_files = StateSignal::new(false);

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
        let pending_icon_loads = Arc::new(Mutex::new(HashSet::new()));
        
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
            pending_icon_loads.clone(),
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
            search_scope: None,
            show_hidden_files,
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
            recursive_result_rx: None,
            last_recursive_search: None,
            last_current_folder_search: None,
            current_folder_result_rx: None,
            search_pending_rx,
            perf: FileListPerfStats::default(),
            item_view_scroll_handle: None,
            last_index_sync_key: None,
        }
    }

    fn estimate_visible_rows(&self, layout: &LayoutNode) -> usize {
        let total_entries = self.entries.get().len();
        if total_entries == 0 {
            return 0;
        }
        let mode = *self.view_mode.get();
        let row_height = match mode {
            FileListViewMode::List | FileListViewMode::Table => ITEM_VIEW_ROW_HEIGHT_PX,
            FileListViewMode::Compact => ITEM_VIEW_ROW_HEIGHT_PX,
            FileListViewMode::Icon => (*self.icon_size.get() as f32 + 20.0).max(ITEM_VIEW_ROW_HEIGHT_PX),
        };
        let viewport_h = layout.layout.size.height.max(1.0);
        let visible = ((viewport_h / row_height).ceil() as usize).saturating_add(2);
        visible.min(total_entries)
    }

    fn push_visible_row_hint(&self, layout: &LayoutNode) {
        let Some(model) = self.sort_model.as_ref() else {
            return;
        };
        let Some(scroll_handle) = self.item_view_scroll_handle.as_ref() else {
            model.set_visible_row_range(0, usize::MAX);
            return;
        };
        let mode = *self.view_mode.get();
        let row_h = match mode {
            FileListViewMode::List | FileListViewMode::Table | FileListViewMode::Compact => {
                ITEM_VIEW_ROW_HEIGHT_PX
            }
            FileListViewMode::Icon => (*self.icon_size.get() as f32 + 20.0).max(ITEM_VIEW_ROW_HEIGHT_PX),
        }
        .max(1.0);
        let scroll_y = scroll_handle.offset().y.max(0.0);
        let first = (scroll_y / row_h).floor() as usize;
        let viewport_h = layout.layout.size.height.max(1.0);
        let visible_rows = ((viewport_h / row_h).ceil() as usize).saturating_add(8);
        let entry_count = self.entries.get().len();
        let end_exclusive = if entry_count == 0 {
            0usize
        } else {
            first
                .saturating_add(visible_rows)
                .min(entry_count)
                .max(first.saturating_add(1))
        };
        model.set_visible_row_range(first, end_exclusive);
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

            if let Some(op_tx_bg) = self.operation_tx.clone() {
                view = view.with_on_background_context_menu(move |pos, context| {
                    show_file_list_background_context_menu(op_tx_bg.clone(), pos, context)
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

            let list_scroll_handle = Arc::new(ScrollHandle::new());
            self.item_view_scroll_handle = Some(list_scroll_handle.clone());

            let mut scroll_container = ScrollContainer::new()
                .with_scroll_handle(list_scroll_handle)
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

    /// Replace selection with entries that were not selected.
    pub fn invert_selection(&mut self) {
        use std::collections::HashSet;
        let entries = self.entries.get();
        let selected_set: HashSet<PathBuf> = self.selected_paths.get().iter().cloned().collect();
        let paths: Vec<PathBuf> = entries
            .iter()
            .map(|e| e.path.clone())
            .filter(|p| !selected_set.contains(p))
            .collect();
        self.selected_paths.set(paths.clone());
        if let Some(ref tx) = self.selection_change_tx {
            let _ = tx.send(paths);
        }
    }

    /// Open paths: single directory uses `on_enter_folder`; single file launches; multiple selection launches files only.
    pub fn open_paths(&self, paths: &[PathBuf], mut on_enter_folder: impl FnMut(PathBuf)) {
        if paths.is_empty() {
            return;
        }
        if paths.len() == 1 {
            let p = paths[0].clone();
            if p.is_dir() {
                on_enter_folder(p);
            } else {
                launch_default_app(self.mime_registry.clone(), p);
            }
            return;
        }
        for p in paths {
            if p.is_dir() {
                continue;
            }
            launch_default_app(self.mime_registry.clone(), p.clone());
        }
    }

    pub fn open_selected_paths(&self, on_enter_folder: impl FnMut(PathBuf)) {
        let paths = self.selected_paths.get().clone();
        self.open_paths(&paths, on_enter_folder);
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

    pub fn with_search_scope_signal(mut self, signal: StateSignal<SearchScope>) -> Self {
        self.search_scope = Some(signal);
        self
    }

    /// Use a shared signal to control visibility of hidden / dotfile entries.
    pub fn with_show_hidden_files_signal(mut self, signal: StateSignal<bool>) -> Self {
        self.show_hidden_files = signal;
        self
    }

    pub fn show_hidden_files_signal(&self) -> StateSignal<bool> {
        self.show_hidden_files.clone()
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

    fn filter_visible_entries(entries: Vec<FileEntry>, show_hidden: bool) -> Vec<FileEntry> {
        if show_hidden {
            entries
        } else {
            entries
                .into_iter()
                .filter(|e| !e.metadata.is_hidden)
                .collect()
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
        let update_start = Instant::now();
        let _mode = *self.view_mode.get();
        
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
            context.hook_signal(&mut self.show_hidden_files);
            if self.search_pending_rx.is_none() {
                context.hook_signal(&mut self.search_query);
            }
            if let Some(ref mut scope) = self.search_scope {
                context.hook_signal(scope);
            }
            self.signals_hooked = true;
        }

        // Apply pending search query from location bar (avoids signal write from inside TextInput update).
        // Coalesce and bound the number of updates per frame to keep UI responsive even if many keystrokes
        // are queued (e.g. when typing quickly or if a slow search/filtering path is active).
        if let Some(ref mut rx) = self.search_pending_rx {
            const MAX_SEARCH_UPDATES_PER_FRAME: usize = 64;
            let mut updates_processed = 0usize;
            let mut last_value: Option<String> = None;
            while updates_processed < MAX_SEARCH_UPDATES_PER_FRAME {
                match rx.try_recv() {
                    Ok(value) => {
                        updates_processed += 1;
                        last_value = Some(value);
                    }
                    Err(_) => break,
                }
            }
            if let Some(value) = last_value {
                self.search_query.set(value);
            }
        }
        
        let mut update = Update::empty();

        // Keyboard: select all (does not use operation_tx). Clipboard/delete/refresh/rename go through operation_tx when set.
        for (_, key_event) in &info.keys {
            if key_event.state != nptk::core::window::ElementState::Pressed {
                continue;
            }
            let modifiers = info.modifiers;
            if modifiers.control_key() {
                if key_event.physical_key
                    == nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::KeyA)
                {
                    self.select_all();
                    update.insert(Update::DRAW);
                    continue;
                }
            } else if key_event.physical_key
                == nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::Escape)
            {
                self.clear_selection();
                update.insert(Update::DRAW);
                continue;
            } else if key_event.physical_key
                == nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::Backspace)
                && !modifiers.control_key()
            {
                if let Some(ref tx) = self.operation_tx {
                    let _ = tx.send(FileListOperation::NavigateUp);
                }
                continue;
            } else if modifiers.alt_key()
                && !modifiers.control_key()
                && key_event.physical_key
                    == nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::ArrowUp)
            {
                if let Some(ref tx) = self.operation_tx {
                    let _ = tx.send(FileListOperation::NavigateUp);
                }
                continue;
            } else if key_event.physical_key
                == nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::F5)
            {
                if let Some(ref tx) = self.operation_tx {
                    let _ = tx.send(FileListOperation::Refresh);
                }
                continue;
            } else if key_event.physical_key
                == nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::F2)
            {
                if let Some(ref tx) = self.operation_tx {
                    let paths = self.selected_paths.get().clone();
                    if paths.len() == 1 {
                        let _ = tx.send(FileListOperation::PromptRename(paths[0].clone()));
                    }
                }
                continue;
            } else if !modifiers.control_key()
                && (key_event.physical_key
                    == nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::Enter)
                    || key_event.physical_key
                        == nptk::core::window::PhysicalKey::Code(
                            nptk::core::window::KeyCode::NumpadEnter,
                        ))
            {
                if let Some(ref tx) = self.operation_tx {
                    let _ = tx.send(FileListOperation::Open);
                }
                continue;
            }

            if let Some(ref tx) = self.operation_tx {
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
                        nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::KeyD) => {
                            let selection = self.selected_paths.get().clone();
                            if !selection.is_empty() {
                                let _ = tx.send(FileListOperation::Duplicate(selection));
                            }
                        }
                        _ => {}
                    }
                } else if key_event.physical_key
                    == nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::Delete)
                {
                    let selection = self.selected_paths.get().clone();
                    if !selection.is_empty() {
                        if modifiers.shift_key() {
                            let _ = tx.send(FileListOperation::DeletePermanent(selection));
                        } else {
                            let _ = tx.send(FileListOperation::DeleteToTrash(selection));
                        }
                    }
                }
            }
        }
        
        // Poll filesystem events FIRST - this must happen before ItemView handling
        // so that entries get updated even when using ItemView.
        // Limit per-frame event processing; coalesce cache invalidations and draw signals.
        if let Ok(mut rx) = self._event_rx.try_lock() {
            const MAX_FS_EVENTS_PER_UPDATE: usize = 256;
            let mut events_processed = 0usize;
            let mut pending_invalidates: HashSet<PathBuf> = HashSet::new();
            while events_processed < MAX_FS_EVENTS_PER_UPDATE {
                let event = match rx.try_recv() {
                    Ok(event) => event,
                    Err(_) => break,
                };
                events_processed += 1;
                match event {
                    FileSystemEvent::DirectoryLoaded { path, entries } => {
                        if path == *self.current_path.get() {
                            self.all_entries.set(entries.clone());

                            let scope_is_recursive = self
                                .search_scope
                                .as_ref()
                                .map(|s| *s.get() == SearchScope::FolderAndSubfolders)
                                .unwrap_or(false);
                            let query = self.search_query.get().to_lowercase();
                            let use_current_folder_only =
                                !scope_is_recursive || query.is_empty();

                            if use_current_folder_only && query.is_empty() {
                                let show = *self.show_hidden_files.get();
                                self.entries
                                    .set(Self::filter_visible_entries(entries, show));
                            }

                            update.insert(Update::LAYOUT | Update::DRAW);
                        }
                    }
                    FileSystemEvent::EntryAdded { path, .. }
                    | FileSystemEvent::EntryRemoved { path }
                    | FileSystemEvent::EntryModified { path, .. } => {
                        if let Some(parent) = path.parent() {
                            if parent == *self.current_path.get() {
                                pending_invalidates.insert(path);
                            }
                        }
                    }
                    _ => {}
                }
            }
            if events_processed == MAX_FS_EVENTS_PER_UPDATE {
                self.perf.fs_event_truncated_frames += 1;
                update.insert(Update::DRAW);
            }
            if !pending_invalidates.is_empty() {
                self.perf.fs_invalidate_batches += 1;
                self.perf.fs_dirty_events_coalesced += pending_invalidates.len() as u64;
                for path in pending_invalidates {
                    if let Err(e) = self.cache_invalidate_tx.send(path) {
                        log::warn!("Failed to send cache invalidation request: {}", e);
                    }
                }
                update.insert(Update::DRAW);
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
            self.perf.refresh_triggers += 1;
            update.insert(Update::LAYOUT | Update::DRAW);
        }
        
        // Single snapshot for search/filter branches (after FS + selection + path mutations).
        let snapshot_path = self.current_path.get().clone();
        let scope_is_recursive = self
            .search_scope
            .as_ref()
            .map(|s| *s.get() == SearchScope::FolderAndSubfolders)
            .unwrap_or(false);
        let query = self.search_query.get().to_lowercase();
        let show_hid = *self.show_hidden_files.get();
        let use_current_folder_only = !scope_is_recursive || query.is_empty();

        if !use_current_folder_only {
            let path = snapshot_path.clone();
            let need_search = self
                .last_recursive_search
                .as_ref()
                != Some(&(path.clone(), query.clone(), show_hid));
            if need_search {
                self.perf.search_rebuild_triggers += 1;
                self.last_recursive_search = Some((path.clone(), query.clone(), show_hid));
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                self.recursive_result_rx = Some(rx);
                let path_move = path.clone();
                let query_move = query.clone();
                tokio::task::spawn_blocking(move || {
                    let mut results = recursive_search_entries(
                        path_move,
                        &query_move,
                        MAX_RECURSIVE_SEARCH_DEPTH,
                    );
                    if !show_hid {
                        results.retain(|e| !e.metadata.is_hidden);
                    }
                    let _ = tx.send(results);
                });
            }
        } else {
            self.last_recursive_search = None;
        }

        if let Some(ref mut recv_rx) = self.recursive_result_rx {
            while let Ok(results) = recv_rx.try_recv() {
                self.entries.set(results);
                update.insert(Update::LAYOUT | Update::DRAW);
            }
        }

        if let Some(ref mut rx) = self.current_folder_result_rx {
            if let Ok(filtered) = rx.try_recv() {
                self.entries.set(filtered);
                update.insert(Update::LAYOUT | Update::DRAW);
            }
        }

        if use_current_folder_only {
            let path = snapshot_path.clone();
            let all_entries_snapshot = self.all_entries.get();
            let all_entries_len = all_entries_snapshot.len();
            let filter_key = (path.clone(), query.clone(), all_entries_len, show_hid);
            let need_filter = self
                .last_current_folder_search
                .as_ref()
                != Some(&filter_key);
            if need_filter {
                self.perf.search_rebuild_triggers += 1;
                self.last_current_folder_search = Some(filter_key);
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                self.current_folder_result_rx = Some(rx);
                let all_snapshot = all_entries_snapshot.clone();
                let query_move = query.clone();
                tokio::task::spawn_blocking(move || {
                    let mut filtered: Vec<FileEntry> = if query_move.is_empty() {
                        all_snapshot
                    } else {
                        all_snapshot
                            .into_iter()
                            .filter(|e| e.name.to_lowercase().contains(&query_move))
                            .collect()
                    };
                    if !show_hid {
                        filtered.retain(|e| !e.metadata.is_hidden);
                    }
                    let _ = tx.send(filtered);
                });
            }
        } else {
            self.last_current_folder_search = None;
            self.current_folder_result_rx = None;
        }

        self.push_visible_row_hint(layout);
        
        // Ensure ItemView exists if mode is Table or List
        if _mode == FileListViewMode::Table || _mode == FileListViewMode::List {
            self.ensure_item_view();
            if let Some(ref mut view) = self.item_view {
                 // Sync FileList selection (paths) -> ItemView selection (indices)
                 if let Some(signal) = &self.item_view_selection {
                     let current_selected_paths = self.selected_paths.get();
                     let entries = self.entries.get();
                     let sync_key = (
                         hash_path_slice(&current_selected_paths),
                         entries_fingerprint(&entries),
                     );
                     if self.last_index_sync_key != Some(sync_key) {
                         self.last_index_sync_key = Some(sync_key);
                         let mut indices = Vec::new();
                         for path in current_selected_paths.iter() {
                             if let Some(idx) = entries.iter().position(|e| e.path == *path) {
                                 indices.push(idx);
                             }
                         }
                         let current_indices = signal.get();
                         if current_indices.as_slice() != indices.as_slice() {
                             drop(current_indices);
                             signal.set(indices);
                         }
                     }
                 }
                 
                 // ItemView is a child in layout tree, use layout.children[0]
                if !layout.children.is_empty() {
                    let mut ret = view.update(&layout.children[0], context.clone(), info).await;
                    ret |= update;
                    let elapsed = update_start.elapsed();
                    self.perf.update_total += elapsed;
                    self.perf.frames += 1;
                    self.perf.entries_processed += self.entries.get().len() as u64;
                    self.perf.update_phase_max_us =
                        self.perf.update_phase_max_us.max(elapsed.as_micros() as u64);
                    return ret;
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
        let elapsed = update_start.elapsed();
        self.perf.update_total += elapsed;
        self.perf.frames += 1;
        self.perf.entries_processed += self.entries.get().len() as u64;
        self.perf.update_phase_max_us = self.perf.update_phase_max_us.max(elapsed.as_micros() as u64);

        update
    }

    fn render(
        &mut self,
        graphics: &mut dyn Graphics,
        layout: &LayoutNode,
        info: &mut AppInfo,
        context: AppContext,
    ) {
        let render_start = Instant::now();
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
        self.perf.render_total += render_start.elapsed();
        self.perf.visible_rows_rendered += self.estimate_visible_rows(layout) as u64;
        if self.perf.frames % 240 == 0 && self.perf.frames > 0 {
            let frames = self.perf.frames as u128;
            log::info!(
                "FileList perf: avg_update={}us avg_render={}us avg_entries={} avg_visible_rows={} refresh={} search_rebuilds={} fs_batches={} fs_trunc_frames={} fs_coalesced_inv={} update_max_us={}",
                self.perf.update_total.as_micros() / frames,
                self.perf.render_total.as_micros() / frames,
                self.perf.entries_processed as u128 / frames,
                self.perf.visible_rows_rendered as u128 / frames,
                self.perf.refresh_triggers,
                self.perf.search_rebuild_triggers,
                self.perf.fs_invalidate_batches,
                self.perf.fs_event_truncated_frames,
                self.perf.fs_dirty_events_coalesced,
                self.perf.update_phase_max_us,
            );
            if std::env::var("FILEMAN_PERF").as_deref() == Ok("1") {
                if let Some(model) = &self.sort_model {
                    let p = &model.icon_perf;
                    log::info!(
                        "FileList icon perf: cache_try_fail={} pending_try_fail={} queue_sat={} offscreen_def={} svg_try_fail={}",
                        p.icon_cache_try_fail.load(std::sync::atomic::Ordering::Relaxed),
                        p.icon_pending_try_fail.load(std::sync::atomic::Ordering::Relaxed),
                        p.icon_queue_saturated.load(std::sync::atomic::Ordering::Relaxed),
                        p.icon_offscreen_deferred.load(std::sync::atomic::Ordering::Relaxed),
                        p.icon_svg_cache_try_fail.load(std::sync::atomic::Ordering::Relaxed),
                    );
                }
            }
            self.perf.update_phase_max_us = 0;
        }
    }
}

const MAX_RECURSIVE_SEARCH_DEPTH: u32 = 4;

fn recursive_search_entries(
    root: PathBuf,
    query_lower: &str,
    max_depth: u32,
) -> Vec<FileEntry> {
    use std::fs;
    use std::time::SystemTime;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    let mut out = Vec::new();
    fn go(
        path: &std::path::Path,
        query: &str,
        depth: u32,
        out: &mut Vec<FileEntry>,
    ) {
        if depth == 0 {
            return;
        }
        let read_dir = match fs::read_dir(path) {
            Ok(d) => d,
            Err(_) => return,
        };
        for entry in read_dir.filter_map(Result::ok) {
            let path = entry.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name == "." || name == ".." {
                continue;
            }
            let meta = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let file_type = if meta.is_dir() {
                FileType::Directory
            } else if meta.is_symlink() {
                FileType::Symlink
            } else if meta.is_file() {
                FileType::File
            } else {
                FileType::Other
            };
            #[cfg(unix)]
            let permissions = meta.permissions().mode();
            #[cfg(not(unix))]
            let permissions = 0o644;
            let metadata = FileMetadata {
                size: meta.len(),
                modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                created: meta.created().ok(),
                permissions,
                mime_type: None,
                is_hidden: name.starts_with('.'),
            };
            let parent = path.parent().map(PathBuf::from);
            if name.to_lowercase().contains(query) {
                out.push(FileEntry::new(
                    path.clone(),
                    name,
                    file_type,
                    metadata,
                    parent,
                ));
            }
            if meta.is_dir() && !meta.is_symlink() {
                go(&path, query, depth.saturating_sub(1), out);
            }
        }
    }
    go(&root, query_lower, max_depth, &mut out);
    out
}

impl WidgetLayoutExt for FileList {
    fn set_layout_style(&mut self, layout_style: impl Into<MaybeSignal<LayoutStyle>>) {
        self.layout_style = layout_style.into();
    }
}

