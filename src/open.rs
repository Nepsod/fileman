use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use npio::service::filesystem::mime_detector::MimeDetector;
use npio::service::filesystem::mime_registry::MimeRegistry;

#[derive(Debug, Clone)]
pub struct OpenWithHandler {
    pub app_id: String,
    pub label: String,
}

pub async fn launch_path(path: PathBuf) -> Result<(), String> {
    if path.is_dir() {
        return Err("Cannot open a directory with an application".to_string());
    }

    let registry = MimeRegistry::load_default();
    let mime = MimeDetector::detect_mime_type(&path)
        .await
        .or_else(|| xdg_mime_filetype(&path));

    let Some(mime) = mime else {
        return Err(format!("Could not detect MIME type for {}", path.display()));
    };

    if let Some(application_id) = resolve_default_handler(&registry, &mime) {
        registry
            .launch(&application_id, &path)
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    Command::new("xdg-open")
        .arg(&path)
        .spawn()
        .map_err(|error| format!("No application for {mime}: {error}"))?;
    Ok(())
}

pub async fn launch_with_application(app_id: &str, path: PathBuf) -> Result<(), String> {
    if path.is_dir() {
        return Err("Cannot open a directory with an application".to_string());
    }

    let registry = MimeRegistry::load_default();
    registry
        .launch(app_id, &path)
        .map_err(|error| error.to_string())
}

pub fn open_label_for_path(path: &Path) -> String {
    if path.is_dir() {
        return "Open".to_string();
    }

    let Some(mime) = xdg_mime_filetype(path) else {
        return "Open".to_string();
    };

    let registry = MimeRegistry::load_default();
    for variant in mime_variants(&mime) {
        if let Some((_, name)) = registry.resolve_with_name(&variant) {
            return format!("Open with {name}");
        }

        if let Some(application_id) = registry.list_handlers(&variant).into_iter().next() {
            return format!(
                "Open with {}",
                registry.name_or_prettify(&application_id)
            );
        }

        if let Some(application_id) = xdg_default_for_mime(&variant) {
            return format!(
                "Open with {}",
                registry.name_or_prettify(&application_id)
            );
        }
    }

    "Open".to_string()
}

pub fn handlers_for_path(path: &Path) -> Vec<OpenWithHandler> {
    if path.is_dir() {
        return Vec::new();
    }

    let Some(mime) = xdg_mime_filetype(path) else {
        return Vec::new();
    };

    let registry = MimeRegistry::load_default();
    let mut seen = HashSet::new();
    let mut handlers = Vec::new();

    for variant in mime_variants(&mime) {
        if let Some(application_id) = registry.resolve(&variant) {
            push_handler(&mut handlers, &mut seen, &registry, application_id);
        }
        for application_id in registry.list_handlers(&variant) {
            push_handler(&mut handlers, &mut seen, &registry, application_id);
        }
        if let Some(application_id) = xdg_default_for_mime(&variant) {
            push_handler(&mut handlers, &mut seen, &registry, application_id);
        }
    }

    handlers
}

fn push_handler(
    handlers: &mut Vec<OpenWithHandler>,
    seen: &mut HashSet<String>,
    registry: &MimeRegistry,
    application_id: String,
) {
    if seen.insert(application_id.clone()) {
        handlers.push(OpenWithHandler {
            label: registry.name_or_prettify(&application_id),
            app_id: application_id,
        });
    }
}

fn resolve_default_handler(registry: &MimeRegistry, mime: &str) -> Option<String> {
    for variant in mime_variants(mime) {
        if let Some(application_id) = registry.resolve(&variant) {
            return Some(application_id);
        }
        if let Some(application_id) = registry.list_handlers(&variant).into_iter().next() {
            return Some(application_id);
        }
        if let Some(application_id) = xdg_default_for_mime(&variant) {
            return Some(application_id);
        }
    }
    None
}

pub fn mime_variants(mime: &str) -> Vec<String> {
    let mut variants = vec![mime.to_string()];

    match mime {
        "text/x-toml" => {
            variants.push("application/toml".to_string());
            variants.push("text/plain".to_string());
        }
        "application/toml" => {
            variants.push("text/plain".to_string());
        }
        "text/x-rust" => {
            variants.push("text/plain".to_string());
        }
        other if other.starts_with("text/") => {
            if other != "text/plain" {
                variants.push("text/plain".to_string());
            }
        }
        other
            if other.starts_with("application/")
                && (other.contains("json")
                    || other.contains("xml")
                    || other.contains("yaml")
                    || other.contains("toml")
                    || other.contains("markdown")) =>
        {
            variants.push("text/plain".to_string());
        }
        _ => {}
    }

    variants
}

fn xdg_mime_filetype(path: &Path) -> Option<String> {
    let output = Command::new("xdg-mime")
        .args(["query", "filetype", path.to_string_lossy().as_ref()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mime = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if mime.is_empty() {
        None
    } else {
        Some(mime)
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
    let application_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if application_id.is_empty() {
        None
    } else {
        Some(application_id)
    }
}
