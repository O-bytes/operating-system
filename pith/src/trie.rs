use std::collections::BTreeMap;
use std::path::Path;
use std::time::SystemTime;
use tracing::info;
use walkdir::WalkDir;

use crate::alphabet::Alphabet;
use crate::error::{PithError, Result};
use crate::parser::{classify_segment, NodeClass};

/// A node in the in-memory trie representing the 0-bytes filesystem.
///
/// The trie IS the L1 cache of the OS state. The filesystem is main memory.
/// BTreeMap is used for deterministic ordering (important for channels, sequences).
#[derive(Debug, Clone)]
pub struct TrieNode {
    /// This segment's raw name.
    pub segment: String,

    /// Classification of this segment (Data/Instruction/Pointer).
    pub class: NodeClass,

    /// Ordered children by segment name.
    pub children: BTreeMap<String, TrieNode>,

    /// Whether this node represents a file (leaf) or directory (scope).
    pub is_file: bool,

    /// Modification time — used by the scheduler and for ordering.
    pub mtime: Option<SystemTime>,
}

impl TrieNode {
    /// Create a new trie node.
    pub fn new(segment: String, class: NodeClass, is_file: bool) -> Self {
        Self {
            segment,
            class,
            children: BTreeMap::new(),
            is_file,
            mtime: None,
        }
    }

    /// Count total nodes in this subtree (including self).
    pub fn count(&self) -> usize {
        1 + self.children.values().map(|c| c.count()).sum::<usize>()
    }
}

/// The in-memory trie index for the entire 0-bytes filesystem.
///
/// Built at boot by walking `src/`. Updated incrementally by the watcher.
/// The root node represents the `fs_root` directory itself.
#[derive(Debug)]
pub struct Trie {
    pub root: TrieNode,
}

/// Paths to skip during the initial filesystem walk.
const SKIP_DIRS: &[&str] = &["pointers/unicodes", ".git"];

impl Trie {
    /// Build the trie by walking the entire filesystem under `fs_root`.
    ///
    /// Skips:
    /// - `pointers/unicodes/` (65k dirs, ROM — loaded separately if needed)
    /// - `.git*` directories and dotfiles
    pub fn build(fs_root: &Path, alphabet: &Alphabet) -> Result<Self> {
        let mut root = TrieNode::new(
            String::new(),
            NodeClass::Data(String::new()),
            false,
        );

        let walker = WalkDir::new(fs_root)
            .follow_links(false)
            .sort_by_file_name();

        let mut count = 0u64;

        for entry in walker {
            let entry = entry.map_err(|e| PithError::Io {
                path: fs_root.to_path_buf(),
                source: e.into(),
            })?;

            let path = entry.path();

            // Get relative path from fs_root.
            let relative = match path.strip_prefix(fs_root) {
                Ok(r) => r,
                Err(_) => continue,
            };

            // Skip the root itself.
            if relative.as_os_str().is_empty() {
                continue;
            }

            let relative_str = relative.to_string_lossy();

            // Skip dotfiles and dotdirs (.git, .gitkeep, .gitmodules, .DS_Store).
            if relative.components().any(|c| {
                c.as_os_str()
                    .to_string_lossy()
                    .starts_with('.')
            }) {
                continue;
            }

            // Skip known large/static subtrees.
            let should_skip = SKIP_DIRS
                .iter()
                .any(|skip| relative_str.starts_with(skip));
            if should_skip {
                continue;
            }

            let is_file = entry.file_type().is_file();
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok());

            // Walk segments and insert into trie.
            let segments: Vec<String> = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect();

            let mut current = &mut root;
            for (i, seg_name) in segments.iter().enumerate() {
                let is_last = i == segments.len() - 1;
                let node_is_file = is_last && is_file;

                current = current
                    .children
                    .entry(seg_name.clone())
                    .or_insert_with(|| {
                        let class = classify_segment(seg_name, alphabet);
                        TrieNode::new(seg_name.clone(), class, node_is_file)
                    });

                if is_last {
                    current.is_file = node_is_file;
                    if node_is_file {
                        current.mtime = mtime;
                    }
                }
            }

            count += 1;
        }

        info!("Trie built: {} entries indexed from {}", count, fs_root.display());

        Ok(Self { root })
    }

    /// Lookup a node by its relative path segments.
    pub fn get(&self, segments: &[&str]) -> Option<&TrieNode> {
        let mut current = &self.root;
        for seg in segments {
            current = current.children.get(*seg)?;
        }
        Some(current)
    }

    /// Lookup a node by its relative path segments (mutable).
    pub fn get_mut(&mut self, segments: &[&str]) -> Option<&mut TrieNode> {
        let mut current = &mut self.root;
        for seg in segments {
            current = current.children.get_mut(*seg)?;
        }
        Some(current)
    }

    /// Insert a node at the given path. Creates intermediate directories as needed.
    pub fn insert(&mut self, segments: &[String], is_file: bool, alphabet: &Alphabet) {
        let mut current = &mut self.root;
        for (i, seg_name) in segments.iter().enumerate() {
            let is_last = i == segments.len() - 1;
            let node_is_file = is_last && is_file;

            current = current
                .children
                .entry(seg_name.clone())
                .or_insert_with(|| {
                    let class = classify_segment(seg_name, alphabet);
                    TrieNode::new(seg_name.clone(), class, node_is_file)
                });

            if is_last {
                current.is_file = node_is_file;
            }
        }
    }

    /// Remove a node at the given path. Returns true if found and removed.
    pub fn remove(&mut self, segments: &[&str]) -> bool {
        if segments.is_empty() {
            return false;
        }

        // Navigate to the parent, then remove the last segment.
        let (parent_segs, last) = segments.split_at(segments.len() - 1);

        let parent = if parent_segs.is_empty() {
            &mut self.root
        } else {
            match self.get_mut(parent_segs) {
                Some(node) => node,
                None => return false,
            }
        };

        parent.children.remove(last[0]).is_some()
    }

    /// List all children of a node at the given path.
    pub fn list(&self, segments: &[&str]) -> Option<Vec<&str>> {
        let node = self.get(segments)?;
        Some(node.children.keys().map(|k| k.as_str()).collect())
    }

    /// Count total nodes in the trie (excluding root).
    pub fn total_nodes(&self) -> usize {
        self.root.count() - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_fs() -> (TempDir, Alphabet) {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create hard/reserved with a few logic doors.
        let reserved = root.join("hard/reserved");
        std::fs::create_dir_all(&reserved).unwrap();
        std::fs::File::create(reserved.join("$")).unwrap();
        std::fs::File::create(reserved.join("-")).unwrap();
        std::fs::File::create(reserved.join("!")).unwrap();
        std::fs::File::create(reserved.join("§")).unwrap();
        std::fs::File::create(reserved.join("€")).unwrap();

        // Create some OS structure.
        let id_path = root.join("hard/identities/001/-expected/type");
        std::fs::create_dir_all(&id_path).unwrap();
        std::fs::File::create(id_path.join("identity")).unwrap();

        let jobs = root.join("jobs/0");
        std::fs::create_dir_all(&jobs).unwrap();

        let states = root.join("states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::File::create(states.join("0")).unwrap();

        let alphabet = Alphabet::load(&reserved).unwrap();
        (dir, alphabet)
    }

    #[test]
    fn test_build_trie() {
        let (dir, alphabet) = setup_test_fs();
        let trie = Trie::build(dir.path(), &alphabet).unwrap();

        // Should have hard/ and jobs/ and states/ at top level.
        assert!(trie.root.children.contains_key("hard"));
        assert!(trie.root.children.contains_key("jobs"));
        assert!(trie.root.children.contains_key("states"));
    }

    #[test]
    fn test_lookup() {
        let (dir, alphabet) = setup_test_fs();
        let trie = Trie::build(dir.path(), &alphabet).unwrap();

        let node = trie.get(&["hard", "identities", "001", "-expected", "type", "identity"]);
        assert!(node.is_some());
        assert!(node.unwrap().is_file);
    }

    #[test]
    fn test_list() {
        let (dir, alphabet) = setup_test_fs();
        let trie = Trie::build(dir.path(), &alphabet).unwrap();

        let children = trie.list(&["hard"]).unwrap();
        assert!(children.contains(&"reserved"));
        assert!(children.contains(&"identities"));
    }

    #[test]
    fn test_insert_and_remove() {
        let (dir, alphabet) = setup_test_fs();
        let mut trie = Trie::build(dir.path(), &alphabet).unwrap();

        // Insert a new path.
        trie.insert(
            &["events".to_string(), "!boot".to_string()],
            true,
            &alphabet,
        );
        assert!(trie.get(&["events", "!boot"]).is_some());

        // Remove it.
        assert!(trie.remove(&["events", "!boot"]));
        assert!(trie.get(&["events", "!boot"]).is_none());
    }

    #[test]
    fn test_dotfiles_skipped() {
        let (dir, alphabet) = setup_test_fs();

        // Create a .gitkeep file.
        std::fs::File::create(dir.path().join("jobs/.gitkeep")).unwrap();

        let trie = Trie::build(dir.path(), &alphabet).unwrap();

        // .gitkeep should NOT be in the trie.
        let jobs_children = trie.list(&["jobs"]);
        if let Some(children) = jobs_children {
            assert!(!children.contains(&".gitkeep"));
        }
    }
}
