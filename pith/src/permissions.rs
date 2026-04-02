/// Permission loading and resolution for the 0-bytes OS.
///
/// Permissions are encoded in the filesystem using the `§` logic door.
/// The engine walks the trie under `hard/identities/` and `hard/groups/`
/// to build permission sets, then resolves access checks at runtime.
///
/// Resolution algorithm:
///   1. Collect all rules: direct (from identity) + inherited (from groups)
///   2. Check §deny first — deny ALWAYS wins
///   3. Check §own — own implies all verbs
///   4. Check §{verb} grants — prefix match or wildcard `_`
///   5. Default deny — everything denied unless explicitly granted

use std::collections::HashMap;
use tracing::{debug, info, warn};

use crate::identity::{Identity, PrivilegeTier};
use crate::parser::NodeClass;
use crate::trie::{Trie, TrieNode};

/// Permission verbs — what an identity can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verb {
    Read,
    Write,
    Execute,
    Own,
    Deny,
}

impl Verb {
    /// Parse a verb from the argument of a `§` logic door.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "execute" => Some(Self::Execute),
            "own" => Some(Self::Own),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }

    /// Check if a verb is satisfied by a grant verb.
    /// `Own` grants everything. Otherwise, exact match.
    pub fn satisfied_by(&self, grant: Verb) -> bool {
        grant == Verb::Own || grant == *self
    }
}

/// A single permission rule extracted from the filesystem.
///
/// Example: `§read/databases` → `PermissionRule { verb: Read, target: ["databases"] }`
/// Example: `§write/hard/groups` → `PermissionRule { verb: Write, target: ["hard", "groups"] }`
/// Example: `§read/_` → `PermissionRule { verb: Read, target: ["_"] }` (wildcard = match all)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRule {
    pub verb: Verb,
    /// The target path segments. `["_"]` = wildcard (matches everything).
    pub target: Vec<String>,
}

impl PermissionRule {
    /// Check if this rule matches a given target path.
    ///
    /// Matching logic:
    /// - Wildcard `["_"]`: matches ANY path
    /// - Prefix match: the rule's target is a prefix of the checked path
    /// - Exact match: the rule's target equals the checked path
    pub fn matches(&self, path: &[&str]) -> bool {
        // Wildcard: single segment "_" matches everything.
        if self.target.len() == 1 && self.target[0] == "_" {
            return true;
        }

        // The rule target must be a prefix of (or equal to) the checked path.
        if self.target.len() > path.len() {
            return false;
        }

        self.target
            .iter()
            .zip(path.iter())
            .all(|(rule_seg, path_seg)| rule_seg == "_" || rule_seg == *path_seg)
    }
}

/// All permissions for a single identity (direct + group-inherited).
#[derive(Debug, Clone)]
pub struct IdentityPermissions {
    pub identity: Identity,
    pub rules: Vec<PermissionRule>,
}

/// The permission engine — loaded from the trie at boot.
#[derive(Debug)]
pub struct PermissionEngine {
    /// Permissions per identity id.
    identities: HashMap<u64, IdentityPermissions>,
    /// Group definitions: group_name → list of rules.
    groups: HashMap<String, Vec<PermissionRule>>,
}

impl PermissionEngine {
    /// Load all permissions from the trie.
    ///
    /// Walks:
    /// - `hard/groups/{name}/§{verb}/...` → group permission definitions
    /// - `hard/identities/{id}/-group/{name}` → group memberships
    /// - `hard/identities/{id}/§{verb}/...` → direct permissions
    pub fn load(trie: &Trie) -> Self {
        let groups = Self::load_groups(trie);
        let identities = Self::load_identities(trie, &groups);

        info!(
            "Permissions loaded: {} identities, {} groups",
            identities.len(),
            groups.len()
        );

        Self {
            identities,
            groups,
        }
    }

    /// Load group definitions from `hard/groups/`.
    fn load_groups(trie: &Trie) -> HashMap<String, Vec<PermissionRule>> {
        let mut groups = HashMap::new();

        let groups_node = match trie.get(&["hard", "groups"]) {
            Some(node) => node,
            None => {
                warn!("hard/groups/ not found in trie — no groups loaded");
                return groups;
            }
        };

        for (group_name, group_node) in &groups_node.children {
            let rules = Self::extract_rules(group_node);
            debug!(
                "Group '{}': {} permission rules",
                group_name,
                rules.len()
            );
            groups.insert(group_name.clone(), rules);
        }

        groups
    }

    /// Load identities from `hard/identities/`.
    fn load_identities(
        trie: &Trie,
        groups: &HashMap<String, Vec<PermissionRule>>,
    ) -> HashMap<u64, IdentityPermissions> {
        let mut identities = HashMap::new();

        let ids_node = match trie.get(&["hard", "identities"]) {
            Some(node) => node,
            None => {
                warn!("hard/identities/ not found in trie — no identities loaded");
                return identities;
            }
        };

        for (id_name, id_node) in &ids_node.children {
            // Parse the identity number from the directory name.
            let id: u64 = match id_name.parse() {
                Ok(n) => n,
                Err(_) => {
                    // Skip non-numeric entries (like make.sh).
                    continue;
                }
            };

            // Use from_name to preserve leading zeros for tier detection.
            let mut identity = Identity::from_name(id_name, id);

            // Find group memberships: children that are `-group` instruction nodes.
            // Pattern: {id}/-group/{group_name}
            for (_name, child_node) in &id_node.children {
                if let NodeClass::Instruction { door: '-', ref arg } = child_node.class {
                    if arg == "group" {
                        // Each child of -group/ is a group membership.
                        for group_name in child_node.children.keys() {
                            identity.groups.push(group_name.clone());
                        }
                    }
                }
            }

            // Collect direct permission rules from §* children.
            let direct_rules = Self::extract_rules(id_node);

            // Collect inherited rules from groups.
            let mut all_rules = direct_rules;
            for group_name in &identity.groups {
                if let Some(group_rules) = groups.get(group_name) {
                    all_rules.extend(group_rules.iter().cloned());
                }
            }

            identities.insert(
                id,
                IdentityPermissions {
                    identity,
                    rules: all_rules,
                },
            );
        }

        identities
    }

    /// Extract permission rules from a trie node's children.
    ///
    /// Looks for children that are `§{verb}` instruction nodes (door='§').
    /// For each, walks the subtree to collect all leaf paths as targets.
    fn extract_rules(node: &TrieNode) -> Vec<PermissionRule> {
        let mut rules = Vec::new();

        for (_name, child) in &node.children {
            if let NodeClass::Instruction { door: '§', ref arg } = child.class {
                let verb = match Verb::from_str(arg) {
                    Some(v) => v,
                    None => continue, // Unknown verb, skip.
                };

                // Walk the subtree under §{verb}/ to find all leaf paths.
                let mut leaf_paths = Vec::new();
                Self::collect_leaf_paths(child, &mut Vec::new(), &mut leaf_paths);

                if leaf_paths.is_empty() {
                    // §{verb} with no children = permission on everything under the parent scope.
                    // Treat as wildcard.
                    rules.push(PermissionRule {
                        verb,
                        target: vec!["_".to_string()],
                    });
                } else {
                    for path in leaf_paths {
                        rules.push(PermissionRule { verb, target: path });
                    }
                }
            }
        }

        rules
    }

    /// Recursively collect all leaf paths under a §{verb} node.
    ///
    /// Each leaf path represents a target that the permission applies to.
    /// Example: `§write/hard/groups` and `§write/hard/identities` produce
    /// two paths: `["hard", "groups"]` and `["hard", "identities"]`.
    fn collect_leaf_paths(
        node: &TrieNode,
        current_path: &mut Vec<String>,
        results: &mut Vec<Vec<String>>,
    ) {
        if node.children.is_empty() {
            // Leaf node — the current path IS a target (if we have segments).
            if !current_path.is_empty() {
                results.push(current_path.clone());
            }
            return;
        }

        for (child_name, child_node) in &node.children {
            current_path.push(child_name.clone());

            if child_node.is_file && child_node.children.is_empty() {
                // This is a leaf file — its path is a target.
                results.push(current_path.clone());
            } else {
                // Recurse into directories.
                Self::collect_leaf_paths(child_node, current_path, results);
            }

            current_path.pop();
        }
    }

    /// Check if an identity has permission to perform a verb on a target path.
    ///
    /// Resolution:
    ///   1. If identity not found → default deny
    ///   2. Check all §deny rules — if ANY matches → DENY
    ///   3. Check §own rules — if matches → ALLOW (own implies all verbs)
    ///   4. Check §{verb} rules — if matches → ALLOW
    ///   5. Default → DENY
    pub fn check(&self, identity_id: u64, verb: Verb, target: &[&str]) -> PermissionResult {
        let perms = match self.identities.get(&identity_id) {
            Some(p) => p,
            None => {
                // Unknown identity → check tier-based defaults.
                // Omni (0xx) tier gets implicit read access.
                let tier = PrivilegeTier::from_id(identity_id);
                if tier == PrivilegeTier::Omni {
                    return PermissionResult::Allow {
                        reason: "omni tier — implicit access".to_string(),
                    };
                }
                return PermissionResult::Deny {
                    reason: format!("identity {} not found", identity_id),
                };
            }
        };

        // 1. Check deny rules first — deny ALWAYS wins.
        for rule in &perms.rules {
            if rule.verb == Verb::Deny && rule.matches(target) {
                return PermissionResult::Deny {
                    reason: format!(
                        "explicit §deny matching {}",
                        target.join("/")
                    ),
                };
            }
        }

        // 2. Check own rules — own implies all verbs.
        for rule in &perms.rules {
            if rule.verb == Verb::Own && rule.matches(target) {
                return PermissionResult::Allow {
                    reason: format!(
                        "§own matching {}",
                        target.join("/")
                    ),
                };
            }
        }

        // 3. Check specific verb grants.
        for rule in &perms.rules {
            if rule.verb == verb && rule.matches(target) {
                return PermissionResult::Allow {
                    reason: format!(
                        "§{:?} matching {}",
                        verb,
                        target.join("/")
                    ),
                };
            }
        }

        // 4. Tier-based fallback for high-privilege tiers.
        match perms.identity.tier {
            PrivilegeTier::Omni | PrivilegeTier::Shadow | PrivilegeTier::Superroot => {
                PermissionResult::Allow {
                    reason: format!("tier {:?} — implicit access", perms.identity.tier),
                }
            }
            _ => PermissionResult::Deny {
                reason: format!(
                    "no matching grant for {:?} on {}",
                    verb,
                    target.join("/")
                ),
            },
        }
    }

    /// Get the permissions for a specific identity (if loaded).
    pub fn get_identity(&self, id: u64) -> Option<&IdentityPermissions> {
        self.identities.get(&id)
    }

    /// Get the rules for a specific group (if loaded).
    pub fn get_group(&self, name: &str) -> Option<&Vec<PermissionRule>> {
        self.groups.get(name)
    }

    /// Get the total number of loaded identities.
    pub fn identity_count(&self) -> usize {
        self.identities.len()
    }

    /// Get the total number of loaded groups.
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }
}

/// Result of a permission check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionResult {
    Allow { reason: String },
    Deny { reason: String },
}

impl PermissionResult {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }

    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Deny { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alphabet::Alphabet;
    use tempfile::TempDir;

    /// Build a test filesystem with identities, groups, and permissions.
    fn setup_permission_fs() -> (TempDir, Trie) {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create reserved alphabet.
        let reserved = root.join("hard/reserved");
        std::fs::create_dir_all(&reserved).unwrap();
        for ch in ['$', '-', '!', '#', '§', '~', '€', '_'] {
            std::fs::File::create(reserved.join(ch.to_string())).unwrap();
        }

        // Create group: system (§read/_, §write/_, §execute/_)
        let system_group = root.join("hard/groups/system");
        std::fs::create_dir_all(system_group.join("§read")).unwrap();
        std::fs::File::create(system_group.join("§read/_")).unwrap();
        std::fs::create_dir_all(system_group.join("§write")).unwrap();
        std::fs::File::create(system_group.join("§write/_")).unwrap();
        std::fs::create_dir_all(system_group.join("§execute")).unwrap();
        std::fs::File::create(system_group.join("§execute/_")).unwrap();

        // Create group: developers (§read/databases, §write/jobs, §execute/workers)
        let dev_group = root.join("hard/groups/developers");
        std::fs::create_dir_all(dev_group.join("§read")).unwrap();
        std::fs::File::create(dev_group.join("§read/databases")).unwrap();
        std::fs::create_dir_all(dev_group.join("§write")).unwrap();
        std::fs::File::create(dev_group.join("§write/jobs")).unwrap();
        std::fs::create_dir_all(dev_group.join("§execute")).unwrap();
        std::fs::File::create(dev_group.join("§execute/workers")).unwrap();

        // Create group: guests (§read/databases, §deny/hard)
        let guest_group = root.join("hard/groups/guests");
        std::fs::create_dir_all(guest_group.join("§read")).unwrap();
        std::fs::File::create(guest_group.join("§read/databases")).unwrap();
        std::fs::create_dir_all(guest_group.join("§deny")).unwrap();
        std::fs::File::create(guest_group.join("§deny/hard")).unwrap();

        // Create identity 001 (omni tier) — in system group
        let id001 = root.join("hard/identities/001");
        std::fs::create_dir_all(id001.join("-expected/type")).unwrap();
        std::fs::File::create(id001.join("-expected/type/identity")).unwrap();
        std::fs::create_dir_all(id001.join("-group")).unwrap();
        std::fs::File::create(id001.join("-group/system")).unwrap();

        // Create identity 600 (user tier) — in developers group
        let id600 = root.join("hard/identities/600");
        std::fs::create_dir_all(id600.join("-expected/type")).unwrap();
        std::fs::File::create(id600.join("-expected/type/identity")).unwrap();
        std::fs::create_dir_all(id600.join("-group")).unwrap();
        std::fs::File::create(id600.join("-group/developers")).unwrap();

        // Create identity 601 (user tier) — in developers group + direct §own/databases/translations
        let id601 = root.join("hard/identities/601");
        std::fs::create_dir_all(id601.join("-expected/type")).unwrap();
        std::fs::File::create(id601.join("-expected/type/identity")).unwrap();
        std::fs::create_dir_all(id601.join("-group")).unwrap();
        std::fs::File::create(id601.join("-group/developers")).unwrap();
        std::fs::create_dir_all(id601.join("§own/databases")).unwrap();
        std::fs::File::create(id601.join("§own/databases/translations")).unwrap();

        // Create identity 800 (guest tier) — in guests group
        let id800 = root.join("hard/identities/800");
        std::fs::create_dir_all(id800.join("-expected/type")).unwrap();
        std::fs::File::create(id800.join("-expected/type/identity")).unwrap();
        std::fs::create_dir_all(id800.join("-group")).unwrap();
        std::fs::File::create(id800.join("-group/guests")).unwrap();

        // Create identity 700 (shared tier) — no group, no permissions
        let id700 = root.join("hard/identities/700");
        std::fs::create_dir_all(id700.join("-expected/type")).unwrap();
        std::fs::File::create(id700.join("-expected/type/identity")).unwrap();

        // Build trie.
        let alphabet = Alphabet::load(&reserved).unwrap();
        let trie = Trie::build(root, &alphabet).unwrap();

        (dir, trie)
    }

    #[test]
    fn test_load_groups() {
        let (_dir, trie) = setup_permission_fs();
        let engine = PermissionEngine::load(&trie);

        assert_eq!(engine.group_count(), 3); // system, developers, guests

        let system_rules = engine.get_group("system").unwrap();
        assert_eq!(system_rules.len(), 3); // read/_, write/_, execute/_

        let dev_rules = engine.get_group("developers").unwrap();
        assert_eq!(dev_rules.len(), 3); // read/databases, write/jobs, execute/workers

        let guest_rules = engine.get_group("guests").unwrap();
        assert_eq!(guest_rules.len(), 2); // read/databases, deny/hard
    }

    #[test]
    fn test_load_identities() {
        let (_dir, trie) = setup_permission_fs();
        let engine = PermissionEngine::load(&trie);

        // Identity 001 should be in system group.
        let id001 = engine.get_identity(1).unwrap();
        assert_eq!(id001.identity.groups, vec!["system"]);
        assert_eq!(id001.identity.tier, PrivilegeTier::Omni);
        // Has 3 rules inherited from system group (read/_, write/_, execute/_).
        assert_eq!(id001.rules.len(), 3);

        // Identity 600 should be in developers group.
        let id600 = engine.get_identity(600).unwrap();
        assert_eq!(id600.identity.groups, vec!["developers"]);
        assert_eq!(id600.identity.tier, PrivilegeTier::User);
        // Has 3 rules inherited from developers.
        assert_eq!(id600.rules.len(), 3);

        // Identity 601 should have direct §own + group rules.
        let id601 = engine.get_identity(601).unwrap();
        assert!(id601.rules.len() >= 4); // 1 direct own + 3 from developers
    }

    #[test]
    fn test_system_group_allows_everything() {
        let (_dir, trie) = setup_permission_fs();
        let engine = PermissionEngine::load(&trie);

        // Identity 001 (system group, wildcard _) can read anything.
        assert!(engine
            .check(1, Verb::Read, &["databases", "colors", "blue"])
            .is_allowed());
        assert!(engine
            .check(1, Verb::Write, &["jobs", "1"])
            .is_allowed());
        assert!(engine
            .check(1, Verb::Execute, &["workers", "1"])
            .is_allowed());
    }

    #[test]
    fn test_developer_scoped_permissions() {
        let (_dir, trie) = setup_permission_fs();
        let engine = PermissionEngine::load(&trie);

        // Identity 600 (developers) CAN read databases.
        assert!(engine
            .check(600, Verb::Read, &["databases", "colors", "blue"])
            .is_allowed());

        // Identity 600 CAN write to jobs.
        assert!(engine
            .check(600, Verb::Write, &["jobs", "1"])
            .is_allowed());

        // Identity 600 CAN execute workers.
        assert!(engine
            .check(600, Verb::Execute, &["workers", "1"])
            .is_allowed());

        // Identity 600 CANNOT write to hard/ (no grant).
        assert!(engine
            .check(600, Verb::Write, &["hard", "reserved"])
            .is_denied());

        // Identity 600 CANNOT read events/ (no grant).
        assert!(engine
            .check(600, Verb::Read, &["events", "!boot"])
            .is_denied());
    }

    #[test]
    fn test_deny_overrides_grant() {
        let (_dir, trie) = setup_permission_fs();
        let engine = PermissionEngine::load(&trie);

        // Identity 800 (guests) has §read/databases AND §deny/hard.
        // Can read databases.
        assert!(engine
            .check(800, Verb::Read, &["databases", "colors"])
            .is_allowed());

        // CANNOT read hard/ — deny overrides any potential grant.
        assert!(engine
            .check(800, Verb::Read, &["hard", "identities"])
            .is_denied());

        // CANNOT write anything (no write grant).
        assert!(engine
            .check(800, Verb::Write, &["jobs", "1"])
            .is_denied());
    }

    #[test]
    fn test_own_implies_all_verbs() {
        let (_dir, trie) = setup_permission_fs();
        let engine = PermissionEngine::load(&trie);

        // Identity 601 has §own/databases/translations.
        // Own implies read, write, execute on that subtree.
        assert!(engine
            .check(601, Verb::Read, &["databases", "translations", "english"])
            .is_allowed());
        assert!(engine
            .check(601, Verb::Write, &["databases", "translations", "french"])
            .is_allowed());
        assert!(engine
            .check(601, Verb::Execute, &["databases", "translations"])
            .is_allowed());
    }

    #[test]
    fn test_default_deny() {
        let (_dir, trie) = setup_permission_fs();
        let engine = PermissionEngine::load(&trie);

        // Identity 700 has NO groups and NO direct permissions.
        // Everything should be denied.
        assert!(engine
            .check(700, Verb::Read, &["databases"])
            .is_denied());
        assert!(engine
            .check(700, Verb::Write, &["jobs"])
            .is_denied());
    }

    #[test]
    fn test_unknown_identity_denied() {
        let (_dir, trie) = setup_permission_fs();
        let engine = PermissionEngine::load(&trie);

        // Identity 999 doesn't exist in the filesystem.
        assert!(engine
            .check(999, Verb::Read, &["databases"])
            .is_denied());
    }

    #[test]
    fn test_prefix_matching() {
        let (_dir, trie) = setup_permission_fs();
        let engine = PermissionEngine::load(&trie);

        // Identity 600 has §read/databases (prefix match).
        // Should match databases/anything/deep/path.
        assert!(engine
            .check(600, Verb::Read, &["databases", "colors", "blue", "psychology"])
            .is_allowed());

        // Should NOT match something that doesn't start with databases.
        assert!(engine
            .check(600, Verb::Read, &["pointers", "unicodes"])
            .is_denied());
    }

    #[test]
    fn test_rule_matches_wildcard() {
        let rule = PermissionRule {
            verb: Verb::Read,
            target: vec!["_".to_string()],
        };
        assert!(rule.matches(&["anything"]));
        assert!(rule.matches(&["deep", "nested", "path"]));
        assert!(rule.matches(&[])); // wildcard even matches empty
    }

    #[test]
    fn test_rule_matches_prefix() {
        let rule = PermissionRule {
            verb: Verb::Write,
            target: vec!["jobs".to_string()],
        };
        assert!(rule.matches(&["jobs"]));
        assert!(rule.matches(&["jobs", "1"]));
        assert!(rule.matches(&["jobs", "1", "-state", "pending"]));
        assert!(!rule.matches(&["workers"]));
        assert!(!rule.matches(&[]));
    }

    #[test]
    fn test_rule_matches_deep_prefix() {
        let rule = PermissionRule {
            verb: Verb::Write,
            target: vec!["hard".to_string(), "groups".to_string()],
        };
        assert!(rule.matches(&["hard", "groups"]));
        assert!(rule.matches(&["hard", "groups", "admin"]));
        assert!(!rule.matches(&["hard", "identities"]));
        assert!(!rule.matches(&["hard"]));
    }
}
