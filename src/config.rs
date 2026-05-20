use std::path::PathBuf;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilemanConfig {
    #[serde(rename = "Window", default)]
    pub window: WindowSection,
    #[serde(rename = "FolderView", default)]
    pub folder_view: FolderViewSection,
    #[serde(rename = "Behavior", default)]
    pub behavior: BehaviorSection,
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
}

impl Default for FolderViewSection {
    fn default() -> Self {
        Self {
            show_hidden: default_false(),
            default_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BehaviorSection {
    #[serde(rename = "ConfirmDelete", default = "default_true")]
    pub confirm_delete: bool,
    #[serde(rename = "UseTrash", default = "default_true")]
    pub use_trash: bool,
}

fn default_title() -> String { "Fileman".to_string() }
fn default_true() -> bool { true }

fn default_false() -> bool { false }
fn default_width() -> u32 { 1000 }
fn default_height() -> u32 { 700 }
fn default_splitter() -> u32 { 220 }

impl Default for FilemanConfig {
    fn default() -> Self {
        Self {
            window: WindowSection {
                window_title: default_title(),
                remember_window_size: default_true(),
                last_window_width: default_width(),
                last_window_height: default_height(),
                splitter_pos: default_splitter(),
            },
            folder_view: FolderViewSection {
                show_hidden: default_false(),
                default_path: None,
            },
            behavior: BehaviorSection {
                confirm_delete: default_true(),
                use_trash: default_true(),
            },
        }
    }
}

impl FilemanConfig {
    pub fn load_or_create() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("fileman");
        let _ = std::fs::create_dir_all(&config_dir);
        let config_path = config_dir.join("config.toml");

        if !config_path.exists() {
            let default_cfg = Self::default();
            if let Ok(toml_str) = toml::to_string_pretty(&default_cfg) {
                let _ = std::fs::write(&config_path, toml_str);
            }
            default_cfg
        } else {
            match std::fs::read_to_string(&config_path) {
                Ok(content) => toml::from_str(&content).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        }
    }

    pub fn save(&self) {
        if let Some(config_dir) = dirs::config_dir() {
            let config_path = config_dir.join("fileman").join("config.toml");
            if let Ok(toml_str) = toml::to_string_pretty(self) {
                let _ = std::fs::write(config_path, toml_str);
            }
        }
    }
}
