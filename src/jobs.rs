use nptk::std::path::{Path, PathBuf};

use crate::operations::PasteResult;

pub fn count_paste_conflicts(sources: &[PathBuf], destination_directory: &Path) -> usize {
    sources
        .iter()
        .filter(|source| {
            source
                .file_name()
                .map(|name| destination_directory.join(name).exists())
                .unwrap_or(false)
        })
        .count()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    Skip,
    Overwrite,
    KeepBoth,
}

#[derive(Debug, Clone, Copy)]
pub struct PasteJobSettings {
    pub conflict: ConflictResolution,
}

impl Default for PasteJobSettings {
    fn default() -> Self {
        Self {
            conflict: ConflictResolution::KeepBoth,
        }
    }
}

pub fn run_paste_batch(
    sources: Vec<PathBuf>,
    destination_directory: PathBuf,
    is_cut: bool,
    settings: PasteJobSettings,
) -> PasteResult {
    let mut result = PasteResult::default();

    for source in sources {
        let file_name = match source.file_name() {
            Some(name) => name.to_owned(),
            None => {
                result
                    .errors
                    .push(format!("Invalid source path {}", source.display()));
                continue;
            }
        };

        let mut destination = destination_directory.join(&file_name);
        if destination.exists() {
            match settings.conflict {
                ConflictResolution::Skip => continue,
                ConflictResolution::Overwrite => {
                    if let Err(error) = delete_before_overwrite(&destination) {
                        result.errors.push(error);
                        continue;
                    }
                }
                ConflictResolution::KeepBoth => match crate::operations::unique_copy_name_in_parent(&destination)
                {
                    Ok(unique_destination) => destination = unique_destination,
                    Err(error) => {
                        result.errors.push(error);
                        continue;
                    }
                },
            }
        }

        let source_for_undo = source.clone();
        let destination_for_undo = destination.clone();
        let operation_result = if is_cut {
            crate::operations::move_path(source, destination)
        } else {
            crate::operations::copy_path(source, destination)
        };

        match operation_result {
            Ok(()) => {
                if is_cut {
                    result
                        .recorded_moves
                        .push((source_for_undo, destination_for_undo));
                }
            }
            Err(error) => result.errors.push(error),
        }
    }

    result
}

fn delete_before_overwrite(path: &nptk::std::path::Path) -> Result<(), String> {
    let metadata = nptk::std::fs::metadata(path)
        .map_err(|error| format!("Failed to read metadata: {error}"))?;
    if metadata.is_dir() {
        nptk::std::fs::remove_dir_all(path)
            .map_err(|error| format!("Failed to remove directory: {error}"))
    } else {
        nptk::std::fs::remove_file(path)
            .map_err(|error| format!("Failed to remove file: {error}"))
    }
}
