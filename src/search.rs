use nptk::std::fs;
use nptk::std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchScope {
    CurrentFolder,
    Subfolders,
}

#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub name: String,
    pub parent_label: String,
    pub is_directory: bool,
}

const MAX_SEARCH_DEPTH: usize = 12;
const MAX_SEARCH_RESULTS: usize = 500;

pub fn find_in_subfolders(
    root: &Path,
    query: &str,
    show_hidden: bool,
) -> Vec<SearchMatch> {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    walk_directory(
        root,
        root,
        &query,
        show_hidden,
        0,
        &mut results,
    );
    results
}

fn walk_directory(
    root: &Path,
    directory: &Path,
    query: &str,
    show_hidden: bool,
    depth: usize,
    results: &mut Vec<SearchMatch>,
) {
    if results.len() >= MAX_SEARCH_RESULTS || depth > MAX_SEARCH_DEPTH {
        return;
    }

    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        if results.len() >= MAX_SEARCH_RESULTS {
            break;
        }

        let path = entry.path();
        let Some(name) = path.file_name().and_then(|segment| segment.to_str()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        if !show_hidden && name.starts_with('.') {
            continue;
        }

        if name.to_ascii_lowercase().contains(query) {
            let parent_label = path
                .parent()
                .and_then(|parent| parent.strip_prefix(root).ok())
                .map(|relative| {
                    if relative.as_os_str().is_empty() {
                        ".".to_string()
                    } else {
                        relative.display().to_string()
                    }
                })
                .unwrap_or_else(|| ".".to_string());
            results.push(SearchMatch {
                path: path.clone(),
                name: name.to_string(),
                parent_label,
                is_directory: path.is_dir(),
            });
        }

        if path.is_dir() {
            walk_directory(root, &path, query, show_hidden, depth + 1, results);
        }
    }
}
