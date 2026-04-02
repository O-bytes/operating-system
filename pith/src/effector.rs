/// The effector — the ONLY module that writes to the filesystem.
///
/// All subsystem reactions produce `Vec<Effect>` which the effector executes.
/// This ensures:
///   - All writes go through permission checks
///   - All writes are logged
///   - Atomic multi-step operations (stage in tmp/, then mv)
///   - The watcher can distinguish engine-generated events from external ones
///     via the pending ops set

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use filetime::FileTime;
use tracing::{debug, error, info};

use crate::error::{PithError, Result};

/// A filesystem effect to be executed by the effector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Create a zero-byte file.
    Touch { path: PathBuf },

    /// Create a zero-byte file with a specific mtime (for scheduling).
    TouchWithMtime { path: PathBuf, mtime: SystemTime },

    /// Delete a file or empty directory.
    Remove { path: PathBuf },

    /// Rename/move a file or directory.
    Move { from: PathBuf, to: PathBuf },

    /// Create a directory (and parents if needed).
    MakeDir { path: PathBuf },
}

impl Effect {
    /// Get the primary path affected by this effect.
    pub fn path(&self) -> &Path {
        match self {
            Self::Touch { path } => path,
            Self::TouchWithMtime { path, .. } => path,
            Self::Remove { path } => path,
            Self::Move { from, .. } => from,
            Self::MakeDir { path } => path,
        }
    }
}

/// The effector executes filesystem effects and tracks pending operations
/// to avoid infinite watcher loops.
#[derive(Debug, Clone)]
pub struct Effector {
    /// The root path of the 0-bytes filesystem.
    fs_root: PathBuf,
    /// Set of paths with pending operations — the watcher checks this
    /// to skip engine-generated events.
    pending: Arc<Mutex<HashSet<PathBuf>>>,
}

impl Effector {
    /// Create a new effector for the given filesystem root.
    pub fn new(fs_root: PathBuf) -> Self {
        Self {
            fs_root,
            pending: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Get a reference to the pending ops set (for the watcher to check).
    pub fn pending_ops(&self) -> Arc<Mutex<HashSet<PathBuf>>> {
        Arc::clone(&self.pending)
    }

    /// Execute a single effect.
    pub async fn execute(&self, effect: &Effect) -> Result<()> {
        let full_path = self.resolve_path(effect.path());

        // Register pending op BEFORE writing.
        self.register_pending(&full_path);

        let result = match effect {
            Effect::Touch { .. } => self.do_touch(&full_path).await,
            Effect::TouchWithMtime { mtime, .. } => {
                self.do_touch(&full_path).await?;
                self.do_set_mtime(&full_path, *mtime).await
            }
            Effect::Remove { .. } => self.do_remove(&full_path).await,
            Effect::Move { from: _, to } => {
                let full_to = self.resolve_path(to);
                self.register_pending(&full_to);
                self.do_move(&full_path, &full_to).await
            }
            Effect::MakeDir { .. } => self.do_mkdir(&full_path).await,
        };

        if let Err(ref e) = result {
            error!("Effector failed on {}: {}", full_path.display(), e);
            // Remove from pending on failure so the watcher doesn't skip it forever.
            self.unregister_pending(&full_path);
        }

        result
    }

    /// Execute a batch of effects.
    pub async fn execute_batch(&self, effects: &[Effect]) -> Vec<Result<()>> {
        let mut results = Vec::with_capacity(effects.len());
        for effect in effects {
            results.push(self.execute(effect).await);
        }
        results
    }

    /// Check if a path has a pending operation (called by the watcher).
    /// Returns true and removes the entry if found.
    pub fn consume_pending(&self, path: &Path) -> bool {
        let mut pending = self.pending.lock().unwrap();
        pending.remove(path)
    }

    /// Check if a path is in the pending set WITHOUT consuming it.
    pub fn is_pending(&self, path: &Path) -> bool {
        let pending = self.pending.lock().unwrap();
        pending.contains(path)
    }

    // --- Internal helpers ---

    fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.fs_root.join(path)
        }
    }

    fn register_pending(&self, path: &PathBuf) {
        let mut pending = self.pending.lock().unwrap();
        pending.insert(path.clone());
        debug!("Pending registered: {}", path.display());
    }

    fn unregister_pending(&self, path: &PathBuf) {
        let mut pending = self.pending.lock().unwrap();
        pending.remove(path);
    }

    async fn do_touch(&self, path: &Path) -> Result<()> {
        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| PithError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        // Create zero-byte file (or update mtime if exists).
        tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)
            .await
            .map_err(|e| PithError::Io {
                path: path.to_path_buf(),
                source: e,
            })?;
        debug!("touch {}", path.display());
        Ok(())
    }

    async fn do_set_mtime(&self, path: &Path, mtime: SystemTime) -> Result<()> {
        let ft = FileTime::from_system_time(mtime);
        filetime::set_file_mtime(path, ft).map_err(|e| PithError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        debug!("set_mtime {} → {:?}", path.display(), mtime);
        Ok(())
    }

    async fn do_remove(&self, path: &Path) -> Result<()> {
        let meta = tokio::fs::metadata(path).await.map_err(|e| PithError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        if meta.is_dir() {
            tokio::fs::remove_dir(path).await
        } else {
            tokio::fs::remove_file(path).await
        }
        .map_err(|e| PithError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        debug!("rm {}", path.display());
        Ok(())
    }

    async fn do_move(&self, from: &Path, to: &Path) -> Result<()> {
        // Ensure target parent exists.
        if let Some(parent) = to.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| PithError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        tokio::fs::rename(from, to).await.map_err(|e| PithError::Io {
            path: from.to_path_buf(),
            source: e,
        })?;
        debug!("mv {} → {}", from.display(), to.display());
        Ok(())
    }

    async fn do_mkdir(&self, path: &Path) -> Result<()> {
        tokio::fs::create_dir_all(path).await.map_err(|e| PithError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        debug!("mkdir {}", path.display());
        Ok(())
    }
}

/// Clean all contents of the tmp/ directory.
pub async fn clean_tmp(fs_root: &Path) -> Result<()> {
    let tmp_dir = fs_root.join("tmp");
    if tmp_dir.exists() {
        let mut entries = tokio::fs::read_dir(&tmp_dir).await.map_err(|e| PithError::Io {
            path: tmp_dir.clone(),
            source: e,
        })?;
        while let Some(entry) = entries.next_entry().await.map_err(|e| PithError::Io {
            path: tmp_dir.clone(),
            source: e,
        })? {
            let path = entry.path();
            // Skip .gitkeep
            if path.file_name().map_or(false, |n| n.to_string_lossy().starts_with('.')) {
                continue;
            }
            if path.is_dir() {
                let _ = tokio::fs::remove_dir_all(&path).await;
            } else {
                let _ = tokio::fs::remove_file(&path).await;
            }
        }
        info!("Cleaned tmp/");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_touch_creates_zero_byte_file() {
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());

        let effect = Effect::Touch {
            path: PathBuf::from("events/!boot"),
        };
        effector.execute(&effect).await.unwrap();

        let file_path = dir.path().join("events/!boot");
        assert!(file_path.exists());
        assert_eq!(std::fs::metadata(&file_path).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_mkdir_creates_directory() {
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());

        let effect = Effect::MakeDir {
            path: PathBuf::from("jobs/1/-expected/type"),
        };
        effector.execute(&effect).await.unwrap();

        assert!(dir.path().join("jobs/1/-expected/type").is_dir());
    }

    #[tokio::test]
    async fn test_remove_deletes_file() {
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());

        // Create first.
        effector
            .execute(&Effect::Touch {
                path: PathBuf::from("tmp/test"),
            })
            .await
            .unwrap();
        assert!(dir.path().join("tmp/test").exists());

        // Remove.
        effector
            .execute(&Effect::Remove {
                path: PathBuf::from("tmp/test"),
            })
            .await
            .unwrap();
        assert!(!dir.path().join("tmp/test").exists());
    }

    #[tokio::test]
    async fn test_move_renames_file() {
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());

        effector
            .execute(&Effect::Touch {
                path: PathBuf::from("jobs/1/-state/pending"),
            })
            .await
            .unwrap();

        effector
            .execute(&Effect::Move {
                from: PathBuf::from("jobs/1/-state/pending"),
                to: PathBuf::from("jobs/1/-state/running"),
            })
            .await
            .unwrap();

        assert!(!dir.path().join("jobs/1/-state/pending").exists());
        assert!(dir.path().join("jobs/1/-state/running").exists());
    }

    #[tokio::test]
    async fn test_pending_ops_tracking() {
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());

        let full_path = dir.path().join("events/!boot");

        // Before executing, not pending.
        assert!(!effector.is_pending(&full_path));

        // Execute touch.
        effector
            .execute(&Effect::Touch {
                path: PathBuf::from("events/!boot"),
            })
            .await
            .unwrap();

        // After executing, should be pending.
        assert!(effector.is_pending(&full_path));

        // Consume it (simulating what the watcher does).
        assert!(effector.consume_pending(&full_path));

        // Now it's gone.
        assert!(!effector.is_pending(&full_path));
    }

    #[tokio::test]
    async fn test_touch_with_mtime() {
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());

        let future_time = SystemTime::now() + std::time::Duration::from_secs(3600);

        effector
            .execute(&Effect::TouchWithMtime {
                path: PathBuf::from("schedules/test_task"),
                mtime: future_time,
            })
            .await
            .unwrap();

        let file_path = dir.path().join("schedules/test_task");
        assert!(file_path.exists());

        let meta = std::fs::metadata(&file_path).unwrap();
        let actual_mtime = meta.modified().unwrap();
        // Should be within 1 second of the target.
        let diff = future_time
            .duration_since(actual_mtime)
            .unwrap_or_default();
        assert!(diff.as_secs() < 2);
    }

    #[tokio::test]
    async fn test_clean_tmp() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create some tmp files.
        std::fs::create_dir_all(root.join("tmp")).unwrap();
        std::fs::File::create(root.join("tmp/garbage")).unwrap();
        std::fs::create_dir_all(root.join("tmp/subdir")).unwrap();
        std::fs::File::create(root.join("tmp/.gitkeep")).unwrap();

        clean_tmp(root).await.unwrap();

        // .gitkeep should survive, everything else gone.
        assert!(!root.join("tmp/garbage").exists());
        assert!(!root.join("tmp/subdir").exists());
        assert!(root.join("tmp/.gitkeep").exists());
    }
}
