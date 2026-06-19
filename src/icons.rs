use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use nptk::file_icons::{icon_presentation_from_cached, FileIconPresentation, FileIconService};
use npio::FileType;

const PATH_ICON_CACHE_CAPACITY: usize = 512;

pub struct FileIconCache {
    icons: HashMap<(PathBuf, u32), FileIconPresentation>,
    icon_order: VecDeque<(PathBuf, u32)>,
    theme_icons: HashMap<(String, u32), FileIconPresentation>,
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
        self.icons.get(&(path.to_path_buf(), size)).cloned()
    }

    pub fn cached_theme_icon(&self, icon_name: &str, size: u32) -> Option<FileIconPresentation> {
        self.theme_icons
            .get(&(icon_name.to_string(), size))
            .cloned()
    }

    pub fn store_icon(&mut self, path: PathBuf, size: u32, icon: FileIconPresentation) {
        let key = (path.clone(), size);
        if !self.icons.contains_key(&key) {
            self.icon_order.push_back(key.clone());
        }
        self.icons.insert(key, icon);
        while self.icon_order.len() > PATH_ICON_CACHE_CAPACITY {
            if let Some(evicted) = self.icon_order.pop_front() {
                self.icons.remove(&evicted);
            }
        }
    }

    pub fn store_theme_icon(&mut self, icon_name: String, size: u32, icon: FileIconPresentation) {
        self.theme_icons.insert((icon_name, size), icon);
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
