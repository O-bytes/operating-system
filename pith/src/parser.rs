use std::path::Path;
use unicode_normalization::UnicodeNormalization;

use crate::alphabet::Alphabet;

/// Classification of a single path segment (filename or directory name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeClass {
    /// Data node: plain value, name IS the data.
    /// Example: `blue`, `001`, `anxiety`
    Data(String),

    /// Instruction node: name starts with a logic door character.
    /// The `door` is the logic door, `arg` is the remainder.
    /// Example: `-expected` → door='-', arg="expected"
    Instruction { door: char, arg: String },

    /// Pointer node: name starts with `€`, escaping the next character.
    /// The content is the literal value after `€`.
    /// Example: `€$price` → content="$price"
    Pointer(String),
}

/// A fully parsed path with classified segments.
#[derive(Debug, Clone)]
pub struct ParsedPath {
    /// Each segment of the path, classified.
    pub segments: Vec<NodeClass>,

    /// The top-level scope (first segment after root).
    pub scope: Option<String>,

    /// Whether the original path pointed to a file (leaf) or directory.
    pub is_leaf: bool,
}

/// Parse a single path segment (filename or directory name) given the alphabet.
///
/// Classification rules:
/// 1. If starts with `€` → Pointer (escape: rest is literal)
/// 2. If starts with a reserved character → Instruction (door + argument)
/// 3. Otherwise → Data (plain value)
pub fn classify_segment(name: &str, alphabet: &Alphabet) -> NodeClass {
    // Normalize to NFC for consistent comparison (APFS may store NFD).
    let normalized: String = name.nfc().collect();

    let mut chars = normalized.chars();
    match chars.next() {
        None => NodeClass::Data(String::new()),
        Some(first) => {
            if alphabet.is_escape(first) {
                // Pointer node: everything after € is the literal content.
                let content: String = chars.collect();
                NodeClass::Pointer(content)
            } else if alphabet.is_reserved(first) {
                // Instruction node: first char is the logic door.
                let arg: String = chars.collect();
                NodeClass::Instruction {
                    door: first,
                    arg,
                }
            } else {
                // Data node: the name IS the value.
                NodeClass::Data(normalized)
            }
        }
    }
}

/// Parse a full filesystem path into classified segments.
///
/// The path is split by `/`, each segment is classified.
/// The `root` prefix is stripped if present (e.g., `/Users/.../src/` → starts at first scope).
pub fn parse_path(path: &Path, fs_root: &Path, alphabet: &Alphabet, is_file: bool) -> ParsedPath {
    // Strip the fs_root prefix to get the relative path within the OS.
    let relative = path.strip_prefix(fs_root).unwrap_or(path);

    let segments: Vec<NodeClass> = relative
        .components()
        .filter_map(|c| {
            let s = c.as_os_str().to_string_lossy();
            if s.is_empty() || s.starts_with('.') {
                None // Skip empty segments and dotfiles
            } else {
                Some(classify_segment(&s, alphabet))
            }
        })
        .collect();

    let scope = relative
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_string());

    ParsedPath {
        segments,
        scope,
        is_leaf: is_file,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_alphabet() -> Alphabet {
        // Build a minimal alphabet for testing.
        let dir = tempfile::TempDir::new().unwrap();
        for ch in ['$', '-', '!', '#', '§', '~', '(', ')', '[', ']', '{', '}', '@', '*', '+', ',', '|', ':', '?', '^', '&', '%', '<', '>', '=', ';', '_'] {
            std::fs::File::create(dir.path().join(ch.to_string())).unwrap();
        }
        std::fs::File::create(dir.path().join("€")).unwrap();
        Alphabet::load(dir.path()).unwrap()
    }

    #[test]
    fn test_data_node() {
        let alphabet = test_alphabet();
        assert_eq!(
            classify_segment("blue", &alphabet),
            NodeClass::Data("blue".to_string())
        );
        assert_eq!(
            classify_segment("001", &alphabet),
            NodeClass::Data("001".to_string())
        );
    }

    #[test]
    fn test_instruction_node() {
        let alphabet = test_alphabet();
        assert_eq!(
            classify_segment("-expected", &alphabet),
            NodeClass::Instruction {
                door: '-',
                arg: "expected".to_string()
            }
        );
        assert_eq!(
            classify_segment("!completed", &alphabet),
            NodeClass::Instruction {
                door: '!',
                arg: "completed".to_string()
            }
        );
        assert_eq!(
            classify_segment("§read", &alphabet),
            NodeClass::Instruction {
                door: '§',
                arg: "read".to_string()
            }
        );
        assert_eq!(
            classify_segment("~42", &alphabet),
            NodeClass::Instruction {
                door: '~',
                arg: "42".to_string()
            }
        );
    }

    #[test]
    fn test_pointer_node() {
        let alphabet = test_alphabet();
        assert_eq!(
            classify_segment("€$price", &alphabet),
            NodeClass::Pointer("$price".to_string())
        );
        assert_eq!(
            classify_segment("€€", &alphabet),
            NodeClass::Pointer("€".to_string())
        );
        assert_eq!(
            classify_segment("€#channel", &alphabet),
            NodeClass::Pointer("#channel".to_string())
        );
    }

    #[test]
    fn test_parse_full_path() {
        let alphabet = test_alphabet();
        let root = Path::new("/tmp/test_os/src");
        let path = Path::new("/tmp/test_os/src/hard/identities/001/-expected/type/identity");

        let parsed = parse_path(path, root, &alphabet, true);

        assert_eq!(parsed.segments.len(), 6);
        assert_eq!(parsed.segments[0], NodeClass::Data("hard".to_string()));
        assert_eq!(parsed.segments[1], NodeClass::Data("identities".to_string()));
        assert_eq!(parsed.segments[2], NodeClass::Data("001".to_string()));
        assert_eq!(
            parsed.segments[3],
            NodeClass::Instruction {
                door: '-',
                arg: "expected".to_string()
            }
        );
        assert_eq!(parsed.segments[4], NodeClass::Data("type".to_string()));
        assert_eq!(parsed.segments[5], NodeClass::Data("identity".to_string()));
        assert_eq!(parsed.scope, Some("hard".to_string()));
        assert!(parsed.is_leaf);
    }
}
