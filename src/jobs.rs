use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
    cancel: Option<Arc<AtomicBool>>,
) -> PasteResult {
    let mut result = PasteResult::default();

    for source in sources {
        if cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            result.cancelled = true;
            break;
        }

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
        let mut replaced_existing = false;
        if destination.exists() {
            match settings.conflict {
                ConflictResolution::Skip => continue,
                ConflictResolution::Overwrite => {
                    replaced_existing = true;
                    if let Err(error) = crate::operations::remove_path_at(&destination) {
                        result.errors.push(error);
                        continue;
                    }
                }
                ConflictResolution::KeepBoth => {
                    match crate::operations::unique_copy_name_in_parent(&destination) {
                        Ok(unique_destination) => destination = unique_destination,
                        Err(error) => {
                            result.errors.push(error);
                            continue;
                        }
                    }
                }
            }
        }

        let undoable_move = is_cut && !replaced_existing;
        let source_for_undo = source.clone();
        let destination_for_undo = destination.clone();
        let operation_result = if is_cut {
            crate::operations::move_path(source, destination)
        } else {
            crate::operations::copy_path(source, destination)
        };

        match operation_result {
            Ok(()) => {
                if undoable_move {
                    result
                        .recorded_moves
                        .push((source_for_undo, destination_for_undo));
                }
            }
            Err(error) => {
                if replaced_existing {
                    result.errors.push(format!(
                        "{error} (the previous item at the destination may have been removed)"
                    ));
                } else {
                    result.errors.push(error);
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use nptk::std::fs;

    fn test_directory(label: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "fileman_jobs_test_{label}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create jobs test directory");
        directory
    }

    #[test]
    fn count_paste_conflicts_only_counts_existing_destination_names() {
        let destination = test_directory("conflicts");
        let existing = destination.join("exists.txt");
        fs::write(&existing, b"a").unwrap();
        let missing_name = destination.join("missing.txt");
        let outside = test_directory("outside");
        let outside_file = outside.join("outside.txt");
        fs::write(&outside_file, b"b").unwrap();

        assert_eq!(
            count_paste_conflicts(&[existing.clone(), outside_file.clone()], &destination),
            1
        );
        assert_eq!(
            count_paste_conflicts(&[missing_name, outside_file], &destination),
            0
        );

        let _ = fs::remove_dir_all(&destination);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn run_paste_batch_skips_conflicting_sources() {
        let destination = test_directory("skip");
        let source_directory = test_directory("sources");
        let source = source_directory.join("report.txt");
        fs::write(&source, b"source").unwrap();
        fs::write(destination.join("report.txt"), b"dest").unwrap();

        let result = run_paste_batch(
            vec![source.clone()],
            destination.clone(),
            false,
            PasteJobSettings {
                conflict: ConflictResolution::Skip,
            },
            None,
        );

        assert!(result.errors.is_empty());
        assert!(result.recorded_moves.is_empty());
        assert_eq!(fs::read_to_string(destination.join("report.txt")).unwrap(), "dest");
        assert!(source.exists());

        let _ = fs::remove_dir_all(&destination);
        let _ = fs::remove_dir_all(&source_directory);
    }
}
