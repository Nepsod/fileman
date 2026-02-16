use std::path::PathBuf;
use std::fs;

/// Create a new directory
pub fn create_directory(path: PathBuf) -> Result<(), String> {
    fs::create_dir(&path)
        .map_err(|e| format!("Failed to create directory: {}", e))
}

/// Create a new file
#[allow(dead_code)]
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
#[allow(dead_code)]
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
