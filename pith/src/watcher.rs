/// Filesystem watcher — observes the 0-bytes filesystem and bridges events to the engine.
///
/// Uses the `notify` crate (kqueue on macOS, inotify on Linux).
///
/// Watch strategy:
///   - WATCH recursively: states/, jobs/, workers/, channels/, events/,
///     programs/, schedules/, sessions/, subscriptions/, logs/, tmp/
///   - LOAD-ONCE at boot: hard/ (watch non-recursively for unauthorized changes)
///   - NEVER WATCH: pointers/unicodes/ (65k dirs, ROM)
///   - LOAD-ONCE: databases/ (git submodule)

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{Event, EventKind, RecursiveMode, Watcher as NotifyWatcher};
use tracing::{debug, error, info, trace, warn};

use crate::effector::Effector;
use crate::subsystems::FsEventKind;

/// Scopes to watch recursively at runtime.
const WATCHED_SCOPES: &[&str] = &[
    "states",
    "jobs",
    "workers",
    "channels",
    "events",
    "programs",
    "schedules",
    "sessions",
    "subscriptions",
    "logs",
    "tmp",
];

/// A processed filesystem event ready for dispatch.
#[derive(Debug, Clone)]
pub struct WatchEvent {
    /// The kind of change.
    pub kind: FsEventKind,
    /// The relative path from fs_root.
    pub relative_path: PathBuf,
    /// The absolute path.
    pub absolute_path: PathBuf,
}

/// The filesystem watcher — wraps `notify` and filters events.
pub struct FsWatcher {
    /// The underlying notify watcher (kept alive).
    _watcher: notify::RecommendedWatcher,
    /// Channel receiver for raw events.
    rx: mpsc::Receiver<notify::Result<Event>>,
    /// Filesystem root for computing relative paths.
    fs_root: PathBuf,
    /// Reference to the effector's pending ops (for loop avoidance).
    effector: Effector,
}

impl FsWatcher {
    /// Start watching the filesystem.
    ///
    /// Sets up recursive watches on active scopes, non-recursive on hard/.
    pub fn start(fs_root: &Path, effector: Effector) -> crate::error::Result<Self> {
        let (tx, rx) = mpsc::channel();

        let mut watcher = notify::recommended_watcher(tx).map_err(|e| {
            crate::error::PithError::Watcher {
                reason: format!("Failed to create watcher: {}", e),
            }
        })?;

        // Watch active scopes recursively.
        for scope in WATCHED_SCOPES {
            let path = fs_root.join(scope);
            if path.exists() {
                if let Err(e) = watcher.watch(&path, RecursiveMode::Recursive) {
                    warn!("Cannot watch {}: {} — skipping", path.display(), e);
                } else {
                    debug!("Watching recursively: {}", path.display());
                }
            }
        }

        // Watch hard/ non-recursively (detect unauthorized top-level modifications).
        let hard_path = fs_root.join("hard");
        if hard_path.exists() {
            if let Err(e) = watcher.watch(&hard_path, RecursiveMode::NonRecursive) {
                warn!("Cannot watch hard/: {}", e);
            } else {
                debug!("Watching non-recursively: hard/");
            }
        }

        info!(
            "Watcher started: {} scopes watched from {}",
            WATCHED_SCOPES.len(),
            fs_root.display()
        );

        Ok(Self {
            _watcher: watcher,
            rx,
            fs_root: fs_root.to_path_buf(),
            effector,
        })
    }

    /// Poll for the next event, with a timeout.
    ///
    /// Returns `None` if no event is available within the timeout.
    /// Filters out:
    ///   - Events for dotfiles (.gitkeep, .DS_Store)
    ///   - Events from the effector's pending ops (loop avoidance)
    ///   - Events we don't care about (Access, Other)
    pub fn poll(&self, timeout: Duration) -> Option<WatchEvent> {
        loop {
            let raw_event = match self.rx.recv_timeout(timeout) {
                Ok(Ok(event)) => event,
                Ok(Err(e)) => {
                    error!("Watcher error: {}", e);
                    return None;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => return None,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    error!("Watcher channel disconnected");
                    return None;
                }
            };

            // Classify the event kind.
            let kind = match raw_event.kind {
                EventKind::Create(_) => FsEventKind::Assert,
                EventKind::Remove(_) => FsEventKind::Retract,
                EventKind::Modify(notify::event::ModifyKind::Name(_)) => FsEventKind::Transform,
                EventKind::Modify(_) => {
                    // Content/metadata modifications — treat as assert (re-touch).
                    FsEventKind::Assert
                }
                _ => {
                    trace!("Ignoring event kind: {:?}", raw_event.kind);
                    continue;
                }
            };

            // Process the first path in the event.
            let abs_path = match raw_event.paths.first() {
                Some(p) => p.clone(),
                None => continue,
            };

            // Skip dotfiles.
            if let Some(name) = abs_path.file_name() {
                if name.to_string_lossy().starts_with('.') {
                    trace!("Skipping dotfile event: {}", abs_path.display());
                    continue;
                }
            }

            // Compute relative path.
            let relative = match abs_path.strip_prefix(&self.fs_root) {
                Ok(r) => r.to_path_buf(),
                Err(_) => {
                    trace!("Event outside fs_root: {}", abs_path.display());
                    continue;
                }
            };

            // Loop avoidance: check if this is an engine-generated event.
            if self.effector.consume_pending(&abs_path) {
                trace!(
                    "Skipping engine-generated event: {}",
                    relative.display()
                );
                continue;
            }

            debug!("{:?} → {}", kind, relative.display());

            return Some(WatchEvent {
                kind,
                relative_path: relative,
                absolute_path: abs_path,
            });
        }
    }

    /// Drain all immediately available events (non-blocking).
    pub fn drain(&self) -> Vec<WatchEvent> {
        let mut events = Vec::new();
        while let Some(event) = self.poll(Duration::from_millis(1)) {
            events.push(event);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_watch_fs() -> TempDir {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        // Create watched scopes.
        for scope in WATCHED_SCOPES {
            std::fs::create_dir_all(root.join(scope)).unwrap();
        }
        std::fs::create_dir_all(root.join("hard")).unwrap();
        dir
    }

    #[test]
    fn test_watcher_starts() {
        let dir = setup_watch_fs();
        let effector = Effector::new(dir.path().to_path_buf());
        let watcher = FsWatcher::start(dir.path(), effector);
        assert!(watcher.is_ok());
    }

    #[test]
    #[ignore = "platform-dependent: FSEvents latency on macOS"]
    fn test_watcher_detects_touch() {
        let dir = setup_watch_fs();
        let effector = Effector::new(dir.path().to_path_buf());
        let watcher = FsWatcher::start(dir.path(), effector).unwrap();

        // Give kqueue/inotify time to stabilize.
        std::thread::sleep(Duration::from_millis(200));

        // Touch a file in a watched scope.
        std::fs::File::create(dir.path().join("jobs/test_job")).unwrap();

        // Poll with generous timeout.
        let event = watcher.poll(Duration::from_secs(3));
        assert!(event.is_some(), "Should detect the touch event");

        let event = event.unwrap();
        assert_eq!(event.kind, FsEventKind::Assert);
        assert!(event.relative_path.starts_with("jobs"));
    }

    #[test]
    fn test_watcher_skips_dotfiles() {
        let dir = setup_watch_fs();
        let effector = Effector::new(dir.path().to_path_buf());
        let watcher = FsWatcher::start(dir.path(), effector).unwrap();

        // Create a dotfile.
        std::fs::File::create(dir.path().join("jobs/.gitkeep")).unwrap();

        // Should NOT detect it (poll should timeout).
        let event = watcher.poll(Duration::from_millis(500));
        assert!(event.is_none(), "Dotfiles should be filtered out");
    }

    #[tokio::test]
    async fn test_watcher_skips_effector_events() {
        let dir = setup_watch_fs();
        let effector = Effector::new(dir.path().to_path_buf());
        let watcher = FsWatcher::start(dir.path(), effector.clone()).unwrap();

        // Use the effector to create a file (should be skipped by watcher).
        effector
            .execute(&crate::effector::Effect::Touch {
                path: PathBuf::from("events/!engine_event"),
            })
            .await
            .unwrap();

        // The watcher should NOT report this (pending ops filter).
        let event = watcher.poll(Duration::from_millis(500));
        assert!(
            event.is_none(),
            "Effector-generated events should be filtered"
        );
    }

    #[test]
    #[ignore = "platform-dependent: FSEvents latency on macOS"]
    fn test_watcher_detects_remove() {
        let dir = setup_watch_fs();
        let effector = Effector::new(dir.path().to_path_buf());

        // Create a file first.
        let file_path = dir.path().join("states/test_state");
        std::fs::File::create(&file_path).unwrap();

        let watcher = FsWatcher::start(dir.path(), effector).unwrap();

        // Give kqueue/inotify time to stabilize.
        std::thread::sleep(Duration::from_millis(300));
        std::fs::remove_file(&file_path).unwrap();

        // Should detect the remove.
        let event = watcher.poll(Duration::from_secs(3));
        assert!(event.is_some(), "Should detect the remove event");
        assert_eq!(event.unwrap().kind, FsEventKind::Retract);
    }
}
