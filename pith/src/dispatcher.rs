/// Event dispatcher — routes filesystem events to the correct subsystem
/// and updates the in-memory trie.
///
/// The dispatcher receives WatchEvents from the watcher, classifies them
/// by scope, updates the trie, and logs the event. In later phases,
/// it will also route to subsystem handlers.

use std::sync::{Arc, RwLock};

use tracing::{debug, info, warn};

use crate::alphabet::Alphabet;
use crate::subsystems::{FsEvent, FsEventKind};
use crate::trie::Trie;
use crate::watcher::WatchEvent;

/// Top-level scopes of the 0-bytes filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Hard,
    States,
    Jobs,
    Workers,
    Channels,
    Events,
    Programs,
    Databases,
    Pointers,
    Schedules,
    Sessions,
    Subscriptions,
    Logs,
    Tmp,
}

impl Scope {
    /// Parse a scope from the first segment of a relative path.
    pub fn from_segment(segment: &str) -> Option<Self> {
        match segment {
            "hard" => Some(Self::Hard),
            "states" => Some(Self::States),
            "jobs" => Some(Self::Jobs),
            "workers" => Some(Self::Workers),
            "channels" => Some(Self::Channels),
            "events" => Some(Self::Events),
            "programs" => Some(Self::Programs),
            "databases" => Some(Self::Databases),
            "pointers" => Some(Self::Pointers),
            "schedules" => Some(Self::Schedules),
            "sessions" => Some(Self::Sessions),
            "subscriptions" => Some(Self::Subscriptions),
            "logs" => Some(Self::Logs),
            "tmp" => Some(Self::Tmp),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Hard => "hard",
            Self::States => "states",
            Self::Jobs => "jobs",
            Self::Workers => "workers",
            Self::Channels => "channels",
            Self::Events => "events",
            Self::Programs => "programs",
            Self::Databases => "databases",
            Self::Pointers => "pointers",
            Self::Schedules => "schedules",
            Self::Sessions => "sessions",
            Self::Subscriptions => "subscriptions",
            Self::Logs => "logs",
            Self::Tmp => "tmp",
        }
    }
}

/// The event dispatcher — routes events and updates the trie.
pub struct Dispatcher {
    trie: Arc<RwLock<Trie>>,
    alphabet: Alphabet,
}

impl Dispatcher {
    pub fn new(trie: Arc<RwLock<Trie>>, alphabet: Alphabet) -> Self {
        Self { trie, alphabet }
    }

    /// Dispatch a watcher event: classify, update trie, route to subsystem.
    ///
    /// Returns the parsed FsEvent if the event was successfully dispatched,
    /// or None if it was filtered out (unknown scope, etc.).
    pub fn dispatch(&self, watch_event: &WatchEvent) -> Option<FsEvent> {
        let segments: Vec<String> = watch_event
            .relative_path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();

        if segments.is_empty() {
            return None;
        }

        // Determine scope from first segment.
        let scope = match Scope::from_segment(&segments[0]) {
            Some(s) => s,
            None => {
                warn!(
                    "Unknown scope for event: {}",
                    watch_event.relative_path.display()
                );
                return None;
            }
        };

        // Protect hard/ scope — log but allow for now.
        if scope == Scope::Hard {
            warn!(
                "External modification detected in protected scope hard/: {:?} {}",
                watch_event.kind,
                watch_event.relative_path.display()
            );
        }

        // Update the trie.
        self.update_trie(&segments, watch_event);

        let fs_event = FsEvent {
            kind: watch_event.kind,
            segments: segments.clone(),
            scope,
        };

        info!(
            "[{}] {:?} {}",
            scope.name(),
            watch_event.kind,
            watch_event.relative_path.display()
        );

        // TODO: Phase 5+ — route to subsystem handler based on scope.

        Some(fs_event)
    }

    /// Update the in-memory trie based on the event.
    fn update_trie(&self, segments: &[String], event: &WatchEvent) {
        let mut trie = self.trie.write().unwrap();

        match event.kind {
            FsEventKind::Assert => {
                // File or directory created — check if it's a file.
                let is_file = event.absolute_path.is_file();
                trie.insert(segments, is_file, &self.alphabet);
                debug!("Trie insert: {} (file={})", segments.join("/"), is_file);
            }
            FsEventKind::Retract => {
                // File or directory removed.
                let seg_refs: Vec<&str> = segments.iter().map(|s| s.as_str()).collect();
                if trie.remove(&seg_refs) {
                    debug!("Trie remove: {}", segments.join("/"));
                }
            }
            FsEventKind::Transform => {
                // For rename/move, we get two events (remove old + create new)
                // from the watcher, so individual handling suffices.
                // If we only see the transform event, treat as assert.
                let is_file = event.absolute_path.is_file();
                trie.insert(segments, is_file, &self.alphabet);
                debug!("Trie transform: {}", segments.join("/"));
            }
        }
    }

    /// Get a read lock on the trie.
    pub fn trie(&self) -> Arc<RwLock<Trie>> {
        Arc::clone(&self.trie)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use crate::parser::NodeClass;

    fn make_trie() -> Trie {
        use crate::trie::TrieNode;
        Trie {
            root: TrieNode::new(String::new(), NodeClass::Data(String::new()), false),
        }
    }

    fn make_alphabet() -> Alphabet {
        let dir = tempfile::TempDir::new().unwrap();
        for ch in ['$', '-', '!', '§', '€'] {
            std::fs::File::create(dir.path().join(ch.to_string())).unwrap();
        }
        Alphabet::load(dir.path()).unwrap()
    }

    #[test]
    fn test_scope_from_segment() {
        assert_eq!(Scope::from_segment("jobs"), Some(Scope::Jobs));
        assert_eq!(Scope::from_segment("hard"), Some(Scope::Hard));
        assert_eq!(Scope::from_segment("unknown"), None);
    }

    #[test]
    fn test_dispatch_updates_trie_on_assert() {
        let trie = Arc::new(RwLock::new(make_trie()));
        let alphabet = make_alphabet();
        let dispatcher = Dispatcher::new(trie.clone(), alphabet);

        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("test_file");
        std::fs::File::create(&file_path).unwrap();

        let event = WatchEvent {
            kind: FsEventKind::Assert,
            relative_path: PathBuf::from("jobs/1"),
            absolute_path: file_path,
        };

        let fs_event = dispatcher.dispatch(&event);
        assert!(fs_event.is_some());
        assert_eq!(fs_event.unwrap().scope, Scope::Jobs);

        // Trie should now contain jobs/1.
        let trie = trie.read().unwrap();
        assert!(trie.get(&["jobs", "1"]).is_some());
    }

    #[test]
    fn test_dispatch_updates_trie_on_retract() {
        let trie = Arc::new(RwLock::new(make_trie()));
        let alphabet = make_alphabet();
        let dispatcher = Dispatcher::new(trie.clone(), alphabet.clone());

        // First insert something.
        {
            let mut t = trie.write().unwrap();
            t.insert(&["jobs".to_string(), "1".to_string()], true, &alphabet);
        }

        let event = WatchEvent {
            kind: FsEventKind::Retract,
            relative_path: PathBuf::from("jobs/1"),
            absolute_path: PathBuf::from("/fake/jobs/1"),
        };

        dispatcher.dispatch(&event);

        // Should be removed from trie.
        let t = trie.read().unwrap();
        assert!(t.get(&["jobs", "1"]).is_none());
    }

    #[test]
    fn test_dispatch_warns_on_hard_scope() {
        let trie = Arc::new(RwLock::new(make_trie()));
        let alphabet = make_alphabet();
        let dispatcher = Dispatcher::new(trie, alphabet);

        let event = WatchEvent {
            kind: FsEventKind::Assert,
            relative_path: PathBuf::from("hard/reserved/new_char"),
            absolute_path: PathBuf::from("/fake/hard/reserved/new_char"),
        };

        // Should still dispatch (logging the warning).
        let fs_event = dispatcher.dispatch(&event);
        assert!(fs_event.is_some());
        assert_eq!(fs_event.unwrap().scope, Scope::Hard);
    }
}
