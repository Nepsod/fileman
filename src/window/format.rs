use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use nptk::std::fs;
use npio::{FileInfo, FileType};

pub(crate) fn delete_confirmation_message(paths: &[PathBuf], permanent: bool) -> String {
    if paths.len() == 1 {
        let name = paths[0]
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .unwrap_or("<unnamed>");
        if permanent {
            format!("Permanently delete \"{name}\"? This cannot be undone.")
        } else {
            format!("Move \"{name}\" to the trash?")
        }
    } else if permanent {
        format!(
            "Permanently delete {} selected items? This cannot be undone.",
            paths.len()
        )
    } else {
        format!("Move {} selected items to the trash?", paths.len())
    }
}

pub(crate) fn quick_access_places() -> Vec<(&'static str, PathBuf)> {
    let mut places = vec![("Root", PathBuf::from("/"))];
    if let Some(home) = dirs::home_dir() {
        places.push(("Home", home.clone()));
        for (label, path) in [
            ("Desktop", home.join("Desktop")),
            ("Documents", home.join("Documents")),
            ("Downloads", home.join("Downloads")),
            ("Music", home.join("Music")),
            ("Pictures", home.join("Pictures")),
            ("Videos", home.join("Videos")),
        ] {
            if path.is_dir() {
                places.push((label, path));
            }
        }
    }
    places
}

pub(crate) fn path_to_file_uri(path: &Path) -> String {
    let absolute = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf());
    let path_string = absolute.to_string_lossy();
    if path_string.starts_with('/') {
        format!("file://{path_string}")
    } else {
        format!("file:///{path_string}")
    }
}

pub(crate) fn table_columns_for_path(path: &Path, is_directory: bool) -> (String, String, String) {
    if is_directory {
        let type_display = "File folder".to_string();
        let modified_display = path_modified_display(path);
        return ("--".to_string(), type_display, modified_display);
    }

    let metadata = fs::metadata(path).ok();
    let size_display = metadata
        .as_ref()
        .map(|meta| format_size(meta.len() as i64))
        .unwrap_or_else(|| "--".to_string());
    let modified_display = metadata
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| format_unix_timestamp(duration.as_secs()))
        .unwrap_or_else(|| "--".to_string());
    let type_display = format_file_type(&file_info_from_path(path, false));
    (size_display, type_display, modified_display)
}

fn path_modified_display(path: &Path) -> String {
    fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| format_unix_timestamp(duration.as_secs()))
        .unwrap_or_else(|| "--".to_string())
}

fn file_info_from_path(path: &Path, is_directory: bool) -> FileInfo {
    let mut info = FileInfo::new();
    if let Some(name) = path.file_name().and_then(|segment| segment.to_str()) {
        info.set_name(name);
    }
    info.set_file_type(if is_directory {
        FileType::Directory
    } else {
        FileType::Regular
    });
    if !is_directory {
        if let Some(extension) = path.extension().and_then(|segment| segment.to_str()) {
            let content_type = match extension.to_ascii_lowercase().as_str() {
                "txt" => "text/plain",
                "pdf" => "application/pdf",
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "zip" => "application/zip",
                "json" => "application/json",
                "html" | "htm" => "text/html",
                "rs" => "text/plain",
                _ => "application/octet-stream",
            };
            info.set_content_type(content_type);
        }
    }
    info
}

pub(crate) fn format_file_type(file_info: &FileInfo) -> String {
    if file_info.get_file_type() == FileType::Directory {
        return "File folder".to_string();
    }

    let Some(content_type) = file_info.get_content_type() else {
        return "File".to_string();
    };

    if content_type.is_empty() {
        return "File".to_string();
    }

    let subtype = content_type.split('/').nth(1).unwrap_or(&content_type);
    match subtype {
        "octet-stream" => "File".to_string(),
        "directory" => "File folder".to_string(),
        _ => {
            let words: Vec<String> = subtype
                .split(&['-', '.', '_'][..])
                .filter(|segment| !segment.is_empty())
                .map(|segment| {
                    let mut chars = segment.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect();
            if words.is_empty() {
                content_type.to_string()
            } else {
                words.join(" ")
            }
        }
    }
}

pub(crate) fn format_modified(file_info: &FileInfo) -> String {
    let timestamp = match file_info.get_attribute("time::modified") {
        Some(npio::FileAttributeType::Uint64(value)) => *value,
        Some(npio::FileAttributeType::Int64(value)) => *value as u64,
        _ => return "--".to_string(),
    };

    if timestamp == 0 {
        return "--".to_string();
    }

    format_unix_timestamp(timestamp)
}

pub(crate) fn format_unix_timestamp(secs: u64) -> String {
    let days = secs / 86_400;
    let remainder = secs % 86_400;
    let hour = remainder / 3600;
    let minute = (remainder % 3600) / 60;

    let mut year = 1970i32;
    let mut day_count = days as i32;
    while day_count >= days_in_year(year) {
        day_count -= days_in_year(year);
        year += 1;
    }

    let mut month = 1i32;
    while day_count >= days_in_month(year, month) {
        day_count -= days_in_month(year, month);
        month += 1;
    }

    format!("{year:04}-{month:02}-{:02} {hour:02}:{minute:02}", day_count + 1)
}

pub(crate) fn days_in_year(year: i32) -> i32 {
    if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
        366
    } else {
        365
    }
}

pub(crate) fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if days_in_year(year) == 366 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

pub(crate) fn format_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
