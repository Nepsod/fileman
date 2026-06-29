use nptk::std::ffi::OsString;
use nptk::std::fs;
use nptk::std::os::unix::ffi::OsStringExt;
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

fn canonical_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn percent_decode_path(encoded: &str) -> OsString {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""), 16)
            {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    OsString::from_vec(decoded)
}

fn decode_file_uri_path(uri: &str) -> Option<PathBuf> {
    let path_portion = uri.strip_prefix("localhost").unwrap_or(uri);
    if path_portion.is_empty() {
        return None;
    }
    Some(PathBuf::from(percent_decode_path(path_portion)))
}

fn split_uri_and_label(uri_part: &str) -> (&str, Option<&str>) {
    if let Some(space_index) = uri_part.find(' ') {
        (
            uri_part.get(..space_index).unwrap_or(uri_part),
            uri_part.get(space_index + 1..).filter(|label| !label.is_empty()),
        )
    } else {
        (uri_part, None)
    }
}

fn parse_bookmark_line(line: &str) -> Option<PathBuf> {
    parse_bookmark_entry(line)
}

fn parse_bookmark_entry(line: &str) -> Option<PathBuf> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(uri_part) = trimmed.strip_prefix("file://") {
        let (uri, _) = split_uri_and_label(uri_part);
        return decode_file_uri_path(uri);
    }

    Some(PathBuf::from(trimmed))
}

pub fn add_bookmark(path: &Path) -> Result<(), String> {
    let bookmarks_path = gtk_bookmarks_path().ok_or_else(|| "Bookmarks file not found".to_string())?;

    if let Some(parent) = bookmarks_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create bookmarks directory: {error}"))?;
    }

    let target = canonical_path(path);
    let existing = fs::read_to_string(&bookmarks_path).unwrap_or_default();
    if existing
        .lines()
        .filter_map(parse_bookmark_line)
        .any(|bookmark_path| canonical_path(&bookmark_path) == target)
    {
        return Ok(());
    }

    let uri = format!("file://{}", path.display());
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

    let target = canonical_path(path);
    let content = fs::read_to_string(&bookmarks_path)
        .map_err(|error| format!("Failed to read bookmarks: {error}"))?;

    let filtered: Vec<&str> = content
        .lines()
        .filter(|line| {
            parse_bookmark_line(line)
                .map(|bookmark_path| canonical_path(&bookmark_path) != target)
                .unwrap_or(true)
        })
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
    let canonical = canonical_path(path);
    bookmarks
        .iter()
        .any(|bookmark| canonical_path(bookmark) == canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bookmark_line_strips_label_suffix() {
        let path = parse_bookmark_line("file:///home/user/Documents Documents").expect("path");
        assert_eq!(path, PathBuf::from("/home/user/Documents"));
    }

    #[test]
    fn parse_bookmark_line_decodes_percent_encoding() {
        let path = parse_bookmark_line("file:///home/user/My%20Docs").expect("path");
        assert_eq!(path, PathBuf::from("/home/user/My Docs"));
    }

    #[test]
    fn parse_bookmark_line_handles_localhost_prefix() {
        let path =
            parse_bookmark_line("file://localhost/home/user/Documents").expect("path");
        assert_eq!(path, PathBuf::from("/home/user/Documents"));
    }
}
