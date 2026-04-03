/// Session management — binds API connections to identities.
///
/// When a client connects via Unix socket, the engine extracts
/// the peer's PID/UID via UCred and maps it to a 0-bytes identity.
///
/// Sessions are tracked in the filesystem at `sessions/~{id}/`.
///
/// The UID-to-identity mapping is loaded from the filesystem at boot:
///   `hard/identities/{id}/-uid/{unix_uid}`
///
/// Clients can upgrade their session identity via the `authenticate` op.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use tracing::{debug, info, warn};

use crate::effector::Effect;
use crate::error::{PithError, Result};
use crate::parser::NodeClass;
use crate::permissions::{PermissionEngine, PermissionResult, Verb};
use crate::trie::Trie;

/// A live session binding a Unix socket connection to an identity.
#[derive(Debug, Clone)]
pub struct Session {
    /// Monotonic session ID.
    pub id: u64,
    /// Resolved 0-bytes identity ID (can be upgraded via authenticate).
    pub identity_id: u64,
    /// Peer Unix UID from UCred.
    pub unix_uid: u32,
    /// Peer Unix PID (informational).
    pub unix_pid: Option<i32>,
    /// When this session was created.
    pub connected_at: SystemTime,
}

/// Manages the lifecycle of sessions.
///
/// Loaded at boot from the trie. Scans `hard/identities/{id}/-uid/{unix_uid}`
/// to build a UID→identity reverse lookup.
pub struct SessionManager {
    /// Monotonic counter for session IDs.
    next_id: AtomicU64,
    /// Unix UID → 0-bytes identity ID mapping.
    uid_map: HashMap<u32, u64>,
    /// Active sessions keyed by session ID.
    active: RwLock<HashMap<u64, Session>>,
    /// Default identity for unknown UIDs (Guest 800).
    default_identity: u64,
}

impl SessionManager {
    /// Load the UID mapping from the trie at boot.
    ///
    /// Scans `hard/identities/{id}` children for `-uid` instruction nodes.
    /// Pattern: `hard/identities/{id}/-uid/{unix_uid}`
    pub fn load(trie: &Trie) -> Self {
        let mut uid_map = HashMap::new();

        let ids_node = match trie.get(&["hard", "identities"]) {
            Some(node) => node,
            None => {
                warn!("hard/identities/ not found — no UID mappings loaded");
                return Self {
                    next_id: AtomicU64::new(1),
                    uid_map,
                    active: RwLock::new(HashMap::new()),
                    default_identity: 800,
                };
            }
        };

        for (id_name, id_node) in &ids_node.children {
            let identity_id: u64 = match id_name.parse() {
                Ok(n) => n,
                Err(_) => continue, // Skip non-numeric entries.
            };

            // Look for -uid instruction nodes (same pattern as -group).
            for (_name, child_node) in &id_node.children {
                if let NodeClass::Instruction { door: '-', ref arg } = child_node.class {
                    if arg == "uid" {
                        for uid_name in child_node.children.keys() {
                            if let Ok(unix_uid) = uid_name.parse::<u32>() {
                                if let Some(prev) = uid_map.insert(unix_uid, identity_id) {
                                    warn!(
                                        "UID {} mapped to multiple identities: {} and {} (using {})",
                                        unix_uid, prev, identity_id, identity_id
                                    );
                                }
                                debug!("UID {} → identity {}", unix_uid, identity_id);
                            }
                        }
                    }
                }
            }
        }

        info!("Session manager loaded: {} UID mappings", uid_map.len());

        Self {
            next_id: AtomicU64::new(1),
            uid_map,
            active: RwLock::new(HashMap::new()),
            default_identity: 800,
        }
    }

    /// Create a new session for a connecting client.
    ///
    /// Resolves the Unix UID to a 0-bytes identity via the uid_map.
    /// Falls back to the default identity (Guest 800) if unmapped.
    pub fn create_session(&self, unix_uid: u32, unix_pid: Option<i32>) -> Session {
        let session_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let identity_id = self
            .uid_map
            .get(&unix_uid)
            .copied()
            .unwrap_or(self.default_identity);

        let session = Session {
            id: session_id,
            identity_id,
            unix_uid,
            unix_pid,
            connected_at: SystemTime::now(),
        };

        {
            let mut active = self.active.write().unwrap();
            active.insert(session_id, session.clone());
        }

        info!(
            "Session {} created: identity={}, uid={}, pid={:?}",
            session_id, identity_id, unix_uid, unix_pid
        );

        session
    }

    /// Destroy a session (on client disconnect).
    ///
    /// Returns the removed session if it existed.
    pub fn destroy_session(&self, session_id: u64) -> Option<Session> {
        let session = {
            let mut active = self.active.write().unwrap();
            active.remove(&session_id)
        };

        if let Some(ref s) = session {
            info!(
                "Session {} destroyed: identity={}, uid={}",
                s.id, s.identity_id, s.unix_uid
            );
        }

        session
    }

    /// Get a snapshot of a session by ID.
    pub fn get_session(&self, session_id: u64) -> Option<Session> {
        let active = self.active.read().unwrap();
        active.get(&session_id).cloned()
    }

    /// Get all active session IDs (for shutdown cleanup).
    pub fn active_session_ids(&self) -> Vec<u64> {
        let active = self.active.read().unwrap();
        active.keys().copied().collect()
    }

    /// Upgrade a session's identity after successful password authentication.
    ///
    /// Returns the previous identity_id, or None if session not found.
    pub fn authenticate_session(&self, session_id: u64, new_identity_id: u64) -> Option<u64> {
        let mut active = self.active.write().unwrap();
        if let Some(session) = active.get_mut(&session_id) {
            let old = session.identity_id;
            session.identity_id = new_identity_id;
            info!(
                "Session {} authenticated: identity {} → {}",
                session_id, old, new_identity_id
            );
            Some(old)
        } else {
            None
        }
    }

    /// Generate the filesystem effects for creating a session entry.
    ///
    /// Creates:
    ///   sessions/~{session_id}/
    ///   sessions/~{session_id}/-identity/{identity_id}
    ///   sessions/~{session_id}/-uid/{unix_uid}
    ///   sessions/~{session_id}/-state/active
    pub fn session_effects_create(session: &Session) -> Vec<Effect> {
        let base = format!("sessions/~{}", session.id);
        vec![
            Effect::MakeDir {
                path: PathBuf::from(&base),
            },
            Effect::MakeDir {
                path: PathBuf::from(format!("{base}/-identity")),
            },
            Effect::Touch {
                path: PathBuf::from(format!("{base}/-identity/{}", session.identity_id)),
            },
            Effect::MakeDir {
                path: PathBuf::from(format!("{base}/-uid")),
            },
            Effect::Touch {
                path: PathBuf::from(format!("{base}/-uid/{}", session.unix_uid)),
            },
            Effect::MakeDir {
                path: PathBuf::from(format!("{base}/-state")),
            },
            Effect::Touch {
                path: PathBuf::from(format!("{base}/-state/active")),
            },
        ]
    }

    /// Generate the filesystem effects for destroying a session entry.
    pub fn session_effects_destroy(session: &Session) -> Vec<Effect> {
        let base = format!("sessions/~{}", session.id);
        vec![
            Effect::Remove {
                path: PathBuf::from(format!("{base}/-state/active")),
            },
            Effect::Remove {
                path: PathBuf::from(format!("{base}/-state")),
            },
            Effect::Remove {
                path: PathBuf::from(format!("{base}/-uid/{}", session.unix_uid)),
            },
            Effect::Remove {
                path: PathBuf::from(format!("{base}/-uid")),
            },
            Effect::Remove {
                path: PathBuf::from(format!(
                    "{base}/-identity/{}",
                    session.identity_id
                )),
            },
            Effect::Remove {
                path: PathBuf::from(format!("{base}/-identity")),
            },
            Effect::Remove {
                path: PathBuf::from(&base),
            },
        ]
    }
}

/// Per-connection context carrying session binding and permission engine.
///
/// Holds a `session_id` and `Arc<SessionManager>` so that the identity
/// can be upgraded mid-connection via the `authenticate` API op.
pub struct SessionContext {
    pub session_id: u64,
    pub session_manager: Arc<SessionManager>,
    pub permissions: Arc<PermissionEngine>,
    pub enforce: bool,
}

impl SessionContext {
    /// Get the current session snapshot (reflects authenticate upgrades).
    pub fn session(&self) -> Option<Session> {
        self.session_manager.get_session(self.session_id)
    }

    /// Get the current identity ID for this session.
    pub fn identity_id(&self) -> u64 {
        self.session()
            .map(|s| s.identity_id)
            .unwrap_or(800) // Fallback to Guest if session vanished.
    }

    /// Check if the current session's identity is allowed to perform
    /// the given verb on the target path.
    ///
    /// When `enforce` is false, always returns Ok (backward-compatible).
    pub fn check_permission(&self, verb: Verb, target: &[&str]) -> Result<()> {
        if !self.enforce {
            return Ok(());
        }

        let identity_id = self.identity_id();

        match self.permissions.check(identity_id, verb, target) {
            PermissionResult::Allow { .. } => Ok(()),
            PermissionResult::Deny { reason: _ } => Err(PithError::Permission {
                identity: identity_id,
                verb: format!("{:?}", verb),
                path: PathBuf::from(target.join("/")),
            }),
        }
    }
}

/// Map an API operation to the permission verb required.
///
/// Returns `None` for operations that are always allowed (e.g., ping, authenticate).
pub fn verb_for_op(op: &str) -> Option<Verb> {
    match op {
        "ping" | "authenticate" => None, // Always allowed.
        "status" | "ls" | "query" | "db_query" => Some(Verb::Read),
        "touch" | "mkdir" | "rm" | "mv" | "create_identity" => Some(Verb::Write),
        _ => Some(Verb::Execute), // Unknown ops require Execute.
    }
}

/// Clean all contents of the sessions/ directory (called at boot).
pub async fn clean_sessions(fs_root: &std::path::Path) -> crate::error::Result<()> {
    let sessions_dir = fs_root.join("sessions");
    if sessions_dir.exists() {
        let mut entries =
            tokio::fs::read_dir(&sessions_dir)
                .await
                .map_err(|e| PithError::Io {
                    path: sessions_dir.clone(),
                    source: e,
                })?;
        while let Some(entry) = entries.next_entry().await.map_err(|e| PithError::Io {
            path: sessions_dir.clone(),
            source: e,
        })? {
            let path = entry.path();
            if path
                .file_name()
                .map_or(false, |n| n.to_string_lossy().starts_with('.'))
            {
                continue;
            }
            if path.is_dir() {
                let _ = tokio::fs::remove_dir_all(&path).await;
            } else {
                let _ = tokio::fs::remove_file(&path).await;
            }
        }
        info!("Cleaned sessions/");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alphabet::Alphabet;
    use tempfile::TempDir;

    /// Build a test trie with UID mappings.
    fn setup_session_trie() -> (TempDir, Trie) {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create reserved alphabet.
        let reserved = root.join("hard/reserved");
        std::fs::create_dir_all(&reserved).unwrap();
        for ch in ['€', '-', '§', '~'] {
            std::fs::File::create(reserved.join(ch.to_string())).unwrap();
        }

        // Identity 600 (User) with UID 501.
        let id600 = root.join("hard/identities/600");
        std::fs::create_dir_all(id600.join("-uid")).unwrap();
        std::fs::File::create(id600.join("-uid/501")).unwrap();

        // Identity 300 (Root) with UID 0.
        let id300 = root.join("hard/identities/300");
        std::fs::create_dir_all(id300.join("-uid")).unwrap();
        std::fs::File::create(id300.join("-uid/0")).unwrap();

        // Identity 800 (Guest) — no UID mapping (default fallback).
        let id800 = root.join("hard/identities/800");
        std::fs::create_dir_all(&id800).unwrap();

        let alphabet = Alphabet::load(&reserved).unwrap();
        let trie = Trie::build(root, &alphabet).unwrap();

        (dir, trie)
    }

    #[test]
    fn test_session_manager_load() {
        let (_dir, trie) = setup_session_trie();
        let manager = SessionManager::load(&trie);

        assert_eq!(manager.uid_map.len(), 2);
        assert_eq!(manager.uid_map.get(&501), Some(&600));
        assert_eq!(manager.uid_map.get(&0), Some(&300));
    }

    #[test]
    fn test_create_session_known_uid() {
        let (_dir, trie) = setup_session_trie();
        let manager = SessionManager::load(&trie);

        let session = manager.create_session(501, Some(1234));
        assert_eq!(session.id, 1);
        assert_eq!(session.identity_id, 600);
        assert_eq!(session.unix_uid, 501);
        assert_eq!(session.unix_pid, Some(1234));
    }

    #[test]
    fn test_create_session_unknown_uid_falls_back_to_guest() {
        let (_dir, trie) = setup_session_trie();
        let manager = SessionManager::load(&trie);

        let session = manager.create_session(9999, None);
        assert_eq!(session.identity_id, 800); // Guest fallback.
    }

    #[test]
    fn test_session_ids_increment() {
        let (_dir, trie) = setup_session_trie();
        let manager = SessionManager::load(&trie);

        let s1 = manager.create_session(501, None);
        let s2 = manager.create_session(501, None);
        let s3 = manager.create_session(0, None);

        assert_eq!(s1.id, 1);
        assert_eq!(s2.id, 2);
        assert_eq!(s3.id, 3);
    }

    #[test]
    fn test_destroy_session() {
        let (_dir, trie) = setup_session_trie();
        let manager = SessionManager::load(&trie);

        let session = manager.create_session(501, None);
        let id = session.id;

        assert!(manager.get_session(id).is_some());
        let removed = manager.destroy_session(id);
        assert!(removed.is_some());
        assert!(manager.get_session(id).is_none());
    }

    #[test]
    fn test_destroy_nonexistent_session() {
        let (_dir, trie) = setup_session_trie();
        let manager = SessionManager::load(&trie);

        assert!(manager.destroy_session(999).is_none());
    }

    #[test]
    fn test_authenticate_session_upgrade() {
        let (_dir, trie) = setup_session_trie();
        let manager = SessionManager::load(&trie);

        // Create session as Guest (unknown UID).
        let session = manager.create_session(9999, None);
        assert_eq!(session.identity_id, 800);

        // Upgrade to identity 600.
        let old = manager.authenticate_session(session.id, 600);
        assert_eq!(old, Some(800));

        // Verify the session now reflects the new identity.
        let updated = manager.get_session(session.id).unwrap();
        assert_eq!(updated.identity_id, 600);
    }

    #[test]
    fn test_authenticate_nonexistent_session() {
        let (_dir, trie) = setup_session_trie();
        let manager = SessionManager::load(&trie);

        assert!(manager.authenticate_session(999, 600).is_none());
    }

    #[test]
    fn test_session_effects_create() {
        let session = Session {
            id: 42,
            identity_id: 600,
            unix_uid: 501,
            unix_pid: Some(1234),
            connected_at: SystemTime::now(),
        };

        let effects = SessionManager::session_effects_create(&session);
        assert_eq!(effects.len(), 7);

        assert_eq!(
            effects[0],
            Effect::MakeDir {
                path: PathBuf::from("sessions/~42")
            }
        );
        assert_eq!(
            effects[2],
            Effect::Touch {
                path: PathBuf::from("sessions/~42/-identity/600")
            }
        );
        assert_eq!(
            effects[4],
            Effect::Touch {
                path: PathBuf::from("sessions/~42/-uid/501")
            }
        );
        assert_eq!(
            effects[6],
            Effect::Touch {
                path: PathBuf::from("sessions/~42/-state/active")
            }
        );
    }

    #[test]
    fn test_session_effects_destroy() {
        let session = Session {
            id: 42,
            identity_id: 600,
            unix_uid: 501,
            unix_pid: None,
            connected_at: SystemTime::now(),
        };

        let effects = SessionManager::session_effects_destroy(&session);
        assert_eq!(effects.len(), 7);

        assert_eq!(
            effects[6],
            Effect::Remove {
                path: PathBuf::from("sessions/~42")
            }
        );
    }

    #[test]
    fn test_verb_for_op() {
        assert_eq!(verb_for_op("ping"), None);
        assert_eq!(verb_for_op("authenticate"), None);
        assert_eq!(verb_for_op("status"), Some(Verb::Read));
        assert_eq!(verb_for_op("ls"), Some(Verb::Read));
        assert_eq!(verb_for_op("query"), Some(Verb::Read));
        assert_eq!(verb_for_op("db_query"), Some(Verb::Read));
        assert_eq!(verb_for_op("touch"), Some(Verb::Write));
        assert_eq!(verb_for_op("mkdir"), Some(Verb::Write));
        assert_eq!(verb_for_op("rm"), Some(Verb::Write));
        assert_eq!(verb_for_op("mv"), Some(Verb::Write));
        assert_eq!(verb_for_op("create_identity"), Some(Verb::Write));
        assert_eq!(verb_for_op("unknown"), Some(Verb::Execute));
    }

    #[test]
    fn test_active_session_ids() {
        let (_dir, trie) = setup_session_trie();
        let manager = SessionManager::load(&trie);

        let s1 = manager.create_session(501, None);
        let s2 = manager.create_session(0, None);

        let mut ids = manager.active_session_ids();
        ids.sort();
        assert_eq!(ids, vec![s1.id, s2.id]);
    }

    #[tokio::test]
    async fn test_clean_sessions() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join("sessions/~1/-identity")).unwrap();
        std::fs::File::create(root.join("sessions/~1/-identity/600")).unwrap();
        std::fs::create_dir_all(root.join("sessions/~2")).unwrap();
        std::fs::File::create(root.join("sessions/.gitkeep")).unwrap();

        clean_sessions(root).await.unwrap();

        assert!(!root.join("sessions/~1").exists());
        assert!(!root.join("sessions/~2").exists());
        assert!(root.join("sessions/.gitkeep").exists());
    }
}
