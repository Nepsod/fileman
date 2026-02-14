use npio::service::icon::{IconRegistry, CachedIcon};
use npio::{ThumbnailService, ThumbnailSize as NpioThumbnailSize};
use npio::file::local::LocalFile;
use npio::file::File;
use std::sync::{Arc, Mutex};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use nptk::widgets::item_view::IconData;
use nptk::core::model::{ModelData, ItemRole, Orientation, SortOrder, ItemModel};
use nptk::prelude::{StateSignal, Signal};
use nptk::services::filesystem::entry::FileEntry;
use humansize::{format_size, BINARY};

/// Adapter to expose a StateSignal<Vec<FileEntry>> as an ItemModel
#[derive(Clone)]
pub struct FileSystemItemModel {
    entries: StateSignal<Vec<FileEntry>>,
    icon_registry: Arc<IconRegistry>,
    thumbnail_service: Arc<ThumbnailService>,
    icon_cache: Arc<Mutex<HashMap<(PathBuf, u32), Option<CachedIcon>>>>,
    svg_scene_cache: Arc<Mutex<HashMap<String, (nptk::core::vg::Scene, f64, f64)>>>,
    icon_size: u32,
    pending_thumbnails: Arc<Mutex<HashSet<PathBuf>>>,
    cache_update_tx: tokio::sync::mpsc::Sender<()>,
}

impl FileSystemItemModel {
    pub fn new(
        entries: StateSignal<Vec<FileEntry>>,
        icon_registry: Arc<IconRegistry>,
        thumbnail_service: Arc<ThumbnailService>,
        icon_cache: Arc<Mutex<HashMap<(PathBuf, u32), Option<CachedIcon>>>>,
        svg_scene_cache: Arc<Mutex<HashMap<String, (nptk::core::vg::Scene, f64, f64)>>>,
        pending_thumbnails: Arc<Mutex<HashSet<PathBuf>>>,
        cache_update_tx: tokio::sync::mpsc::Sender<()>,
    ) -> Self {
        Self { 
            entries,
            icon_registry,
            thumbnail_service,
            icon_cache,
            svg_scene_cache,
            icon_size: 16, // Default for list view
            pending_thumbnails,
            cache_update_tx,
        }
    }

    pub fn with_icon_size(mut self, size: u32) -> Self {
        self.icon_size = size;
        self
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
                        ModelData::String(format_size(entry.metadata.size, BINARY))
                     }
                },
                2 => ModelData::String(format!("{:?}", entry.file_type)), // Simplify for now
                3 => ModelData::String("Unknown".to_string()), // Date not in FileEntry yet?
                _ => ModelData::None,
            },
            ItemRole::Icon => {
                if col == 0 {
                    // Check cache for icon
                    let path = &entry.path;
                    let size = self.icon_size;
                    
                    let cached = {
                        let cache = self.icon_cache.lock().unwrap();
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
                                let mut cache = self.svg_scene_cache.lock().unwrap();
                                let (scene, width, height) = if let Some((s, w, h)) = cache.get(svg_source.as_str()) {
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
                                        cache.insert(svg_source.to_string(), (scene.clone(), w, h));
                                        (scene, w, h)
                                    } else {
                                        // Invalid SVG
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
                        // Not cached, check if pending
                        let is_pending = {
                            let pending = self.pending_thumbnails.lock().unwrap();
                            pending.contains(path)
                        };

                        if !is_pending {
                            // Mark as pending
                            {
                                let mut pending = self.pending_thumbnails.lock().unwrap();
                                pending.insert(path.clone());
                            }

                            // Spawn load task
                            let registry = self.icon_registry.clone();
                            let thumbnail_service = self.thumbnail_service.clone();
                            let icon_cache = self.icon_cache.clone();
                            let pending_thumbnails = self.pending_thumbnails.clone();
                            let cache_update_tx = self.cache_update_tx.clone();
                            let path_clone = path.clone();
                            let size = self.icon_size;
                            let is_dir = entry.is_dir();

                            tokio::spawn(async move {
                                let icon = if is_dir {
                                    // Use directory icon
                                    registry.get_icon("folder", size)
                                } else {
                                    // Try to load thumbnail
                                    let file = LocalFile::new(path_clone.clone());
                                    
                                    // Determine thumbnail size
                                    let thumb_size = if size > 128 {
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
                                        // Fallback to generic icon
                                        // Real implementation would use mime provider or similar
                                        registry.get_icon("text-x-generic", size)
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
                                    cache.insert((path_clone.clone(), size), icon);
                                }

                                // Trigger redraw
                                let _ = cache_update_tx.send(()).await;
                            });
                        }

                         if entry.is_dir() {
                            ModelData::String("directory".to_string())
                        } else {
                            ModelData::String("file".to_string())
                        }
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
