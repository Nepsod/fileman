//! Async paste batches with progress hooks, optional cancel, and conflict policy.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

use tokio::sync::mpsc;

use super::{copy_recursive, delete_path};
use super::undo::UndoStack;

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate a new job id for status / progress messages.
pub fn next_job_id() -> u64 {
    NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed)
}

/// Coarse progress for a multi-file job.
#[derive(Debug, Clone)]
pub struct JobProgress {
    pub job_id: u64,
    pub files_done: u32,
    pub files_total: u32,
    pub current_path: PathBuf,
}

/// How to handle an existing file at the destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Leave the destination in place; skip this source.
    Skip,
    /// Remove the destination, then copy/move.
    Overwrite,
    /// Pick a new non-colliding name in the destination folder.
    KeepBoth,
}

#[derive(Clone, Copy, Debug)]
pub struct PasteJobSettings {
    pub conflict: ConflictResolution,
}

fn unique_dest_avoiding_collision(dest: PathBuf) -> PathBuf {
    if !dest.exists() {
        return dest;
    }
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    if dest.is_dir() {
        for n in 0u32..10_000 {
            let new_name = if n == 0 {
                format!("{}_copy", name)
            } else {
                format!("{}_copy ({})", name, n)
            };
            let candidate = parent.join(&new_name);
            if !candidate.exists() {
                return candidate;
            }
        }
    } else {
        let stem = dest.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
        let ext = dest.extension().and_then(|s| s.to_str());
        for n in 0u32..10_000 {
            let new_name = match (n, ext) {
                (0, Some(e)) => format!("{}_copy.{}", stem, e),
                (0, None) => format!("{}_copy", stem),
                (i, Some(e)) => format!("{}_copy ({}).{}", stem, i, e),
                (i, None) => format!("{}_copy ({})", stem, i),
            };
            let candidate = parent.join(&new_name);
            if !candidate.exists() {
                return candidate;
            }
        }
    }
    dest
}

async fn transfer_one(
    from: &Path,
    to: &Path,
    is_cut: bool,
) -> Result<(), std::io::Error> {
    if is_cut {
        match tokio::fs::rename(from, to).await {
            Ok(()) => Ok(()),
            Err(e) if e.raw_os_error() == Some(18) => {
                if from.is_dir() {
                    copy_recursive(from.to_path_buf(), to.to_path_buf()).await?;
                } else {
                    tokio::fs::copy(from, to).await?;
                }
                delete_path(from.to_path_buf()).map_err(|msg| {
                    std::io::Error::new(std::io::ErrorKind::Other, msg)
                })?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    } else if from.is_dir() {
        copy_recursive(from.to_path_buf(), to.to_path_buf()).await
    } else {
        tokio::fs::copy(from, to).await.map(|_| ())
    }
}

/// Copy or move each `sources` entry into `dest_dir`, honoring collision policy.
pub async fn run_paste_batch(
    job_id: u64,
    sources: Vec<PathBuf>,
    dest_dir: PathBuf,
    is_cut: bool,
    settings: PasteJobSettings,
    cancel: Arc<AtomicBool>,
    progress_tx: Option<mpsc::UnboundedSender<JobProgress>>,
    undo_sink: Option<Arc<Mutex<UndoStack>>>,
) -> Result<(), String> {
    let files_total = sources.len() as u32;
    let mut files_done = 0u32;

    for from_path in sources {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".to_string());
        }

        let Some(name) = from_path.file_name() else {
            continue;
        };
        let mut target = dest_dir.join(name);

        if target.exists() {
            match settings.conflict {
                ConflictResolution::Skip => {
                    files_done += 1;
                    continue;
                }
                ConflictResolution::Overwrite => {
                    if target.is_dir() {
                        tokio::fs::remove_dir_all(&target)
                            .await
                            .map_err(|e| e.to_string())?;
                    } else {
                        tokio::fs::remove_file(&target)
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                }
                ConflictResolution::KeepBoth => {
                    target = unique_dest_avoiding_collision(target);
                }
            }
        }

        if let Some(tx) = &progress_tx {
            let _ = tx.send(JobProgress {
                job_id,
                files_done,
                files_total,
                current_path: from_path.clone(),
            });
        }

        let src_before = from_path.clone();
        let dst_after = target.clone();

        transfer_one(&from_path, &target, is_cut)
            .await
            .map_err(|e| format!("{} ({})", e, from_path.display()))?;

        if is_cut {
            if let Some(stack) = &undo_sink {
                if let Ok(mut g) = stack.lock() {
                    g.push_move(src_before, dst_after);
                }
            }
        }

        files_done += 1;
    }

    Ok(())
}
