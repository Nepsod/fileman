use std::path::{Path, PathBuf};
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

/// Move a file or directory to the desktop trash (freedesktop / platform trash).
pub fn move_to_trash(path: PathBuf) -> Result<(), String> {
    trash::delete(&path).map_err(|e| format!("Failed to move to trash: {}", e))
}

/// Delete a file or directory permanently
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

/// Recursively copy a directory or file asynchronously
pub fn copy_recursive(
    from: PathBuf,
    to: PathBuf,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send>> {
    Box::pin(async move {
        if !from.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Source path does not exist",
            ));
        }

        if from.is_dir() {
            tokio::fs::create_dir_all(&to).await?;
            let mut entries = tokio::fs::read_dir(&from).await?;
            
            while let Some(entry) = entries.next_entry().await? {
                let entry_path = entry.path();
                let file_name = entry.file_name();
                let dest_path = to.join(file_name);
                
                copy_recursive(entry_path, dest_path).await?;
            }
        } else {
            tokio::fs::copy(&from, &to).await?;
        }
        
        Ok(())
    })
}

/// Unused sibling path for duplicate: `name copy`, `name copy (2)`, … (files use stem/`ext` split).
pub fn duplicate_destination_in_parent(path: &Path) -> Result<PathBuf, String> {
    let meta = fs::metadata(path).map_err(|e| format!("{}", e))?;
    let parent = path
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "Path has no parent".to_string())?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid file name".to_string())?;

    if meta.is_dir() {
        for n in 0u32..10_000 {
            let dest_name = if n == 0 {
                format!("{} copy", name)
            } else {
                format!("{} copy ({})", name, n)
            };
            let dest = parent.join(&dest_name);
            if !dest.exists() {
                return Ok(dest);
            }
        }
        return Err("Could not find an unused duplicate name".to_string());
    }

    let (stem, ext) = match name.rfind('.') {
        Some(0) => (name, ""),
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    };
    for n in 0u32..10_000 {
        let dest_name = if n == 0 {
            format!("{} copy{}", stem, ext)
        } else {
            format!("{} copy ({}){}", stem, n, ext)
        };
        let dest = parent.join(&dest_name);
        if !dest.exists() {
            return Ok(dest);
        }
    }
    Err("Could not find an unused duplicate name".to_string())
}

/// Duplicate a file synchronously (same directory).
pub fn duplicate_in_parent(path: PathBuf) -> Result<PathBuf, String> {
    if path.is_dir() {
        return Err("Folder duplicate runs asynchronously".to_string());
    }
    let dest = duplicate_destination_in_parent(&path)?;
    copy_file(path, dest.clone())?;
    Ok(dest)
}

/// Recursively copy directory tree to a new path (async; caller picks dest via [`duplicate_destination_in_parent`]).
pub async fn duplicate_directory_tree(from: PathBuf, to: PathBuf) -> Result<(), String> {
    copy_recursive(from, to).await.map_err(|e| e.to_string())
}
