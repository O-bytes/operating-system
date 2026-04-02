use std::collections::HashSet;
use std::path::Path;
use tracing::{debug, info, warn};
use unicode_normalization::UnicodeNormalization;

use crate::error::{PithError, Result};

/// The ONE hardcoded value in the entire system.
/// `€` (U+20AC) is the escape logic door — the axiom from which everything else is derived.
/// Without it, the engine cannot distinguish pointer nodes from instruction nodes.
pub const ESCAPE: char = '€';

/// The self-describing alphabet of logic doors.
///
/// Loaded at boot from `hard/reserved/` — each zero-byte file's name IS a logic door.
/// The engine does NOT hardcode reserved characters (except `€`).
#[derive(Debug, Clone)]
pub struct Alphabet {
    /// All reserved characters (logic doors).
    reserved: HashSet<char>,
}

impl Alphabet {
    /// Load the alphabet from the `hard/reserved/` directory.
    ///
    /// Each file in the directory is a zero-byte file whose name is a single logic door character.
    /// The `€` escape character is always included, even if its file is missing.
    pub fn load(reserved_dir: &Path) -> Result<Self> {
        let mut reserved = HashSet::new();

        // The axiom: € is always reserved, regardless of filesystem state.
        reserved.insert(ESCAPE);

        let entries = std::fs::read_dir(reserved_dir).map_err(|e| PithError::Boot {
            reason: format!(
                "Cannot read reserved alphabet at {}: {}",
                reserved_dir.display(),
                e
            ),
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| PithError::Io {
                path: reserved_dir.to_path_buf(),
                source: e,
            })?;

            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            // Normalize to NFC (macOS APFS may store as NFD).
            let normalized: String = name.nfc().collect();

            // Extract the first character — it IS the logic door.
            if let Some(ch) = normalized.chars().next() {
                // Skip dotfiles (.gitkeep, .DS_Store, etc.)
                if ch == '.' {
                    continue;
                }

                reserved.insert(ch);
                debug!(
                    "Loaded logic door: '{}' (U+{:04X})",
                    ch, ch as u32
                );
            }
        }

        info!(
            "Alphabet loaded: {} logic doors from {}",
            reserved.len(),
            reserved_dir.display()
        );

        if reserved.len() < 2 {
            warn!("Alphabet has very few logic doors — is hard/reserved/ populated?");
        }

        Ok(Self { reserved })
    }

    /// Check if a character is a reserved logic door.
    pub fn is_reserved(&self, ch: char) -> bool {
        self.reserved.contains(&ch)
    }

    /// Check if a character is the escape character.
    pub fn is_escape(&self, ch: char) -> bool {
        ch == ESCAPE
    }

    /// Get the number of logic doors in the alphabet.
    pub fn len(&self) -> usize {
        self.reserved.len()
    }

    /// Check if the alphabet is empty (should never happen — at least € exists).
    pub fn is_empty(&self) -> bool {
        self.reserved.is_empty()
    }

    /// Iterate over all reserved characters.
    pub fn iter(&self) -> impl Iterator<Item = &char> {
        self.reserved.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_escape_is_always_present() {
        let dir = TempDir::new().unwrap();
        // Empty directory — only € should be loaded.
        let alphabet = Alphabet::load(dir.path()).unwrap();
        assert!(alphabet.is_reserved(ESCAPE));
        assert!(alphabet.is_escape('€'));
        assert_eq!(alphabet.len(), 1);
    }

    #[test]
    fn test_load_reserved_chars() {
        let dir = TempDir::new().unwrap();
        // Create some logic door files.
        std::fs::File::create(dir.path().join("$")).unwrap();
        std::fs::File::create(dir.path().join("-")).unwrap();
        std::fs::File::create(dir.path().join("!")).unwrap();
        std::fs::File::create(dir.path().join("€")).unwrap();

        let alphabet = Alphabet::load(dir.path()).unwrap();
        assert!(alphabet.is_reserved('$'));
        assert!(alphabet.is_reserved('-'));
        assert!(alphabet.is_reserved('!'));
        assert!(alphabet.is_reserved('€'));
        assert_eq!(alphabet.len(), 4);
    }

    #[test]
    fn test_dotfiles_ignored() {
        let dir = TempDir::new().unwrap();
        std::fs::File::create(dir.path().join(".gitkeep")).unwrap();
        std::fs::File::create(dir.path().join("$")).unwrap();

        let alphabet = Alphabet::load(dir.path()).unwrap();
        assert!(!alphabet.is_reserved('.'));
        assert!(alphabet.is_reserved('$'));
        // € (axiom) + $
        assert_eq!(alphabet.len(), 2);
    }

    #[test]
    fn test_unicode_logic_doors() {
        let dir = TempDir::new().unwrap();
        std::fs::File::create(dir.path().join("§")).unwrap();
        std::fs::File::create(dir.path().join("λ")).unwrap();
        std::fs::File::create(dir.path().join("∞")).unwrap();

        let alphabet = Alphabet::load(dir.path()).unwrap();
        assert!(alphabet.is_reserved('§'));
        assert!(alphabet.is_reserved('λ'));
        assert!(alphabet.is_reserved('∞'));
    }
}
