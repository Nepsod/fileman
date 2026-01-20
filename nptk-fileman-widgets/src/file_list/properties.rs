use async_trait::async_trait;
use super::FileListContent;
use nptk::prelude::{LayoutContext, LengthPercentage};
use nptk::widgets::file_icon::renderer::{render_image_icon, render_svg_icon_with_arc_cache};
use nptk::widgets::tabs_container::{TabItem, TabsContainer};
use chrono::{DateTime, Local};
use humansize::{format_size, BINARY};
use nalgebra::Vector2;
use nptk::core::app::context::AppContext;
use nptk::core::app::info::AppInfo;
use nptk::core::app::update::Update;
use nptk::core::layout::{Dimension, LayoutNode, LayoutStyle, StyleNode};
use nptk::core::text_render::TextRenderContext;
use nptk::core::vg::kurbo::{Affine, Rect, Vec2, Shape};
use nptk::core::vg::peniko::{Blob, Brush, Color, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat};
use nptk::core::vg::Scene;
use nptk::core::vgi::Graphics;
use nptk::core::widget::{BoxedWidget, Widget, WidgetLayoutExt};
use nptk::services::filesystem::entry::{FileEntry, FileMetadata, FileType};
use npio::service::filesystem::mime_detector::MimeDetector;
use npio::service::icon::IconRegistry;
use nptk::services::thumbnail::npio_adapter::{file_entry_to_uri, u32_to_thumbnail_size};
use npio::{ThumbnailService, get_file_for_uri};
use nptk::core::theme::{ColorRole, Palette};
use nptk::theme::id::WidgetId;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

impl FileListContent {
    pub(super) fn build_properties_widget(
        data: PropertiesData,
        icon_registry: Arc<IconRegistry>,
        thumbnail_service: Arc<ThumbnailService>,
        icon_cache: Arc<
            Mutex<
                std::collections::HashMap<(PathBuf, u32), Option<npio::service::icon::CachedIcon>>,
            >,
        >,
        svg_scene_cache: Arc<
            Mutex<std::collections::HashMap<String, (nptk::core::vg::Scene, f64, f64)>>,
        >,
    ) -> BoxedWidget {
        let content = PropertiesContent::new(
            data,
            icon_registry,
            thumbnail_service,
            icon_cache,
            svg_scene_cache,
        );
        let tab = TabItem::new("general", "General", content);
        let tabs = TabsContainer::new()
            .with_layout_style(LayoutStyle {
                size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
                ..Default::default()
            })
            .with_tab(tab);
        Box::new(tabs)
    }

    pub(super) fn show_properties_popup(&self, paths: &[PathBuf], context: AppContext) {
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
                .or_else(|| Self::xdg_mime_filetype(path))
                .unwrap_or_else(|| "unknown".to_string());

            let kind_display = if let Some(description) = self.lookup_mime_description(&mime_type) {
                format!("{} ({})", description, mime_type)
            } else {
                mime_type.clone()
            };
            rows.push(("Kind".to_string(), kind_display));
            rows.push(("Name".to_string(), name.to_string()));

            if let Ok(meta) = fs::metadata(path) {
                let size = if meta.is_dir() {
                    Self::calculate_directory_size(path)
                } else {
                    meta.len()
                };
                rows.push((
                    "Size".to_string(),
                    format_size(size, BINARY) + " (" + size.to_string().as_str() + " bytes)",
                ));
                if let Ok(modified) = meta.modified() {
                    rows.push(("Modified".to_string(), Self::format_system_time(modified)));
                }
                if let Ok(created) = meta.created() {
                    rows.push(("Created".to_string(), Self::format_system_time(created)));
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
                        Self::calculate_directory_size(p)
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

        let title_clone = title.clone();
        let rows_clone = rows.clone();
        let data = PropertiesData {
            title,
            icon_label,
            rows,
            paths: paths.to_vec(),
        };
        let svg_scene_cache = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let props_widget = Self::build_properties_widget(
            data,
            self.icon_registry.clone(),
            self.thumbnail_service.clone(),
            self.icon_cache.clone(),
            svg_scene_cache,
        );
        let pos = self
            .last_cursor
            .map(|p| (p.x as i32, p.y as i32))
            .unwrap_or((100, 100));
        context
            .popup_manager
            .create_popup_at(props_widget, "Properties", (360, 260), pos);
    }

    fn format_system_time(time: std::time::SystemTime) -> String {
        let dt: DateTime<Local> = time.into();
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    }

    fn lookup_mime_description(&self, mime_type: &str) -> Option<String> {
        for variant in Self::mime_description_variants(mime_type) {
            if let Some(desc) = self.mime_registry.description(&variant) {
                return Some(desc);
            }
        }
        Self::get_mime_description(mime_type)
    }

    fn get_mime_description(mime_type: &str) -> Option<String> {
        for variant in Self::mime_description_variants(mime_type) {
            if let Some(desc) = Self::get_mime_description_single(&variant) {
                return Some(desc);
            }
        }
        None
    }

    fn get_mime_description_single(mime_type: &str) -> Option<String> {
        if let Some((major, minor)) = mime_type.split_once('/') {
            let path = Path::new("/usr/share/mime")
                .join(major)
                .join(format!("{minor}.xml"));
            if let Ok(content) = fs::read_to_string(&path) {
                if content.contains(&format!(r#"type="{}""#, mime_type)) {
                    if let Some(comment) = Self::extract_comment(&content) {
                        return Some(comment);
                    }
                }
            }
        }

        let packages_dir = Path::new("/usr/share/mime/packages");
        let entries = match fs::read_dir(packages_dir) {
            Ok(entries) => entries,
            Err(_) => return None,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("xml") {
                continue;
            }
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let mut search_start = 0;
            while let Some(idx) = content[search_start..].find("<mime-type") {
                let mime_start = search_start + idx;
                let tag_end = match content[mime_start..].find('>') {
                    Some(i) => mime_start + i + 1,
                    None => break,
                };
                let tag_text = &content[mime_start..tag_end];

                let type_attr = r#"type=""#;
                let type_idx = match tag_text.find(type_attr) {
                    Some(i) => i + type_attr.len(),
                    None => {
                        search_start = tag_end;
                        continue;
                    },
                };
                let rest = &tag_text[type_idx..];
                let end_quote = match rest.find('"') {
                    Some(i) => i,
                    None => {
                        search_start = tag_end;
                        continue;
                    },
                };
                let ty = &rest[..end_quote];
                if ty != mime_type {
                    search_start = tag_end;
                    continue;
                }

                let end_tag = "</mime-type>";
                let block_end = match content[tag_end..].find(end_tag) {
                    Some(i) => tag_end + i + end_tag.len(),
                    None => {
                        search_start = tag_end;
                        continue;
                    },
                };
                let mime_block = &content[mime_start..block_end];
                if let Some(comment) = Self::extract_comment(mime_block) {
                    return Some(comment);
                }
                search_start = block_end;
            }
        }

        None
    }

    fn extract_comment(mime_block: &str) -> Option<String> {
        let mut best: Option<String> = None;
        let mut fallback: Option<String> = None;
        let mut search_start = 0;
        while let Some(idx) = mime_block[search_start..].find("<comment") {
            let comment_start = search_start + idx;
            let tag_end = match mime_block[comment_start..].find('>') {
                Some(i) => comment_start + i + 1,
                None => break,
            };
            let end_tag = match mime_block[tag_end..].find("</comment>") {
                Some(i) => tag_end + i,
                None => break,
            };
            let tag_text = &mime_block[comment_start..tag_end];
            let body = mime_block[tag_end..end_tag].trim();
            if body.is_empty() {
                search_start = end_tag + "</comment>".len();
                continue;
            }
            let is_en = tag_text.contains(r#"xml:lang="en""#);
            if is_en {
                best = Some(body.to_string());
                break;
            } else if fallback.is_none() {
                fallback = Some(body.to_string());
            }
            search_start = end_tag + "</comment>".len();
        }
        best.or(fallback)
    }

    fn mime_description_variants(mime_type: &str) -> Vec<String> {
        let mut variants = Vec::new();
        variants.push(mime_type.to_string());
        if let Some((major, rest)) = mime_type.split_once('/') {
            if let Some(stripped) = rest.strip_prefix("x-") {
                variants.push(format!("{}/{}", major, stripped));
            }
        }
        match mime_type {
            "application/toml" => variants.push("text/x-toml".to_string()),
            "text/x-rust" => variants.push("text/rust".to_string()),
            "application/x-shellscript" => {
                variants.push("text/x-shellscript".to_string());
                variants.push("text/x-sh".to_string());
            },
            "application/zstd" => variants.push("application/x-zstd".to_string()),
            "application/x-rar" => variants.push("application/vnd.rar".to_string()),
            "application/x-iso9660-image" => {
                variants.push("application/x-iso9660-image".to_string())
            },
            "text/x-log" => variants.push("text/plain".to_string()),
            _ => {},
        }
        variants
    }

    fn calculate_directory_size(path: &Path) -> u64 {
        let metadata = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => return 0,
        };

        if !metadata.is_dir() {
            return metadata.len();
        }

        let mut total_size = 0u64;
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(_) => return metadata.len(),
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let entry_path = entry.path();
            let entry_metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            if entry_metadata.is_dir() {
                total_size = total_size.saturating_add(Self::calculate_directory_size(&entry_path));
            } else {
                total_size = total_size.saturating_add(entry_metadata.len());
            }
        }

        total_size
    }
}

pub(super) struct PropertiesData {
    title: String,
    icon_label: String,
    rows: Vec<(String, String)>,
    paths: Vec<PathBuf>,
}

struct PropertiesContent {
    data: PropertiesData,
    text_ctx: TextRenderContext,
    icon_registry: Arc<IconRegistry>,
    thumbnail_service: Arc<ThumbnailService>,
    _icon_cache: Arc<
        Mutex<
            std::collections::HashMap<(PathBuf, u32), Option<npio::service::icon::CachedIcon>>,
        >,
    >,
    svg_scene_cache: Arc<Mutex<std::collections::HashMap<String, (nptk::core::vg::Scene, f64, f64)>>>,
    thumbnail_size: u32,
}

impl PropertiesContent {
    fn new(
        data: PropertiesData,
        icon_registry: Arc<IconRegistry>,
        thumbnail_service: Arc<ThumbnailService>,
        icon_cache: Arc<
            Mutex<
                std::collections::HashMap<(PathBuf, u32), Option<npio::service::icon::CachedIcon>>,
            >,
        >,
        svg_scene_cache: Arc<
            Mutex<std::collections::HashMap<String, (nptk::core::vg::Scene, f64, f64)>>,
        >,
    ) -> Self {
        Self {
            data,
            text_ctx: TextRenderContext::new(),
            icon_registry,
            thumbnail_service,
            _icon_cache: icon_cache,
            svg_scene_cache,
            thumbnail_size: 64,
        }
    }
}

#[async_trait(?Send)]
impl Widget for PropertiesContent {
    fn widget_id(&self) -> WidgetId {
        WidgetId::new("nptk-fileman-widgets", "FileListProperties")
    }

    fn layout_style(&self, _context: &LayoutContext) -> StyleNode {
        StyleNode {
            style: LayoutStyle {
                size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
                ..Default::default()
            },
            children: vec![],
            measure_func: None,
        }
    }

    async fn update(&mut self, _: &LayoutNode, _: AppContext, _: &mut AppInfo) -> Update {
        Update::empty()
    }

    fn render(
        &mut self,
        graphics: &mut dyn Graphics,
        layout: &LayoutNode,
        info: &mut AppInfo,
        context: AppContext,
    ) {
        let palette = context.palette();
        let bg = palette.color(ColorRole::Window);
        let rect = Rect::new(
            layout.layout.location.x as f64,
            layout.layout.location.y as f64,
            (layout.layout.location.x + layout.layout.size.width) as f64,
            (layout.layout.location.y + layout.layout.size.height) as f64,
        );
        graphics.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            &Brush::Solid(bg),
            None,
            &rect.to_path(4.0),
        );

        let text_color = palette.color(ColorRole::BaseText);
        let label_color = palette.color(ColorRole::DisabledTextFront);

        let padding = 12.0;
        let icon_size = 48.0;
        let icon_rect = Rect::new(
            rect.x0 + padding,
            rect.y0 + padding,
            rect.x0 + padding + icon_size,
            rect.y0 + padding + icon_size,
        );

        let mut icon_rendered = false;

        if self.data.paths.len() > 1 {
            let icon_names = ["document-multiple", "folder-multiple", "document", "folder"];
            for icon_name in &icon_names {
                if let Some(icon) = self.icon_registry.get_icon(icon_name, icon_size as u32) {
                    let icon_x = icon_rect.x0;
                    let icon_y = icon_rect.y0;
                    let icon_size_f64 = icon_rect.width().min(icon_rect.height());

                    match icon {
                        npio::service::icon::CachedIcon::Image {
                            data,
                            width,
                            height,
                        } => {
                            render_image_icon(graphics, data.as_ref(), width, height, icon_rect);
                            icon_rendered = true;
                            break;
                        },
                        npio::service::icon::CachedIcon::Svg(svg_source) => {
                            render_svg_icon_with_arc_cache(
                                graphics,
                                &svg_source,
                                icon_rect,
                                &self.svg_scene_cache,
                            );
                            icon_rendered = true;
                            break;
                        },
                        npio::service::icon::CachedIcon::Path(_) => {},
                    }
                }
            }
        }

        if !icon_rendered && self.data.paths.len() == 1 {
            let path = &self.data.paths[0];

            let entry = if let Ok(metadata) = fs::metadata(path) {
                let file_type = if metadata.is_dir() {
                    FileType::Directory
                } else if metadata.is_file() {
                    FileType::File
                } else if metadata.file_type().is_symlink() {
                    FileType::Symlink
                } else {
                    FileType::Other
                };

                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                let mime_type = if file_type == FileType::File {
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(MimeDetector::detect_mime_type(path))
                    })
                } else {
                    None
                };

                if let Ok(modified) = metadata.modified() {
                    let file_metadata = FileMetadata {
                        size: metadata.len(),
                        modified,
                        created: metadata.created().ok(),
                        permissions: 0,
                        mime_type,
                        is_hidden: name.starts_with('.'),
                    };

                    Some(FileEntry::new(
                        path.clone(),
                        name,
                        file_type,
                        file_metadata,
                        path.parent().map(|p| p.to_path_buf()),
                    ))
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(entry) = entry {
                if let Ok(file) = get_file_for_uri(&file_entry_to_uri(&entry)) {
                    if let Ok(thumbnail_image) = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            // Try to get existing thumbnail image (checks cache first)
                            self.thumbnail_service
                                .get_thumbnail_image(&*file, u32_to_thumbnail_size(self.thumbnail_size), None)
                                .await
                        })
                    }) {
                        let image_data = ImageData {
                            data: Blob::from(thumbnail_image.data),
                            format: ImageFormat::Rgba8,
                            alpha_type: ImageAlphaType::Alpha,
                            width: thumbnail_image.width,
                            height: thumbnail_image.height,
                        };
                        let image_brush = ImageBrush::new(image_data);
                        let icon_x = icon_rect.x0;
                        let icon_y = icon_rect.y0;
                        let icon_size_f64 = icon_rect.width().min(icon_rect.height());
                        let scale_x = icon_size_f64 / (thumbnail_image.width as f64);
                        let scale_y = icon_size_f64 / (thumbnail_image.height as f64);
                        let scale = scale_x.min(scale_y);
                        let transform =
                            Affine::scale_non_uniform(scale, scale).then_translate(Vec2::new(icon_x, icon_y));
                        if let Some(scene) = graphics.as_scene_mut() {
                            scene.draw_image(&image_brush, transform);
                            icon_rendered = true;
                        }
                    }
                }

                if !icon_rendered {
                    let uri = file_entry_to_uri(&entry);
                    if let Ok(file) = get_file_for_uri(&uri) {
                        if let Some(icon) = tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(self.icon_registry.get_file_icon(&*file, icon_size as u32))
                        })
                        {
                        match icon {
                            npio::service::icon::CachedIcon::Image {
                                data,
                                width,
                                height,
                            } => {
                                render_image_icon(graphics, data.as_ref(), width, height, icon_rect);
                                icon_rendered = true;
                            },
                            npio::service::icon::CachedIcon::Svg(svg_source) => {
                                render_svg_icon_with_arc_cache(
                                    graphics,
                                    &svg_source,
                                    icon_rect,
                                    &self.svg_scene_cache,
                                );
                                icon_rendered = true;
                            },
                            npio::service::icon::CachedIcon::Path(_) => {},
                        }
                        }
                    }
                }
            }
        }

        if !icon_rendered {
            self.text_ctx.render_text(
                &mut info.font_context,
                graphics,
                &self.data.icon_label,
                None,
                16.0,
                Brush::Solid(text_color),
                Affine::translate((
                    icon_rect.x0 + 6.0,
                    icon_rect.y0 + icon_size / 2.0 - 6.0,
                )),
                true,
                Some((icon_size - 12.0) as f32),
            );
        }

        self.text_ctx.render_text(
            &mut info.font_context,
            graphics,
            &self.data.title,
            None,
            16.0,
            Brush::Solid(text_color),
            Affine::translate((icon_rect.x1 + 10.0, icon_rect.y0 + 4.0)),
            true,
            Some(
                (rect.width() as f32 - (icon_rect.width() as f32) - 3.0 * padding as f32).max(80.0),
            ),
        );

        let mut y = icon_rect.y1 + 12.0;
        let label_width = 110.0;
        let value_x = rect.x0 + padding + label_width + 8.0;

        for (label, value) in &self.data.rows {
            self.text_ctx.render_text(
                &mut info.font_context,
                graphics,
                &format!("{}:", label),
                None,
                13.0,
                Brush::Solid(label_color),
                Affine::translate((rect.x0 + padding, y)),
                true,
                Some(label_width as f32),
            );
            self.text_ctx.render_text(
                &mut info.font_context,
                graphics,
                value,
                None,
                13.0,
                Brush::Solid(text_color),
                Affine::translate((value_x, y)),
                true,
                Some((rect.width() as f32 - value_x as f32 - padding as f32).max(60.0)),
            );
            y += 20.0;
        }
    }
}
