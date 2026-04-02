/// Databases subsystem — handles `databases/` scope.
///
/// The database is the filesystem hierarchy itself. This subsystem provides
/// query capabilities over path-encoded data:
///
///   - Set membership: leaf files are members of the set defined by their parent
///     databases/psychology/blue/effects/anxiety → anxiety ∈ effects(blue, psychology)
///
///   - Key-value: single-child directories encode k:v pairs
///     databases/translations/english/french/colors/blue/bleu → blue → bleu
///
///   - Cross-references: ∆ markers create bidirectional links
///     databases/colors/blue/psychology/∆psychology∆blue
///
/// The subsystem watches for data changes and logs them.

use tracing::info;

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::logs::LogsSubsystem;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};

pub struct DatabasesSubsystem;

impl DatabasesSubsystem {
    pub fn new() -> Self {
        Self
    }
}

impl Subsystem for DatabasesSubsystem {
    fn scope(&self) -> Scope {
        Scope::Databases
    }

    fn handle(&self, event: &FsEvent) -> Vec<Effect> {
        let mut effects = Vec::new();

        if event.segments.len() < 2 {
            return effects;
        }

        let path_str = event.segments[1..].join("/");

        match event.kind {
            FsEventKind::Assert => {
                // Detect cross-reference markers (∆).
                let last = &event.segments[event.segments.len() - 1];
                if last.contains('∆') {
                    info!("Database cross-reference: {}", path_str);
                    effects.push(LogsSubsystem::log_effect(
                        &format!("db xref {}", path_str),
                    ));
                } else {
                    info!("Database entry added: {}", path_str);
                }
            }
            FsEventKind::Retract => {
                info!("Database entry removed: {}", path_str);
            }
            _ => {}
        }

        effects
    }
}

/// Query the trie for database entries.
///
/// Returns all leaf file names under the given path (set members).
pub fn query_set(
    trie: &crate::trie::Trie,
    path_segments: &[&str],
) -> Vec<String> {
    let mut full_path = vec!["databases"];
    full_path.extend_from_slice(path_segments);

    match trie.get(&full_path) {
        Some(node) => collect_leaves(node),
        None => Vec::new(),
    }
}

/// Recursively collect all leaf file names in a subtree.
fn collect_leaves(node: &crate::trie::TrieNode) -> Vec<String> {
    let mut results = Vec::new();

    for (name, child) in &node.children {
        if child.is_file && child.children.is_empty() {
            results.push(name.clone());
        } else {
            results.extend(collect_leaves(child));
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alphabet::Alphabet;
    use crate::trie::Trie;
    use tempfile::TempDir;

    fn setup_db_fs() -> (TempDir, Trie) {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Reserved
        let reserved = root.join("hard/reserved");
        std::fs::create_dir_all(&reserved).unwrap();
        for ch in ['€', '-', '!', '§', '∆'] {
            std::fs::File::create(reserved.join(ch.to_string())).unwrap();
        }

        // Database entries
        let effects = root.join("databases/psychology/blue/effects");
        std::fs::create_dir_all(&effects).unwrap();
        std::fs::File::create(effects.join("anxiety")).unwrap();
        std::fs::File::create(effects.join("bad_sleep")).unwrap();

        let trans = root.join("databases/translations/en/fr/colors/blue");
        std::fs::create_dir_all(&trans).unwrap();
        std::fs::File::create(trans.join("bleu")).unwrap();

        let alphabet = Alphabet::load(&reserved).unwrap();
        let trie = Trie::build(root, &alphabet).unwrap();
        (dir, trie)
    }

    #[test]
    fn test_query_set_members() {
        let (_dir, trie) = setup_db_fs();
        let results = query_set(&trie, &["psychology", "blue", "effects"]);
        assert!(results.contains(&"anxiety".to_string()));
        assert!(results.contains(&"bad_sleep".to_string()));
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_query_translation() {
        let (_dir, trie) = setup_db_fs();
        let results = query_set(&trie, &["translations", "en", "fr", "colors", "blue"]);
        assert_eq!(results, vec!["bleu".to_string()]);
    }

    #[test]
    fn test_query_nonexistent() {
        let (_dir, trie) = setup_db_fs();
        let results = query_set(&trie, &["nonexistent"]);
        assert!(results.is_empty());
    }

    #[test]
    fn test_handle_entry_added() {
        let sub = DatabasesSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "databases".to_string(),
                "colors".to_string(),
                "red".to_string(),
            ],
            scope: Scope::Databases,
        };
        // Should not panic, no effects for regular entries.
        let effects = sub.handle(&event);
        assert!(effects.is_empty());
    }

    #[test]
    fn test_handle_cross_reference() {
        let sub = DatabasesSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "databases".to_string(),
                "colors".to_string(),
                "blue".to_string(),
                "psychology".to_string(),
                "∆psychology∆blue".to_string(),
            ],
            scope: Scope::Databases,
        };
        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 1); // log for xref
    }
}
