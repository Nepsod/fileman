use std::fs;
use std::path::{Path, PathBuf};

pub fn create_directory(path: PathBuf) -> Result<(), String> {
    fs::create_dir(&path).map_err(|error| format!("Failed to create directory: {error}"))
}

pub fn create_file(path: PathBuf) -> Result<(), String> {
    fs::File::create(&path).map_err(|error| format!("Failed to create file: {error}"))?;
    Ok(())
}

pub fn move_to_trash(path: PathBuf) -> Result<(), String> {
    trash::delete(&path).map_err(|error| format!("Failed to move to trash: {error}"))
}

pub fn delete_path(path: PathBuf) -> Result<(), String> {
    let metadata = fs::metadata(&path).map_err(|error| format!("Failed to read metadata: {error}"))?;

    if metadata.is_dir() {
        fs::remove_dir_all(&path).map_err(|error| format!("Failed to remove directory: {error}"))
    } else {
        fs::remove_file(&path).map_err(|error| format!("Failed to remove file: {error}"))
    }
}

pub fn rename_path(from: PathBuf, to: PathBuf) -> Result<(), String> {
    fs::rename(&from, &to).map_err(|error| format!("Failed to rename: {error}"))
}

pub fn unique_name_in_parent(parent: &Path, base_name: &str) -> PathBuf {
    let mut candidate = parent.join(base_name);
    if !candidate.exists() {
        return candidate;
    }

    for index in 2..10_000 {
        let name = format!("{base_name} ({index})");
        candidate = parent.join(&name);
        if !candidate.exists() {
            return candidate;
        }
    }

    parent.join(format!("{base_name} ({})", std::process::id()))
}

pub fn unique_copy_name_in_parent(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("{error}"))?;
    let parent = path
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "Path has no parent".to_string())?;
    let name = path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .ok_or_else(|| "Invalid file name".to_string())?;

    if metadata.is_dir() {
        return Ok(unique_name_in_parent(&parent, &format!("{name} copy")));
    }

    let (stem, extension) = match name.rfind('.') {
        Some(0) => (name, ""),
        Some(index) => (&name[..index], &name[index..]),
        None => (name, ""),
    };

    let base_name = format!("{stem} copy{extension}");
    Ok(unique_name_in_parent(&parent, &base_name))
}

pub fn copy_path(from: PathBuf, to: PathBuf) -> Result<(), String> {
    let metadata = fs::metadata(&from).map_err(|error| format!("{error}"))?;
    if metadata.is_dir() {
        copy_directory(&from, &to)
    } else {
        fs::copy(&from, &to).map_err(|error| format!("Failed to copy file: {error}"))?;
        Ok(())
    }
}

pub fn paste_files(
    sources: Vec<PathBuf>,
    destination_directory: PathBuf,
    is_cut: bool,
) -> Vec<String> {
    let mut errors = Vec::new();

    for source in sources {
        let file_name = match source.file_name() {
            Some(name) => name.to_owned(),
            None => {
                errors.push(format!("Invalid source path {}", source.display()));
                continue;
            }
        };

        let mut destination = destination_directory.join(&file_name);
        if destination.exists() {
            match unique_copy_name_in_parent(&destination) {
                Ok(unique_destination) => destination = unique_destination,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            }
        }

        let result = if is_cut {
            move_path(source, destination)
        } else {
            copy_path(source, destination)
        };

        if let Err(error) = result {
            errors.push(error);
        }
    }

    errors
}

pub fn move_path(from: PathBuf, to: PathBuf) -> Result<(), String> {
    match fs::rename(&from, &to) {
        Ok(()) => Ok(()),
        Err(rename_error) if rename_error.raw_os_error() == Some(18) => {
            copy_path(from.clone(), to.clone())?;
            delete_path(from)
        }
        Err(rename_error) => Err(format!("Failed to move: {rename_error}")),
    }
}

pub fn duplicate_path(path: PathBuf) -> Result<PathBuf, String> {
    let destination = unique_copy_name_in_parent(&path)?;
    copy_path(path, destination.clone())?;
    Ok(destination)
}

fn copy_directory(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|error| format!("Failed to create directory: {error}"))?;

    for entry in fs::read_dir(from).map_err(|error| format!("Failed to read directory: {error}"))? {
        let entry = entry.map_err(|error| format!("Failed to read directory entry: {error}"))?;
        let source_path = entry.path();
        let destination_path = to.join(entry.file_name());
        copy_path(source_path, destination_path)?;
    }

    Ok(())
}
