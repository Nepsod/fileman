use nptk::file_icons::FileIconPresentation;
use nptk::std::fs;
use nptk::std::path::{Path, PathBuf};

use npio::service::filesystem::mime_detector::MimeDetector;
use npio::service::filesystem::mime_registry::MimeRegistry;

pub const PROPERTIES_ICON_SIZE: u32 = 64;

#[derive(Debug, Clone)]
pub struct PropertyRow {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct PropertiesDialog {
    pub title: String,
    pub rows: Vec<PropertyRow>,
    pub icon: Option<FileIconPresentation>,
}

pub fn properties_for_paths(paths: &[PathBuf]) -> Option<PropertiesDialog> {
    if paths.is_empty() {
        return None;
    }

    if paths.len() == 1 {
        return Some(single_path_properties(&paths[0]));
    }

    Some(multi_path_properties(paths))
}

fn single_path_properties(path: &Path) -> PropertiesDialog {
    let name = path
        .file_name()
        .and_then(|segment| segment.to_str())
        .unwrap_or("<unnamed>")
        .to_string();

    let mut rows = vec![
        PropertyRow {
            label: "Name".to_string(),
            value: name.clone(),
        },
        PropertyRow {
            label: "Location".to_string(),
            value: path
                .parent()
                .map(|parent| parent.display().to_string())
                .unwrap_or_default(),
        },
    ];

    if let Ok(metadata) = fs::metadata(path) {
        let size = if metadata.is_dir() {
            directory_size(path)
        } else {
            metadata.len()
        };
        rows.push(PropertyRow {
            label: "Size".to_string(),
            value: format!("{} ({size} bytes)", format_size(size)),
        });
        if let Ok(modified) = metadata.modified() {
            rows.push(PropertyRow {
                label: "Modified".to_string(),
                value: format_system_time(modified),
            });
        }
        if let Ok(created) = metadata.created() {
            rows.push(PropertyRow {
                label: "Created".to_string(),
                value: format_system_time(created),
            });
        }
        rows.push(PropertyRow {
            label: "Type".to_string(),
            value: if metadata.is_dir() {
                "Folder".to_string()
            } else if metadata.is_symlink() {
                "Symbolic link".to_string()
            } else {
                "File".to_string()
            },
        });
    }

    PropertiesDialog {
        title: format!("Properties — {name}"),
        rows,
        icon: None,
    }
}

pub async fn mime_kind_row(path: &Path) -> Option<PropertyRow> {
    let mime = MimeDetector::detect_mime_type(path).await?;
    let registry = MimeRegistry::load_default();
    let description = crate::open::mime_variants(&mime)
        .into_iter()
        .find_map(|variant| registry.description(&variant));
    let value = if let Some(description) = description {
        format!("{description} ({mime})")
    } else {
        mime
    };
    Some(PropertyRow {
        label: "Kind".to_string(),
        value,
    })
}

pub fn insert_kind_row(dialog: &mut PropertiesDialog, kind_row: PropertyRow) {
    if dialog.rows.iter().any(|row| row.label == "Kind") {
        return;
    }
    let insert_at = dialog
        .rows
        .iter()
        .position(|row| row.label == "Name")
        .map(|index| index + 1)
        .unwrap_or(0);
    dialog.rows.insert(insert_at, kind_row);
}

fn multi_path_properties(paths: &[PathBuf]) -> PropertiesDialog {
    let mut file_count = 0usize;
    let mut directory_count = 0usize;
    let mut total_size = 0u64;

    for path in paths {
        let Ok(metadata) = fs::metadata(path) else {
            continue;
        };
        if metadata.is_dir() {
            directory_count += 1;
            total_size = total_size.saturating_add(directory_size(path));
        } else {
            file_count += 1;
            total_size = total_size.saturating_add(metadata.len());
        }
    }

    let rows = vec![
        PropertyRow {
            label: "Items".to_string(),
            value: paths.len().to_string(),
        },
        PropertyRow {
            label: "Files".to_string(),
            value: file_count.to_string(),
        },
        PropertyRow {
            label: "Folders".to_string(),
            value: directory_count.to_string(),
        },
        PropertyRow {
            label: "Total size".to_string(),
            value: format!("{} ({total_size} bytes)", format_size(total_size)),
        },
    ];

    PropertiesDialog {
        title: format!("Properties — {} items", paths.len()),
        rows,
        icon: None,
    }
}

fn directory_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(path) else {
        return total;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            total = total.saturating_add(directory_size(&entry_path));
        } else {
            total = total.saturating_add(metadata.len());
        }
    }
    total
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn format_system_time(time: std::time::SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    let Ok(duration) = time.duration_since(UNIX_EPOCH) else {
        return "--".to_string();
    };
    let seconds = duration.as_secs();
    let days = seconds / 86_400;
    let remainder = seconds % 86_400;
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

    format!(
        "{year:04}-{month:02}-{:02} {hour:02}:{minute:02}",
        day_count + 1
    )
}

fn days_in_year(year: i32) -> i32 {
    if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
        366
    } else {
        365
    }
}

fn days_in_month(year: i32, month: i32) -> i32 {
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
