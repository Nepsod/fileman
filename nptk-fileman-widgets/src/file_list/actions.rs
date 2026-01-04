use super::{FileListContent, PendingAction};
use nptk::core::menu::{MenuItem, MenuCommand};
use nptk::core::app::update::Update;
use npio::service::filesystem::mime_registry::MimeRegistry;
use npio::service::filesystem::mime_detector::MimeDetector;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

impl FileListContent {
    pub(super) fn launch_path(registry: MimeRegistry, path: PathBuf) {
        let mime = smol::block_on(MimeDetector::detect_mime_type(&path)).or_else(|| Self::xdg_mime_filetype(&path));
        let Some(mime) = mime else {
            log::warn!("Could not detect MIME type for {:?}", path);
            return;
        };

        let app = registry.resolve(&mime).or_else(|| {
            let handlers = registry.list_handlers(&mime);
            handlers.into_iter().next()
        });

        if let Some(app_id) = app {
            if let Err(err) = registry.launch(&app_id, &path) {
                log::warn!("Failed to launch app '{}': {}", app_id, err);
            }
            return;
        }

        match Command::new("xdg-open").arg(path).spawn() {
            Ok(_) => {},
            Err(err) => {
                log::warn!(
                    "No application found for MIME {} and xdg-open failed: {}",
                    mime,
                    err
                );
            },
        }
    }

    pub(super) fn open_label_for_path(&self, path: &Path) -> String {
        if path.is_dir() {
            return "Open".to_string();
        }

        let mime = smol::block_on(MimeDetector::detect_mime_type(path)).or_else(|| Self::xdg_mime_filetype(path));
        let Some(mime) = mime else {
            return "Open".to_string();
        };

        let mime_variants = Self::get_mime_variants(&mime);
        for variant in &mime_variants {
            if let Some((_, name)) = self.mime_registry.resolve_with_name(variant) {
                return format!("Open with {}", name);
            }

            let handlers = self.mime_registry.list_handlers(variant);
            if let Some(app_id) = handlers.into_iter().next() {
                let name = self.display_name_for_appid(&app_id);
                return format!("Open with {}", name);
            }

            if let Some(app_id) = Self::xdg_default_for_mime(variant) {
                let name = self.display_name_for_appid(&app_id);
                return format!("Open with {}", name);
            }
        }

        "Open".to_string()
    }

    pub(super) fn build_open_with_items(
        &self,
        path: &Path,
        selection: Vec<PathBuf>,
    ) -> Vec<MenuItem> {
        let mut items = Vec::new();

        let mime = smol::block_on(MimeDetector::detect_mime_type(path)).or_else(|| Self::xdg_mime_filetype(path));
        let Some(mime) = mime else {
            return items;
        };

        let variants = Self::get_mime_variants(&mime);
        let mut seen: HashSet<String> = HashSet::new();
        let mut handlers: Vec<String> = Vec::new();

        for variant in variants {
            if let Some(default_id) = self.mime_registry.resolve(&variant) {
                if seen.insert(default_id.clone()) {
                    handlers.push(default_id);
                }
            }
            for app_id in self.mime_registry.list_handlers(&variant) {
                if seen.insert(app_id.clone()) {
                    handlers.push(app_id);
                }
            }
            if let Some(app_id) = Self::xdg_default_for_mime(&variant) {
                if seen.insert(app_id.clone()) {
                    handlers.push(app_id);
                }
            }
        }

        let mut command_id = 0x3000u32; // Start from 0x3000 for open-with commands
        for app_id in handlers {
            let label = self.display_name_for_appid(&app_id);
            let pending = self.pending_action.clone();
            let paths_for_action = selection.clone();
            let app_id_cloned = app_id.clone();
            let cmd = MenuCommand::Custom(command_id);
            command_id += 1;
            
            items.push(
                MenuItem::new(cmd, label)
                    .with_action(move || {
                        if let Ok(mut pending_lock) = pending.lock() {
                            *pending_lock = Some(PendingAction {
                                paths: paths_for_action.clone(),
                                app_id: Some(app_id_cloned.clone()),
                                properties: false,
                            });
                        }
                        Update::DRAW
                    }),
            );
        }

        items
    }

    fn get_mime_variants(mime: &str) -> Vec<String> {
        let mut variants = vec![mime.to_string()];

        match mime {
            "text/x-toml" => {
                variants.push("application/toml".to_string());
                variants.push("text/plain".to_string());
            },
            "application/toml" => {
                variants.push("text/plain".to_string());
            },
            "text/x-rust" => {
                variants.push("text/plain".to_string());
            },
            other if other.starts_with("text/") => {
                if other != "text/plain" {
                    variants.push("text/plain".to_string());
                }
            },
            other
                if other.starts_with("application/")
                    && (other.contains("json")
                        || other.contains("xml")
                        || other.contains("yaml")
                        || other.contains("toml")
                        || other.contains("markdown")) =>
            {
                variants.push("text/plain".to_string());
            },
            _ => {},
        }

        variants
    }

    pub(super) fn xdg_mime_filetype(path: &Path) -> Option<String> {
        let output = Command::new("xdg-mime")
            .args(["query", "filetype", path.to_string_lossy().as_ref()])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mime = stdout.trim();
        if mime.is_empty() {
            None
        } else {
            Some(mime.to_string())
        }
    }

    fn xdg_default_for_mime(mime: &str) -> Option<String> {
        let output = Command::new("xdg-mime")
            .args(["query", "default", mime])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let id = stdout.trim();
        if id.is_empty() {
            None
        } else {
            Some(id.to_string())
        }
    }

    fn display_name_for_appid(&self, app_id: &str) -> String {
        self.mime_registry.name_or_prettify(app_id)
    }
}
