use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use nptk::file_icons::{icon_presentation_from_cached, FileIconPresentation, FileIconService};
use npio::FileType;

const PATH_ICON_CACHE_CAPACITY: usize = 512;

pub struct FileIconCache {
    icons: HashMap<PathBuf, HashMap<u32, FileIconPresentation>>,
    icon_order: VecDeque<(PathBuf, u32)>,
    theme_icons: HashMap<String, HashMap<u32, FileIconPresentation>>,
}

impl FileIconCache {
    pub fn new() -> Self {
        Self {
            icons: HashMap::new(),
            icon_order: VecDeque::new(),
            theme_icons: HashMap::new(),
        }
    }

    pub fn cached_icon(&self, path: &Path, size: u32) -> Option<FileIconPresentation> {
        self.icons.get(path)?.get(&size).cloned()
    }

    pub fn cached_icon_ref(&self, path: &Path, size: u32) -> Option<&FileIconPresentation> {
        self.icons.get(path)?.get(&size)
    }

    pub fn cached_theme_icon(&self, icon_name: &str, size: u32) -> Option<FileIconPresentation> {
        self.theme_icons.get(icon_name)?.get(&size).cloned()
    }

    pub fn store_icon(&mut self, path: PathBuf, size: u32, icon: FileIconPresentation) {
        let size_map = self.icons.entry(path.clone()).or_default();
        let is_new = !size_map.contains_key(&size);
        size_map.insert(size, icon);
        if is_new {
            self.icon_order.push_back((path, size));
        }
        while self.icon_order.len() > PATH_ICON_CACHE_CAPACITY {
            if let Some((evicted_path, evicted_size)) = self.icon_order.pop_front() {
                if let Some(size_map) = self.icons.get_mut(&evicted_path) {
                    size_map.remove(&evicted_size);
                    if size_map.is_empty() {
                        self.icons.remove(&evicted_path);
                    }
                }
            }
        }
    }

    pub fn store_theme_icon(&mut self, icon_name: String, size: u32, icon: FileIconPresentation) {
        self.theme_icons
            .entry(icon_name)
            .or_default()
            .insert(size, icon);
    }

    pub fn clear(&mut self) {
        self.icons.clear();
        self.icon_order.clear();
        self.theme_icons.clear();
    }

    pub async fn load_theme_icon(
        service: &FileIconService,
        icon_name: &str,
        size: u32,
    ) -> Option<FileIconPresentation> {
        service.resolve_theme_icon(icon_name, size).await
    }

    pub async fn load_path_icon(
        service: &FileIconService,
        path: PathBuf,
        size: u32,
    ) -> Option<FileIconPresentation> {
        service.resolve_path_icon(&path, size).await
    }

    pub async fn load_icon(
        service: &FileIconService,
        path: PathBuf,
        size: u32,
        file_type: FileType,
        use_thumbnails: bool,
    ) -> Option<FileIconPresentation> {
        let is_directory = file_type == FileType::Directory;
        let cached = service
            .resolve_icon(&path, size, is_directory, use_thumbnails)
            .await?;
        icon_presentation_from_cached(&cached)
    }
}
