use std::collections::HashMap;
use std::path::{Path, PathBuf};

use file_icons::{icon_presentation_from_cached, FileIconPresentation, FileIconService};
use npio::FileType;

pub struct FileIconCache {
    icons: HashMap<(PathBuf, u32), FileIconPresentation>,
}

impl FileIconCache {
    pub fn new() -> Self {
        Self {
            icons: HashMap::new(),
        }
    }

    pub fn cached_icon(&self, path: &Path, size: u32) -> Option<FileIconPresentation> {
        self.icons.get(&(path.to_path_buf(), size)).cloned()
    }

    pub fn store_icon(&mut self, path: PathBuf, size: u32, icon: FileIconPresentation) {
        self.icons.insert((path, size), icon);
    }

    pub fn clear(&mut self) {
        self.icons.clear();
    }

    pub async fn load_icon(
        service: &FileIconService,
        path: PathBuf,
        size: u32,
        file_type: FileType,
    ) -> Option<FileIconPresentation> {
        let is_directory = file_type == FileType::Directory;
        let cached = service
            .resolve_icon(&path, size, is_directory)
            .await?;
        icon_presentation_from_cached(&cached)
    }
}
