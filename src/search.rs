use nptk::std::fs;
use nptk::std::path::{Path, PathBuf};

use crate::window::table_columns_for_path;

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
    pub size_display: String,
    pub type_display: String,
    pub modified_display: String,
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
            let is_directory = path.is_dir();
            let (size_display, type_display, modified_display) =
                table_columns_for_path(&path, is_directory);
            results.push(SearchMatch {
                path: path.clone(),
                name: name.to_string(),
                parent_label,
                is_directory,
                size_display,
                type_display,
                modified_display,
            });
        }

        if path.is_dir() {
            walk_directory(root, &path, query, show_hidden, depth + 1, results);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "fileman_search_test_{label}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create search test root");
        root
    }

    #[test]
    fn empty_query_returns_no_matches() {
        let root = test_root("empty");
        fs::write(root.join("needle.txt"), b"x").unwrap();
        assert!(find_in_subfolders(&root, "   ", true).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hidden_entries_excluded_when_show_hidden_false() {
        let root = test_root("hidden");
        fs::write(root.join(".secret_needle"), b"x").unwrap();
        fs::write(root.join("visible_needle.txt"), b"x").unwrap();
        let matches = find_in_subfolders(&root, "needle", false);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "visible_needle.txt");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn search_stops_at_max_depth() {
        let root = test_root("depth");
        let mut directory = root.clone();
        for index in 0..=MAX_SEARCH_DEPTH + 2 {
            directory = directory.join(format!("level{index}"));
            fs::create_dir_all(&directory).unwrap();
        }
        fs::write(directory.join("deep_needle.txt"), b"x").unwrap();
        let shallow = root.join("level0").join("shallow_needle.txt");
        fs::write(&shallow, b"x").unwrap();

        let matches = find_in_subfolders(&root, "needle", true);
        let names: Vec<_> = matches.iter().map(|entry| entry.name.as_str()).collect();
        assert!(names.contains(&"shallow_needle.txt"));
        assert!(!names.iter().any(|name| *name == "deep_needle.txt"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn search_stops_at_max_results() {
        let root = test_root("cap");
        for index in 0..MAX_SEARCH_RESULTS + 20 {
            fs::write(
                root.join(format!("match_{index:04}.txt")),
                b"x",
            )
            .unwrap();
        }
        let matches = find_in_subfolders(&root, "match", true);
        assert_eq!(matches.len(), MAX_SEARCH_RESULTS);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn table_columns_for_path_on_regular_file() {
        let root = test_root("columns");
        let path = root.join("sample.txt");
        fs::write(&path, b"hello").unwrap();
        let (size_display, type_display, modified_display) =
            table_columns_for_path(&path, false);
        assert!(!size_display.is_empty());
        assert!(!type_display.is_empty());
        assert!(!modified_display.is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
