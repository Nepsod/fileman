use std::path::PathBuf;
use std::fs;

/// Create a new directory
pub fn create_directory(path: PathBuf) -> Result<(), String> {
    fs::create_dir(&path)
        .map_err(|e| format!("Failed to create directory: {}", e))
}

/// Create a new file
pub fn create_file(path: PathBuf) -> Result<(), String> {
    fs::File::create(&path)
        .map_err(|e| format!("Failed to create file: {}", e))?;
    Ok(())
}

/// Delete a file or directory
pub fn delete_path(path: PathBuf) -> Result<(), String> {
    let metadata = fs::metadata(&path)
        .map_err(|e| format!("Failed to get metadata: {}", e))?;

    if metadata.is_dir() {
        fs::remove_dir_all(&path)
            .map_err(|e| format!("Failed to remove directory: {}", e))
    } else {
        fs::remove_file(&path)
            .map_err(|e| format!("Failed to remove file: {}", e))
    }
}

/// Rename/move a file or directory
pub fn rename_path(from: PathBuf, to: PathBuf) -> Result<(), String> {
    fs::rename(&from, &to)
        .map_err(|e| format!("Failed to rename: {}", e))
}

/// Copy a file
pub fn copy_file(from: PathBuf, to: PathBuf) -> Result<(), String> {
    fs::copy(&from, &to)
        .map_err(|e| format!("Failed to copy file: {}", e))?;
    Ok(())
}
