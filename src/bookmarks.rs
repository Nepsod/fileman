use nptk::std::fs;
use nptk::std::path::{Path, PathBuf};

pub fn gtk_bookmarks_path() -> Option<PathBuf> {
    if let Some(config_directory) = dirs::config_dir() {
        let gtk_path = config_directory.join("gtk-3.0").join("bookmarks");
        if gtk_path.is_file() {
            return Some(gtk_path);
        }
    }

    dirs::home_dir().map(|home| home.join(".gtk-3.0").join("bookmarks"))
}

pub fn load_bookmarks() -> Vec<PathBuf> {
    let Some(bookmarks_path) = gtk_bookmarks_path() else {
        return Vec::new();
    };

    if !bookmarks_path.is_file() {
        return Vec::new();
    }

    let Ok(content) = fs::read_to_string(&bookmarks_path) else {
        return Vec::new();
    };

    content
        .lines()
        .filter_map(parse_bookmark_line)
        .collect()
}

fn parse_bookmark_line(line: &str) -> Option<PathBuf> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(path) = trimmed.strip_prefix("file://") {
        return Some(PathBuf::from(path));
    }

    Some(PathBuf::from(trimmed))
}

pub fn add_bookmark(path: &Path) -> Result<(), String> {
    let bookmarks_path = gtk_bookmarks_path().ok_or_else(|| "Bookmarks file not found".to_string())?;

    if let Some(parent) = bookmarks_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create bookmarks directory: {error}"))?;
    }

    let uri = format!("file://{}", path.display());
    let existing = fs::read_to_string(&bookmarks_path).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == uri) {
        return Ok(());
    }

    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&uri);
    content.push('\n');

    fs::write(&bookmarks_path, content)
        .map_err(|error| format!("Failed to write bookmarks: {error}"))
}

pub fn remove_bookmark(path: &Path) -> Result<(), String> {
    let bookmarks_path = gtk_bookmarks_path().ok_or_else(|| "Bookmarks file not found".to_string())?;
    if !bookmarks_path.is_file() {
        return Ok(());
    }

    let uri = format!("file://{}", path.display());
    let content = fs::read_to_string(&bookmarks_path)
        .map_err(|error| format!("Failed to read bookmarks: {error}"))?;

    let filtered: Vec<&str> = content
        .lines()
        .filter(|line| line.trim() != uri)
        .collect();

    let new_content = if filtered.is_empty() {
        String::new()
    } else {
        format!("{}\n", filtered.join("\n"))
    };

    fs::write(&bookmarks_path, new_content)
        .map_err(|error| format!("Failed to write bookmarks: {error}"))
}

pub fn is_bookmarked(path: &Path, bookmarks: &[PathBuf]) -> bool {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    bookmarks.iter().any(|bookmark| {
        bookmark.canonicalize().unwrap_or_else(|_| bookmark.clone()) == canonical
    })
}
