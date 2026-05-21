use std::path::{Path, PathBuf};

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
        places.push(("Desktop", home.join("Desktop")));
        places.push(("Documents", home.join("Documents")));
        places.push(("Downloads", home.join("Downloads")));
        places.push(("Music", home.join("Music")));
        places.push(("Pictures", home.join("Pictures")));
        places.push(("Videos", home.join("Videos")));
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
