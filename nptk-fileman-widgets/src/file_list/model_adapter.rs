use super::content::FileListContent;
use npio::service::icon::{CachedIcon, IconRegistry};
use npio::{ThumbnailService, ThumbnailSize as NpioThumbnailSize};
use npio::file::local::LocalFile;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::collections::{HashMap, HashSet};

use lru::LruCache;
use std::path::PathBuf;
use nptk::widgets::item_view::IconData;
use nptk::core::model::{ModelData, ItemRole, Orientation, SortOrder, ItemModel};
use nptk::prelude::{StateSignal, Signal};
use nptk::services::filesystem::entry::FileEntry;
use humansize::{format_size, BINARY};

const MAX_PENDING_ICON_TASKS: usize = 96;

/// Counters for smoothness-first icon path (read under `FILEMAN_PERF=1` from `FileList`).
#[derive(Default)]
pub struct FileSystemItemModelPerf {
    pub icon_cache_try_fail: AtomicU64,
    pub icon_pending_try_fail: AtomicU64,
    pub icon_queue_saturated: AtomicU64,
    pub icon_offscreen_deferred: AtomicU64,
    pub icon_svg_cache_try_fail: AtomicU64,
}

/// Adapter to expose a StateSignal<Vec<FileEntry>> as an ItemModel
#[derive(Clone)]
pub struct FileSystemItemModel {
    entries: StateSignal<Vec<FileEntry>>,
    icon_registry: Arc<IconRegistry>,
    thumbnail_service: Arc<ThumbnailService>,
    icon_cache: Arc<Mutex<HashMap<(PathBuf, u32), Option<CachedIcon>>>>,
    svg_scene_cache: Arc<Mutex<LruCache<String, (nptk::core::vg::Scene, f64, f64)>>>,
    icon_size: nptk::core::signal::MaybeSignal<f32>,
    pending_thumbnails: Arc<Mutex<HashSet<PathBuf>>>,
    size_display_cache: Arc<Mutex<HashMap<(PathBuf, u64), String>>>,
    cache_update_tx: tokio::sync::mpsc::Sender<()>,
    visible_row_start: Arc<AtomicUsize>,
    visible_row_end_exclusive: Arc<AtomicUsize>,
    pub icon_perf: Arc<FileSystemItemModelPerf>,
}

impl FileSystemItemModel {
    pub fn new(
        entries: StateSignal<Vec<FileEntry>>,
        icon_registry: Arc<IconRegistry>,
        thumbnail_service: Arc<ThumbnailService>,
        icon_cache: Arc<Mutex<HashMap<(PathBuf, u32), Option<CachedIcon>>>>,
        svg_scene_cache: Arc<Mutex<LruCache<String, (nptk::core::vg::Scene, f64, f64)>>>,
        pending_thumbnails: Arc<Mutex<HashSet<PathBuf>>>,
        cache_update_tx: tokio::sync::mpsc::Sender<()>,
    ) -> Self {
        Self { 
            entries,
            icon_registry,
            thumbnail_service,
            icon_cache,
            svg_scene_cache,
            icon_size: nptk::core::signal::MaybeSignal::value(16.0), // Default for list view
            pending_thumbnails,
            size_display_cache: Arc::new(Mutex::new(HashMap::new())),
            cache_update_tx,
            visible_row_start: Arc::new(AtomicUsize::new(0)),
            visible_row_end_exclusive: Arc::new(AtomicUsize::new(usize::MAX)),
            icon_perf: Arc::new(FileSystemItemModelPerf::default()),
        }
    }

    pub fn with_icon_size(mut self, size: impl Into<nptk::core::signal::MaybeSignal<f32>>) -> Self {
        self.icon_size = size.into();
        self
    }

    /// Hint which rows are on-screen; off-screen rows skip spawning new icon/thumbnail work.
    pub fn set_visible_row_range(&self, start: usize, end_exclusive: usize) {
        let end = end_exclusive.max(start);
        self.visible_row_start.store(start, Ordering::Relaxed);
        self.visible_row_end_exclusive.store(end, Ordering::Relaxed);
    }
}

impl ItemModel for FileSystemItemModel {
    fn row_count(&self) -> usize {
        self.entries.get().len()
    }

    fn column_count(&self) -> usize {
        4 // Name, Size, Type, Date (Modified)
    }

    fn data(&self, row: usize, col: usize, role: ItemRole) -> ModelData {
        let entries = self.entries.get();
        if row >= entries.len() {
            return ModelData::None;
        }
        let entry = &entries[row];

        match role {
            ItemRole::Display => match col {
                0 => ModelData::String(entry.name.clone()),
                1 => {
                     if entry.is_dir() {
                        ModelData::String("Directory".to_string())
                     } else {
                        let cache_key = (entry.path.clone(), entry.metadata.size);
                        let cached_size_label = self
                            .size_display_cache
                            .try_lock()
                            .ok()
                            .and_then(|cache| cache.get(&cache_key).cloned());
                        if let Some(label) = cached_size_label {
                            ModelData::String(label)
                        } else {
                            let formatted_size = format_size(entry.metadata.size, BINARY);
                            if let Ok(mut cache) = self.size_display_cache.try_lock() {
                                if cache.len() > 8192 {
                                    cache.clear();
                                }
                                cache.insert(cache_key, formatted_size.clone());
                            }
                            ModelData::String(formatted_size)
                        }
                     }
                },
                2 => ModelData::String(match entry.file_type {
                    nptk::services::filesystem::entry::FileType::File => "File",
                    nptk::services::filesystem::entry::FileType::Directory => "Directory",
                    nptk::services::filesystem::entry::FileType::Symlink => "Symlink",
                    nptk::services::filesystem::entry::FileType::Other => "Other",
                }.to_string()),
                3 => ModelData::String(FileListContent::format_system_time(
                    entry.metadata.modified,
                )),
                _ => ModelData::None,
            },
            ItemRole::Icon => {
                if col == 0 {
                    // Check cache for icon
                    let path = &entry.path;
                    let size = *self.icon_size.get() as u32;
                    let placeholder = || {
                        if entry.is_dir() {
                            ModelData::String("directory".to_string())
                        } else {
                            ModelData::String("file".to_string())
                        }
                    };

                    let cached = {
                        let Ok(cache) = self.icon_cache.try_lock() else {
                            self.icon_perf
                                .icon_cache_try_fail
                                .fetch_add(1, Ordering::Relaxed);
                            return placeholder();
                        };
                        cache.get(&(path.clone(), size)).cloned().flatten()
                    };

                    if let Some(icon) = cached {
                        match icon {
                            CachedIcon::Image { data, width, height } => {
                                use nptk::core::vg::peniko::{Blob, ImageAlphaType, ImageBrush, ImageData, ImageFormat};
                                let image_data = ImageData {
                                    data: Blob::from(data.to_vec()),
                                    format: ImageFormat::Rgba8,
                                    alpha_type: ImageAlphaType::Alpha,
                                    width: width,
                                    height: height,
                                };
                                let brush = ImageBrush::new(image_data);
                                ModelData::Custom(Arc::new(IconData::Image(brush, width, height)))
                            },
                            CachedIcon::Svg(svg_source) => {
                                let Ok(mut cache) = self.svg_scene_cache.try_lock() else {
                                    self.icon_perf
                                        .icon_svg_cache_try_fail
                                        .fetch_add(1, Ordering::Relaxed);
                                    return placeholder();
                                };
                                let (scene, width, height) =
                                    if let Some((s, w, h)) = cache.get(svg_source.as_str()) {
                                        (s.clone(), *w, *h)
                                    } else {
                                        use vello_svg::usvg::{
                                            ImageRendering, Options, ShapeRendering, TextRendering, Tree,
                                        };
                                        if let Ok(tree) = Tree::from_str(
                                            &svg_source,
                                            &Options {
                                                shape_rendering: ShapeRendering::GeometricPrecision,
                                                text_rendering: TextRendering::OptimizeLegibility,
                                                image_rendering: ImageRendering::OptimizeSpeed,
                                                ..Default::default()
                                            },
                                        ) {
                                            let scene = vello_svg::render_tree(&tree);
                                            let size = tree.size();
                                            let w = size.width() as f64;
                                            let h = size.height() as f64;
                                            cache.put(svg_source.to_string(), (scene.clone(), w, h));
                                            (scene, w, h)
                                        } else {
                                            return ModelData::None;
                                        }
                                    };
                                ModelData::Custom(Arc::new(IconData::Scene(scene, width, height)))
                            },
                            CachedIcon::Path(_) => {
                                // Should be handled by pending logic or ignored
                                ModelData::None
                            }
                        }
                    } else {
                        let visible_start = self.visible_row_start.load(Ordering::Relaxed);
                        let visible_end = self.visible_row_end_exclusive.load(Ordering::Relaxed);
                        let row_in_view = row >= visible_start && row < visible_end;
                        if !row_in_view {
                            self.icon_perf
                                .icon_offscreen_deferred
                                .fetch_add(1, Ordering::Relaxed);
                            return placeholder();
                        }

                        // Not cached, check if pending
                        let is_pending = {
                            let Ok(pending) = self.pending_thumbnails.try_lock() else {
                                self.icon_perf
                                    .icon_pending_try_fail
                                    .fetch_add(1, Ordering::Relaxed);
                                return placeholder();
                            };
                            pending.contains(path)
                        };

                        if !is_pending {
                            let should_queue = {
                                let Ok(mut pending) = self.pending_thumbnails.try_lock() else {
                                    self.icon_perf
                                        .icon_pending_try_fail
                                        .fetch_add(1, Ordering::Relaxed);
                                    return placeholder();
                                };
                                if pending.len() >= MAX_PENDING_ICON_TASKS {
                                    self.icon_perf
                                        .icon_queue_saturated
                                        .fetch_add(1, Ordering::Relaxed);
                                    false
                                } else {
                                    pending.insert(path.clone());
                                    true
                                }
                            };
                            if !should_queue {
                                return placeholder();
                            }

                            // Spawn load task
                            let registry = self.icon_registry.clone();
                            let thumbnail_service = self.thumbnail_service.clone();
                            let icon_cache = self.icon_cache.clone();
                            let pending_thumbnails = self.pending_thumbnails.clone();
                            let cache_update_tx = self.cache_update_tx.clone();
                            let path_clone = path.clone();
                            let size = self.icon_size.clone();
                            let is_dir = entry.is_dir();
                            // Get current size from signal
                            let size_val = *size.get() as u32;

                            tokio::spawn(async move {
                                let icon = if is_dir {
                                    // Use directory icon
                                    registry.get_icon("folder", size_val)
                                } else {
                                    // Try to load thumbnail
                                    let file = LocalFile::new(path_clone.clone());
                                    
                                    // Determine thumbnail size
                                    let thumb_size = if size_val > 128 {
                                        NpioThumbnailSize::Large
                                    } else {
                                        NpioThumbnailSize::Normal
                                    };

                                    let mut loaded_icon = None;

                                    if let Ok(supported) = thumbnail_service.is_supported(&file, None).await {
                                        if supported {
                                            if let Ok(image) = thumbnail_service.get_thumbnail_image(&file, thumb_size, None).await {
                                                loaded_icon = Some(CachedIcon::Image {
                                                    width: image.width,
                                                    height: image.height,
                                                    data: Arc::new(image.data),
                                                });
                                            }
                                        }
                                    }
                                    
                                    if let Some(i) = loaded_icon {
                                        Some(i)
                                    } else {
                                        // Fallback to generic icon via registry (uses MIME detection)
                                        registry.get_file_icon(&file, size_val).await
                                    }
                                };
                                
                                // Remove from pending
                                {
                                    let mut pending = pending_thumbnails.lock().unwrap();
                                    pending.remove(&path_clone);
                                }
                                
                                // Update cache
                                {
                                    let mut cache = icon_cache.lock().unwrap();
                                    cache.insert((path_clone.clone(), size_val), icon);
                                }

                                // Trigger redraw
                                let _ = cache_update_tx.send(()).await;
                            });
                        }

                        placeholder()
                    }
                } else {
                    ModelData::None
                }
            },
            ItemRole::Sort => {
                // For sorting
                match col {
                    0 => ModelData::String(entry.name.clone()),
                    1 => ModelData::Int(entry.metadata.size as i64),
                    2 => ModelData::Int(match entry.file_type {
                        nptk::services::filesystem::entry::FileType::Directory => 0,
                        nptk::services::filesystem::entry::FileType::File => 1,
                        nptk::services::filesystem::entry::FileType::Symlink => 2,
                        nptk::services::filesystem::entry::FileType::Other => 3,
                    }),
                    3 => ModelData::Int(
                        entry
                            .metadata
                            .modified
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0),
                    ),
                    _ => ModelData::None,
                }
            }
            _ => ModelData::None,
        }
    }

    fn header_data(&self, section: usize, orientation: Orientation, role: ItemRole) -> ModelData {
        if orientation == Orientation::Horizontal && role == ItemRole::Display {
            match section {
                0 => ModelData::String("Name".to_string()),
                1 => ModelData::String("Size".to_string()),
                2 => ModelData::String("Type".to_string()),
                3 => ModelData::String("Date Modified".to_string()),
                _ => ModelData::None,
            }
        } else {
            ModelData::None
        }
    }

    fn sort(&self, column: usize, order: SortOrder) {
        self.entries.mutate(|entries| {
            entries.sort_by(|a, b| {
                let ord = match column {
                    0 => {
                        // Sort directories first
                        match (a.is_dir(), b.is_dir()) {
                            (true, false) => std::cmp::Ordering::Less,
                            (false, true) => std::cmp::Ordering::Greater,
                            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                        }
                    },
                    1 => a.metadata.size.cmp(&b.metadata.size),
                    2 => format!("{:?}", a.file_type).cmp(&format!("{:?}", b.file_type)),
                    3 => a.metadata.modified.cmp(&b.metadata.modified),
                    _ => std::cmp::Ordering::Equal,
                };
                
                match order {
                    SortOrder::Ascending => ord,
                    SortOrder::Descending => ord.reverse(),
                }
            });
        });
    }
}
