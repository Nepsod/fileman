//! `config.toml` under XDG config dir (`~/.config/fileman/config.toml`).
//! On startup, the config directory and a default `config.toml` are created when missing.

const ICON_SIZE_MIN: u32 = 16;
const ICON_SIZE_MAX: u32 = 256;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nptk::core::config::{MayConfig, WindowConfig};
use nptk::core::model::SortOrder;
use nptk_fileman_widgets::file_list::FileListViewMode;
use serde::Deserialize;
use toml_edit::{DocumentMut, Item, Table, value};
use xdg::BaseDirectories;

fn set_or_replace_value(table: &mut Table, key: &str, new_item: Item) {
    let Item::Value(new_val) = new_item else {
        table.insert(key, new_item);
        return;
    };
    if let Some(item) = table.get_mut(key) {
        if let Some(v) = item.as_value_mut() {
            *v = new_val;
            return;
        }
        *item = Item::Value(new_val);
    } else {
        table.insert(key, Item::Value(new_val));
    }
}

/// Default shipped template written when `config.toml` does not exist yet.
const DEFAULT_CONFIG_TEMPLATE: &str = r#"# Edit values or add optional sections; unknown top-level sections are ignored.

[Window]
WindowTitle = "Fileman"
RememberWindowSize = true
LastWindowWidth = 998
LastWindowHeight = 698
LastWindowMaximized = false
SplitterPos = 175
# FixedWidth = 640
# FixedHeight = 480

[FolderView]
# DefaultPath = "/home/you/Documents"
Mode = "list"
ShowHidden = false
SortColumn = "name"
SortOrder = "ascending"
BigIconSize = 48
# SmallIconSize = 24
# ThumbnailIconSize = 128

[Behavior]
ConfirmDelete = true
ConfirmTrash = true
# When false, "move to trash" uses permanent delete instead of the trash crate.
UseTrash = true

[System]
# Example: Terminal = "kitty"
# Leave empty: uses $TERMINAL, then system fallbacks.
Terminal = ""

[Desktop]
# Ignored by fileman (forward-compatible stubs from PCManFM-style files).
"#;

/// Loaded fileman configuration (MVP keys only; unknown tables kept in `extra`).
#[derive(Debug, Clone, Deserialize)]
pub struct FilemanConfig {
    #[serde(rename = "Window", default)]
    pub window: WindowSection,
    #[serde(rename = "FolderView", default)]
    pub folder_view: FolderViewSection,
    #[serde(rename = "Behavior", default)]
    pub behavior: BehaviorSection,
    #[serde(rename = "System", default)]
    pub system: SystemSection,
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WindowSection {
    #[serde(rename = "WindowTitle")]
    pub window_title: Option<String>,
    #[serde(rename = "LastWindowWidth")]
    pub last_window_width: Option<i64>,
    #[serde(rename = "LastWindowHeight")]
    pub last_window_height: Option<i64>,
    #[serde(rename = "LastWindowMaximized")]
    pub last_window_maximized: Option<bool>,
    #[serde(rename = "RememberWindowSize")]
    pub remember_window_size: Option<bool>,
    #[serde(rename = "FixedWidth")]
    pub fixed_width: Option<i64>,
    #[serde(rename = "FixedHeight")]
    pub fixed_height: Option<i64>,
    #[serde(rename = "SplitterPos")]
    pub splitter_pos: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FolderViewSection {
    #[serde(rename = "Mode")]
    pub mode: Option<String>,
    #[serde(rename = "ShowHidden")]
    pub show_hidden: Option<bool>,
    #[serde(rename = "SortColumn")]
    pub sort_column: Option<String>,
    #[serde(rename = "SortOrder")]
    pub sort_order: Option<String>,
    #[serde(rename = "BigIconSize")]
    pub big_icon_size: Option<u32>,
    #[serde(rename = "SmallIconSize")]
    pub small_icon_size: Option<u32>,
    #[serde(rename = "ThumbnailIconSize")]
    pub thumbnail_icon_size: Option<u32>,
    /// Startup folder when the CLI does not pass a path.
    #[serde(rename = "DefaultPath")]
    pub default_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BehaviorSection {
    #[serde(rename = "ConfirmDelete")]
    pub confirm_delete: Option<bool>,
    #[serde(rename = "ConfirmTrash")]
    pub confirm_trash: Option<bool>,
    /// When `false`, trash operations use permanent delete (no trash crate).
    #[serde(rename = "UseTrash")]
    pub use_trash: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SystemSection {
    #[serde(rename = "Terminal")]
    pub terminal: Option<String>,
}

/// Delete / trash behavior applied by [`crate::window::FileListWrapper`].
#[derive(Debug, Clone, Copy)]
pub struct DeletePolicy {
    pub confirm_delete: bool,
    pub confirm_trash: bool,
    pub use_trash: bool,
}

impl Default for DeletePolicy {
    fn default() -> Self {
        Self {
            confirm_delete: true,
            confirm_trash: true,
            use_trash: true,
        }
    }
}

impl Default for FilemanConfig {
    fn default() -> Self {
        Self {
            window: WindowSection::default(),
            folder_view: FolderViewSection::default(),
            behavior: BehaviorSection::default(),
            system: SystemSection::default(),
            extra: HashMap::new(),
        }
    }
}

impl FilemanConfig {
    /// Ensures `~/.config/fileman` exists, writes default `config.toml` if missing, then loads.
    pub fn load_or_create() -> Self {
        let Ok(xdg) = BaseDirectories::with_prefix("fileman") else {
            log::warn!("fileman: XDG base directories unavailable, using default config");
            return Self::default();
        };

        let config_dir = xdg.get_config_home();
        if let Err(e) = std::fs::create_dir_all(&config_dir) {
            log::warn!(
                "fileman: failed to create config directory {:?}: {}",
                config_dir,
                e
            );
            return Self::default();
        }

        let path = config_dir.join("config.toml");
        if !path.is_file() {
            match std::fs::write(&path, DEFAULT_CONFIG_TEMPLATE) {
                Ok(()) => log::info!("fileman: created default config at {:?}", path),
                Err(e) => log::warn!("fileman: failed to write default {:?}: {}", path, e),
            }
        }

        Self::load_from_path(&path)
    }

    /// Load without creating files (e.g. tests). Uses `find_config_file` or `get_config_home()/config.toml`.
    pub fn load() -> Self {
        let Ok(xdg) = BaseDirectories::with_prefix("fileman") else {
            log::warn!("fileman: XDG base directories unavailable, using default config");
            return Self::default();
        };

        let path = xdg
            .find_config_file("config.toml")
            .unwrap_or_else(|| xdg.get_config_home().join("config.toml"));

        Self::load_from_path(&path)
    }

    pub(crate) fn load_from_path(path: &Path) -> Self {
        if !path.is_file() {
            log::debug!("fileman: no config at {:?}, using defaults", path);
            return Self::default();
        }

        match std::fs::read_to_string(path) {
            Ok(content) => match toml::from_str::<FilemanConfig>(&content) {
                Ok(cfg) => {
                    log::info!("fileman: loaded config from {:?}", path);
                    cfg
                }
                Err(e) => {
                    log::error!("fileman: failed to parse {:?}: {}", path, e);
                    Self::default()
                }
            },
            Err(e) => {
                log::warn!("fileman: failed to read {:?}: {}", path, e);
                Self::default()
            }
        }
    }

    pub fn may_config(&self) -> MayConfig {
        let mut may = MayConfig::default();
        self.apply_to_window(&mut may.window);
        may
    }

    pub fn apply_to_window(&self, window: &mut WindowConfig) {
        if let Some(ref title) = self.window.window_title {
            if !title.is_empty() {
                window.title = title.clone();
            }
        }

        let remember = self.window.remember_window_size.unwrap_or(true);
        if remember {
            if let Some(width) = self.window.last_window_width {
                if width > 0 {
                    window.size.x = width as f64;
                }
            }
            if let Some(height) = self.window.last_window_height {
                if height > 0 {
                    window.size.y = height as f64;
                }
            }
        } else {
            if let Some(fw) = self.window.fixed_width {
                if fw > 0 {
                    window.size.x = fw as f64;
                }
            }
            if let Some(fh) = self.window.fixed_height {
                if fh > 0 {
                    window.size.y = fh as f64;
                }
            }
        }

        if let Some(max) = self.window.last_window_maximized {
            window.maximized = max;
        }
    }

    pub fn sidebar_width(&self) -> f64 {
        self.window
            .splitter_pos
            .filter(|&p| p > 0)
            .map(|p| p as f64)
            .unwrap_or(200.0)
    }

    pub fn initial_show_hidden(&self) -> bool {
        self.folder_view.show_hidden.unwrap_or(false)
    }

    pub fn default_folder_path(&self) -> Option<PathBuf> {
        self.folder_view
            .default_path
            .clone()
            .filter(|p| !p.as_os_str().is_empty())
    }

    pub fn default_view_mode(&self) -> Option<FileListViewMode> {
        self.folder_view.mode.as_deref().and_then(|v| {
            match v.trim().to_lowercase().as_str() {
                "icon" | "thumbnail" | "thumbnails" => Some(FileListViewMode::Icon),
                "compact" => Some(FileListViewMode::Compact),
                "list" => Some(FileListViewMode::List),
                "table" | "detailed" | "detail" => Some(FileListViewMode::Table),
                _ => {
                    log::warn!(
                        "fileman: unknown [FolderView].Mode {:?}, expected list|icon|compact|table (aliases: detailed, thumbnail)",
                        v
                    );
                    None
                }
            }
        })
    }

    fn parse_sort_column(col: &str) -> Option<usize> {
        match col.trim().to_lowercase().as_str() {
            "name" => Some(0usize),
            "size" => Some(1),
            "type" => Some(2),
            "date" | "modified" => Some(3),
            _ => {
                log::warn!(
                    "fileman: unknown [FolderView].SortColumn {:?}, expected name|size|type|date",
                    col
                );
                None
            }
        }
    }

    fn parse_sort_order(s: &str) -> Option<SortOrder> {
        match s.trim().to_lowercase().as_str() {
            "ascending" | "asc" => Some(SortOrder::Ascending),
            "descending" | "desc" => Some(SortOrder::Descending),
            _ => {
                log::warn!(
                    "fileman: unknown [FolderView].SortOrder {:?}, expected ascending|descending",
                    s
                );
                None
            }
        }
    }

    /// Initial sort: column 0–3 and order. Partial keys work (`SortColumn` alone defaults order to ascending; `SortOrder` alone uses name column).
    pub fn initial_sort(&self) -> Option<(usize, SortOrder)> {
        let col_opt = self
            .folder_view
            .sort_column
            .as_deref()
            .and_then(Self::parse_sort_column);
        let order_opt = self
            .folder_view
            .sort_order
            .as_deref()
            .and_then(Self::parse_sort_order);

        match (col_opt, order_opt) {
            (Some(c), Some(o)) => Some((c, o)),
            (Some(c), None) => Some((c, SortOrder::Ascending)),
            (None, Some(o)) => Some((0, o)),
            (None, None) => None,
        }
    }

    pub fn initial_icon_size(&self) -> Option<u32> {
        let fv = &self.folder_view;
        let raw = fv
            .big_icon_size
            .or(fv.thumbnail_icon_size)
            .or(fv.small_icon_size)?;
        Some(raw.clamp(ICON_SIZE_MIN, ICON_SIZE_MAX))
    }

    pub fn delete_policy(&self) -> DeletePolicy {
        DeletePolicy {
            confirm_delete: self.behavior.confirm_delete.unwrap_or(true),
            confirm_trash: self.behavior.confirm_trash.unwrap_or(true),
            use_trash: self.behavior.use_trash.unwrap_or(true),
        }
    }

    /// Configured terminal command (binary or `binary args`). `TERMINAL` env wins at spawn time.
    pub fn terminal_command(&self) -> Option<&str> {
        self.system
            .terminal
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// XDG path to `config.toml` (`…/fileman/config.toml`), if base directories resolve.
    pub fn config_file_path() -> Option<PathBuf> {
        let xdg = BaseDirectories::with_prefix("fileman").ok()?;
        Some(xdg.get_config_home().join("config.toml"))
    }
}

/// Writes `[Window].LastWindowWidth`, `LastWindowHeight`, and `LastWindowMaximized` using a TOML AST
/// so existing comments and commented-out lines stay in the file.
/// When `maximized` is true, previous width/height are kept so restore size stays valid after unmaximize.
pub(crate) fn persist_window_geometry(
    path: &Path,
    width: i64,
    height: i64,
    maximized: bool,
) -> std::io::Result<()> {
    if !maximized && (width <= 0 || height <= 0) {
        return Ok(());
    }
    let content = std::fs::read_to_string(path)?;
    let mut doc: DocumentMut = content
        .parse::<DocumentMut>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    let window_item = doc.entry("Window").or_insert(Item::Table(Table::new()));
    let window = window_item.as_table_mut().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "fileman: config key \"Window\" must be a table",
        )
    })?;

    set_or_replace_value(window, "LastWindowMaximized", value(maximized));
    if !maximized {
        set_or_replace_value(window, "LastWindowWidth", value(width));
        set_or_replace_value(window, "LastWindowHeight", value(height));
    }

    std::fs::write(path, doc.to_string())
}

/// Values written by the Configure Fileman dialog (`[FolderView]`, `[Behavior]`, `[System]`, `[Window]` subset).
#[derive(Debug, Clone)]
pub struct UserSettingsPersist {
    pub show_hidden: bool,
    pub confirm_delete: bool,
    pub confirm_trash: bool,
    pub use_trash: bool,
    pub remember_window_size: bool,
    pub terminal: String,
}

/// Updates user preference keys in `config.toml` via TOML AST (preserves comments and unrelated keys).
pub(crate) fn persist_user_settings(
    path: &Path,
    patch: &UserSettingsPersist,
) -> std::io::Result<()> {
    let content = if path.is_file() {
        std::fs::read_to_string(path)?
    } else {
        DEFAULT_CONFIG_TEMPLATE.to_string()
    };
    let mut doc: DocumentMut = content
        .parse::<DocumentMut>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    let folder = doc.entry("FolderView").or_insert(Item::Table(Table::new()));
    let folder = folder.as_table_mut().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "fileman: config key \"FolderView\" must be a table",
        )
    })?;
    set_or_replace_value(folder, "ShowHidden", value(patch.show_hidden));

    let behavior = doc.entry("Behavior").or_insert(Item::Table(Table::new()));
    let behavior = behavior.as_table_mut().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "fileman: config key \"Behavior\" must be a table",
        )
    })?;
    set_or_replace_value(behavior, "ConfirmDelete", value(patch.confirm_delete));
    set_or_replace_value(behavior, "ConfirmTrash", value(patch.confirm_trash));
    set_or_replace_value(behavior, "UseTrash", value(patch.use_trash));

    let system = doc.entry("System").or_insert(Item::Table(Table::new()));
    let system = system.as_table_mut().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "fileman: config key \"System\" must be a table",
        )
    })?;
    set_or_replace_value(system, "Terminal", value(patch.terminal.as_str()));

    let window = doc.entry("Window").or_insert(Item::Table(Table::new()));
    let window = window.as_table_mut().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "fileman: config key \"Window\" must be a table",
        )
    })?;
    set_or_replace_value(
        window,
        "RememberWindowSize",
        value(patch.remember_window_size),
    );

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nptk::core::config::WindowConfig;
    use nptk::core::model::SortOrder;
    use nptk_fileman_widgets::file_list::FileListViewMode;
    use std::sync::{Mutex, OnceLock};

    fn parse_toml(raw: &str) -> FilemanConfig {
        toml::from_str(raw).expect("valid TOML for FilemanConfig")
    }

    #[test]
    fn default_template_deserializes_and_has_expected_sections() {
        let cfg = parse_toml(DEFAULT_CONFIG_TEMPLATE);
        assert_eq!(cfg.window.window_title.as_deref(), Some("Fileman"));
        assert_eq!(cfg.window.last_window_width, Some(998));
        assert_eq!(cfg.window.last_window_height, Some(698));
        assert_eq!(cfg.window.splitter_pos, Some(175));
        assert_eq!(cfg.folder_view.mode.as_deref(), Some("list"));
        assert_eq!(cfg.folder_view.show_hidden, Some(false));
        assert_eq!(cfg.folder_view.sort_column.as_deref(), Some("name"));
        assert!(!cfg.extra.is_empty());
        assert!(cfg.extra.contains_key("Desktop"));
    }

    #[test]
    fn load_from_path_missing_file_returns_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("does-not-exist.toml");
        let cfg = FilemanConfig::load_from_path(&path);
        assert_eq!(cfg.window.window_title, None);
        assert!(cfg.extra.is_empty());
    }

    #[test]
    fn load_from_path_reads_valid_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[Window]
WindowTitle = "Test"
[FolderView]
Mode = "compact"
"#,
        )
        .expect("write config");
        let cfg = FilemanConfig::load_from_path(&path);
        assert_eq!(cfg.window.window_title.as_deref(), Some("Test"));
        assert_eq!(cfg.default_view_mode(), Some(FileListViewMode::Compact));
    }

    #[test]
    fn initial_sort_partial_column_defaults_to_ascending() {
        let cfg = parse_toml(
            r#"[FolderView]
SortColumn = "size"
"#,
        );
        assert_eq!(cfg.initial_sort(), Some((1, SortOrder::Ascending)));
    }

    #[test]
    fn initial_sort_partial_order_uses_name_column() {
        let cfg = parse_toml(
            r#"[FolderView]
SortOrder = "desc"
"#,
        );
        assert_eq!(cfg.initial_sort(), Some((0, SortOrder::Descending)));
    }

    #[test]
    fn default_view_mode_aliases() {
        for (toml_val, expected) in [
            ("detailed", FileListViewMode::Table),
            ("table", FileListViewMode::Table),
            ("thumbnail", FileListViewMode::Icon),
            ("icon", FileListViewMode::Icon),
        ] {
            let cfg = parse_toml(&format!(
                "[FolderView]\nMode = \"{toml_val}\"\n"
            ));
            assert_eq!(cfg.default_view_mode(), Some(expected), "{toml_val}");
        }
    }

    #[test]
    fn initial_icon_size_prefers_big_then_thumbnail_then_small() {
        let mut cfg = parse_toml(
            r#"[FolderView]
BigIconSize = 48
ThumbnailIconSize = 96
SmallIconSize = 24
"#,
        );
        assert_eq!(cfg.initial_icon_size(), Some(48));

        cfg.folder_view.big_icon_size = None;
        assert_eq!(cfg.initial_icon_size(), Some(96));

        cfg.folder_view.thumbnail_icon_size = None;
        assert_eq!(cfg.initial_icon_size(), Some(24));
    }

    #[test]
    fn initial_icon_size_clamps_to_bounds() {
        let cfg = parse_toml(
            r#"[FolderView]
BigIconSize = 999
"#,
        );
        assert_eq!(cfg.initial_icon_size(), Some(256));
        let cfg = parse_toml(
            r#"[FolderView]
BigIconSize = 8
"#,
        );
        assert_eq!(cfg.initial_icon_size(), Some(16));
    }

    #[test]
    fn apply_to_window_remember_last_size() {
        let cfg = parse_toml(
            r#"[Window]
RememberWindowSize = true
LastWindowWidth = 1024
LastWindowHeight = 768
"#,
        );
        let mut window = WindowConfig::default();
        cfg.apply_to_window(&mut window);
        assert_eq!(window.size.x, 1024.0);
        assert_eq!(window.size.y, 768.0);
    }

    #[test]
    fn apply_to_window_fixed_size_when_not_remember() {
        let cfg = parse_toml(
            r#"[Window]
RememberWindowSize = false
FixedWidth = 640
FixedHeight = 480
"#,
        );
        let mut window = WindowConfig::default();
        cfg.apply_to_window(&mut window);
        assert_eq!(window.size.x, 640.0);
        assert_eq!(window.size.y, 480.0);
    }

    #[test]
    fn apply_to_window_skips_non_positive_dimensions() {
        let cfg = parse_toml(
            r#"[Window]
RememberWindowSize = true
LastWindowWidth = 0
LastWindowHeight = -1
"#,
        );
        let mut window = WindowConfig::default();
        let default_x = window.size.x;
        let default_y = window.size.y;
        cfg.apply_to_window(&mut window);
        assert_eq!(window.size.x, default_x);
        assert_eq!(window.size.y, default_y);
    }

    #[test]
    fn sidebar_width_default_and_positive() {
        let cfg_default = FilemanConfig::default();
        assert_eq!(cfg_default.sidebar_width(), 200.0);
        let cfg = parse_toml(
            r#"[Window]
SplitterPos = 175
"#,
        );
        assert_eq!(cfg.sidebar_width(), 175.0);
    }

    #[test]
    fn delete_policy_defaults_and_overrides() {
        let def = FilemanConfig::default().delete_policy();
        assert!(def.confirm_delete);
        assert!(def.confirm_trash);
        assert!(def.use_trash);
        let cfg = parse_toml(
            r#"[Behavior]
ConfirmDelete = false
ConfirmTrash = false
UseTrash = false
"#,
        );
        let pol = cfg.delete_policy();
        assert!(!pol.confirm_delete);
        assert!(!pol.confirm_trash);
        assert!(!pol.use_trash);
    }

    #[test]
    fn terminal_command_trims_and_ignores_empty() {
        let empty = parse_toml(
            r#"[System]
Terminal = ""
"#,
        );
        assert_eq!(empty.terminal_command(), None);
        let spaced = parse_toml(
            r#"[System]
Terminal = "  kitty -e bash  "
"#,
        );
        assert_eq!(spaced.terminal_command(), Some("kitty -e bash"));
    }

    #[test]
    fn default_folder_path_filters_empty_path() {
        let cfg = parse_toml(
            r#"[FolderView]
DefaultPath = "/tmp/fileman-test"
"#,
        );
        assert_eq!(
            cfg.default_folder_path().map(|p| p.to_string_lossy().to_string()),
            Some("/tmp/fileman-test".to_string())
        );
    }

    #[test]
    fn persist_window_geometry_updates_when_not_maximized() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[Window]
LastWindowWidth = 100
LastWindowHeight = 200
RememberWindowSize = true
[FolderView]
"#,
        )
        .expect("write");
        super::persist_window_geometry(&path, 640, 480, false).expect("persist");
        let cfg = parse_toml(&std::fs::read_to_string(&path).expect("read"));
        assert_eq!(cfg.window.last_window_width, Some(640));
        assert_eq!(cfg.window.last_window_height, Some(480));
        assert_eq!(cfg.window.last_window_maximized, Some(false));
    }

    #[test]
    fn persist_window_geometry_keeps_dimensions_when_maximized() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[Window]
LastWindowWidth = 100
LastWindowHeight = 200
[FolderView]
"#,
        )
        .expect("write");
        super::persist_window_geometry(&path, 9999, 8888, true).expect("persist");
        let cfg = parse_toml(&std::fs::read_to_string(&path).expect("read"));
        assert_eq!(cfg.window.last_window_width, Some(100));
        assert_eq!(cfg.window.last_window_height, Some(200));
        assert_eq!(cfg.window.last_window_maximized, Some(true));
    }

    #[test]
    fn persist_window_geometry_preserves_comments_and_commented_keys() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let before = r#"# Fileman user notes
[Window]
# keep window geometry
LastWindowWidth = 100
# FixedWidth = 640
LastWindowHeight = 200
LastWindowMaximized = false

[FolderView]
Mode = "list"
"#;
        std::fs::write(&path, before).expect("write");
        super::persist_window_geometry(&path, 640, 480, false).expect("persist");
        let after = std::fs::read_to_string(&path).expect("read");
        assert!(
            after.contains("# Fileman user notes"),
            "lost top comment:\n{after}"
        );
        assert!(after.contains("# keep window geometry"), "lost section comment");
        assert!(
            after.contains("# FixedWidth = 640"),
            "lost commented-out line"
        );
        let cfg = parse_toml(&after);
        assert_eq!(cfg.window.last_window_width, Some(640));
        assert_eq!(cfg.window.last_window_height, Some(480));
        assert_eq!(cfg.window.last_window_maximized, Some(false));
    }

    #[test]
    fn persist_user_settings_updates_sections_and_preserves_other_keys() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# header
[Window]
RememberWindowSize = true
WindowTitle = "KeepMe"

[FolderView]
ShowHidden = false
Mode = "list"

[Behavior]
ConfirmDelete = true
ConfirmTrash = true
UseTrash = true

[System]
Terminal = ""

[Desktop]
"#,
        )
        .expect("write");
        let patch = UserSettingsPersist {
            show_hidden: true,
            confirm_delete: false,
            confirm_trash: false,
            use_trash: false,
            remember_window_size: false,
            terminal: "alacritty".to_string(),
        };
        super::persist_user_settings(&path, &patch).expect("persist");
        let raw = std::fs::read_to_string(&path).expect("read");
        assert!(raw.contains("# header"));
        assert!(raw.contains("WindowTitle = \"KeepMe\""));
        assert!(raw.contains("[Desktop]"));
        let cfg = parse_toml(&raw);
        assert_eq!(cfg.folder_view.show_hidden, Some(true));
        assert_eq!(cfg.behavior.confirm_delete, Some(false));
        assert_eq!(cfg.system.terminal.as_deref(), Some("alacritty"));
        assert_eq!(cfg.window.remember_window_size, Some(false));
    }

    static LOAD_OR_CREATE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn load_or_create_writes_default_when_missing() {
        let lock = LOAD_OR_CREATE_LOCK.get_or_init(|| Mutex::new(()));
        let _guard = lock.lock().expect("lock");

        let dir = tempfile::tempdir().expect("tempdir");
        let config_home = dir.path();
        let prev = std::env::var_os("XDG_CONFIG_HOME");
        // Serialized by LOAD_OR_CREATE_LOCK; other threads must not read env concurrently.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", config_home.as_os_str());
        }

        let cfg = FilemanConfig::load_or_create();

        unsafe {
            if let Some(ref previous) = prev {
                std::env::set_var("XDG_CONFIG_HOME", previous);
            } else {
                std::env::remove_var("XDG_CONFIG_HOME");
            }
        }

        let written = config_home.join("fileman").join("config.toml");
        assert!(written.is_file());

        assert!(cfg.window.window_title.is_some());
        assert_eq!(cfg.sidebar_width(), 175.0);
    }
}
