use nptk::std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::sort::{SortColumn, SortOrder};
use crate::view_mode::{self, ViewMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilemanConfig {
    #[serde(rename = "Window", default)]
    pub window: WindowSection,
    #[serde(rename = "FolderView", default)]
    pub folder_view: FolderViewSection,
    #[serde(rename = "Behavior", default)]
    pub behavior: BehaviorSection,
    #[serde(rename = "System", default)]
    pub system: SystemSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSection {
    #[serde(rename = "WindowTitle", default = "default_title")]
    pub window_title: String,
    #[serde(rename = "RememberWindowSize", default = "default_true")]
    pub remember_window_size: bool,
    #[serde(rename = "LastWindowWidth", default = "default_width")]
    pub last_window_width: u32,
    #[serde(rename = "LastWindowHeight", default = "default_height")]
    pub last_window_height: u32,
    #[serde(rename = "SplitterPos", default = "default_splitter")]
    pub splitter_pos: u32,
}

impl Default for WindowSection {
    fn default() -> Self {
        Self {
            window_title: default_title(),
            remember_window_size: default_true(),
            last_window_width: default_width(),
            last_window_height: default_height(),
            splitter_pos: default_splitter(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderViewSection {
    #[serde(rename = "ShowHidden", default = "default_false")]
    pub show_hidden: bool,
    #[serde(rename = "DefaultPath")]
    pub default_path: Option<PathBuf>,
    #[serde(rename = "SortColumn", default = "default_sort_column")]
    pub sort_column: String,
    #[serde(rename = "SortOrder", default = "default_sort_order")]
    pub sort_order: String,
    #[serde(rename = "Mode", default = "default_view_mode")]
    pub mode: String,
    #[serde(rename = "IconSize")]
    pub icon_size: Option<u32>,
}

impl Default for FolderViewSection {
    fn default() -> Self {
        Self {
            show_hidden: default_false(),
            default_path: None,
            sort_column: default_sort_column(),
            sort_order: default_sort_order(),
            mode: default_view_mode(),
            icon_size: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorSection {
    #[serde(rename = "ConfirmDelete", default = "default_true")]
    pub confirm_delete: bool,
    #[serde(rename = "ConfirmTrash", default = "default_true")]
    pub confirm_trash: bool,
    #[serde(rename = "UseTrash", default = "default_true")]
    pub use_trash: bool,
}

impl Default for BehaviorSection {
    fn default() -> Self {
        Self {
            confirm_delete: default_true(),
            confirm_trash: default_true(),
            use_trash: default_true(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSection {
    #[serde(rename = "Terminal")]
    pub terminal: Option<String>,
}

impl Default for SystemSection {
    fn default() -> Self {
        Self { terminal: None }
    }
}

fn default_title() -> String {
    "Fileman".to_string()
}
fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_width() -> u32 {
    1000
}
fn default_height() -> u32 {
    700
}
fn default_splitter() -> u32 {
    220
}
fn default_sort_column() -> String {
    "name".to_string()
}
fn default_sort_order() -> String {
    "ascending".to_string()
}
fn default_view_mode() -> String {
    "list".to_string()
}

impl Default for FilemanConfig {
    fn default() -> Self {
        Self {
            window: WindowSection::default(),
            folder_view: FolderViewSection::default(),
            behavior: BehaviorSection::default(),
            system: SystemSection::default(),
        }
    }
}

impl FilemanConfig {
    pub fn terminal_command(&self) -> Option<&str> {
        self.system.terminal.as_deref()
    }

    pub fn load_or_create() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("fileman");
        if let Err(error) = std::fs::create_dir_all(&config_dir) {
            log::warn!("failed to create config directory {}: {error}", config_dir.display());
        }

        let config_path = config_dir.join("config.toml");
        if !config_path.exists() {
            let default_config = Self::default();
            if let Ok(toml_string) = toml::to_string_pretty(&default_config) {
                if let Err(error) = std::fs::write(&config_path, toml_string) {
                    log::warn!("failed to write default config {}: {error}", config_path.display());
                }
            }
            return default_config;
        }

        match std::fs::read_to_string(&config_path) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|error| {
                log::warn!("failed to parse config {}: {error}", config_path.display());
                Self::default()
            }),
            Err(error) => {
                log::warn!("failed to read config {}: {error}", config_path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let Some(config_dir) = dirs::config_dir() else {
            return;
        };
        let config_path = config_dir.join("fileman").join("config.toml");
        if let Ok(toml_string) = toml::to_string_pretty(self) {
            if let Err(error) = std::fs::write(&config_path, toml_string) {
                log::warn!("failed to save config {}: {error}", config_path.display());
            }
        }
    }

    pub fn sort_column(&self) -> SortColumn {
        SortColumn::from_config(&self.folder_view.sort_column)
    }

    pub fn sort_order(&self) -> SortOrder {
        SortOrder::from_config(&self.folder_view.sort_order)
    }

    pub fn view_mode(&self) -> ViewMode {
        ViewMode::from_config(&self.folder_view.mode)
    }

    pub fn icon_size_for_mode(&self, mode: ViewMode) -> u32 {
        self.folder_view
            .icon_size
            .map(view_mode::clamp_icon_size)
            .unwrap_or_else(|| mode.default_icon_size())
    }
}

pub const SIDEBAR_MIN_WIDTH: u32 = 120;
pub const SIDEBAR_MAX_WIDTH: u32 = 400;
