/// API command handlers.
///
/// Each handler:
///   1. Validates the request
///   2. Reads from the trie (for queries) or produces effects (for mutations)
///   3. Returns a Response

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tracing::info;

use crate::api::protocol::{Request, Response};
use crate::auth;
use crate::effector::{Effect, Effector};
use crate::identity::PrivilegeTier;
use crate::session::SessionContext;
use crate::trie::Trie;

/// Handle a single API request.
pub async fn handle_request(
    request: &Request,
    trie: &Arc<RwLock<Trie>>,
    effector: &Effector,
    ctx: &SessionContext,
) -> Response {
    let identity_id = ctx.identity_id();
    info!("API [identity={}]: {} {}", identity_id, request.op, request.path);

    match request.op.as_str() {
        "ping" => Response::success(Some(serde_json::json!("pong"))),

        "status" => handle_status(trie),

        "ls" => handle_ls(request, trie),

        "query" => handle_query(request, trie),

        "touch" => handle_touch(request, effector).await,

        "mkdir" => handle_mkdir(request, effector).await,

        "rm" => handle_rm(request, effector).await,

        "mv" => handle_mv(request, effector).await,

        "db_query" => handle_db_query(request, trie),

        "authenticate" => handle_authenticate(request, trie, ctx),

        "create_identity" => handle_create_identity(request, trie, effector, ctx).await,

        _ => Response::error(format!("Unknown operation: {}", request.op)),
    }
}

/// Status: return engine info.
fn handle_status(trie: &Arc<RwLock<Trie>>) -> Response {
    let trie = trie.read().unwrap();
    let node_count = trie.total_nodes();
    Response::success(Some(serde_json::json!({
        "status": "running",
        "nodes": node_count,
    })))
}

/// List children of a path (reads from trie, no filesystem hit).
fn handle_ls(request: &Request, trie: &Arc<RwLock<Trie>>) -> Response {
    let trie = trie.read().unwrap();

    let segments: Vec<&str> = if request.path.is_empty() {
        vec![]
    } else {
        request.path.split('/').filter(|s| !s.is_empty()).collect()
    };

    match trie.list(&segments) {
        Some(children) => {
            let list: Vec<serde_json::Value> = children
                .iter()
                .map(|c| serde_json::json!(c))
                .collect();
            Response::success(Some(serde_json::json!(list)))
        }
        None => Response::error(format!("Path not found: {}", request.path)),
    }
}

/// Query: return full subtree info for a path.
fn handle_query(request: &Request, trie: &Arc<RwLock<Trie>>) -> Response {
    let trie = trie.read().unwrap();

    let segments: Vec<&str> = if request.path.is_empty() {
        vec![]
    } else {
        request.path.split('/').filter(|s| !s.is_empty()).collect()
    };

    match trie.get(&segments) {
        Some(node) => {
            let children: Vec<serde_json::Value> = node
                .children
                .iter()
                .map(|(name, child)| {
                    serde_json::json!({
                        "name": name,
                        "is_file": child.is_file,
                        "children_count": child.children.len(),
                    })
                })
                .collect();

            Response::success(Some(serde_json::json!({
                "path": request.path,
                "is_file": node.is_file,
                "children": children,
            })))
        }
        None => Response::error(format!("Path not found: {}", request.path)),
    }
}

/// Touch: create a zero-byte file.
async fn handle_touch(request: &Request, effector: &Effector) -> Response {
    if request.path.is_empty() {
        return Response::error("Path required for touch");
    }

    match effector
        .execute(&Effect::Touch {
            path: PathBuf::from(&request.path),
        })
        .await
    {
        Ok(()) => Response::success(None),
        Err(e) => Response::error(format!("touch failed: {}", e)),
    }
}

/// Mkdir: create a directory.
async fn handle_mkdir(request: &Request, effector: &Effector) -> Response {
    if request.path.is_empty() {
        return Response::error("Path required for mkdir");
    }

    match effector
        .execute(&Effect::MakeDir {
            path: PathBuf::from(&request.path),
        })
        .await
    {
        Ok(()) => Response::success(None),
        Err(e) => Response::error(format!("mkdir failed: {}", e)),
    }
}

/// Rm: delete a file or empty directory.
async fn handle_rm(request: &Request, effector: &Effector) -> Response {
    if request.path.is_empty() {
        return Response::error("Path required for rm");
    }

    match effector
        .execute(&Effect::Remove {
            path: PathBuf::from(&request.path),
        })
        .await
    {
        Ok(()) => Response::success(None),
        Err(e) => Response::error(format!("rm failed: {}", e)),
    }
}

/// Mv: rename/move a file or directory.
async fn handle_mv(request: &Request, effector: &Effector) -> Response {
    if request.path.is_empty() {
        return Response::error("Path required for mv (source)");
    }

    let to = match &request.args {
        Some(args) => match args.get("to").and_then(|v| v.as_str()) {
            Some(to) => to.to_string(),
            None => return Response::error("args.to required for mv"),
        },
        None => return Response::error("args.to required for mv"),
    };

    match effector
        .execute(&Effect::Move {
            from: PathBuf::from(&request.path),
            to: PathBuf::from(&to),
        })
        .await
    {
        Ok(()) => Response::success(None),
        Err(e) => Response::error(format!("mv failed: {}", e)),
    }
}

/// Database query: return set members under a database path.
fn handle_db_query(request: &Request, trie: &Arc<RwLock<Trie>>) -> Response {
    let trie = trie.read().unwrap();

    let segments: Vec<&str> = if request.path.is_empty() {
        vec![]
    } else {
        request.path.split('/').filter(|s| !s.is_empty()).collect()
    };

    let results = crate::subsystems::databases::query_set(&trie, &segments);
    Response::success(Some(serde_json::json!(results)))
}

/// Authenticate: verify identity + password, upgrade session.
///
/// Request: `{"op": "authenticate", "args": {"identity": "001", "password": "..."}}`
/// Response: `{"ok": true, "data": {"identity_id": 1, "tier": "Omni"}}`
fn handle_authenticate(
    request: &Request,
    trie: &Arc<RwLock<Trie>>,
    ctx: &SessionContext,
) -> Response {
    let args = match &request.args {
        Some(a) => a,
        None => return Response::error("args required for authenticate"),
    };

    let identity_name = match args.get("identity").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return Response::error("args.identity required"),
    };

    let password = match args.get("password").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return Response::error("args.password required"),
    };

    // Parse identity ID.
    let identity_id: u64 = match identity_name.parse() {
        Ok(n) => n,
        Err(_) => return Response::error("invalid identity id (must be numeric)"),
    };

    // Look up the identity's -secret in the trie.
    let trie_guard = trie.read().unwrap();
    let secret_node = match trie_guard.get(&["hard", "identities", identity_name]) {
        Some(id_node) => {
            // Find the -secret instruction child.
            let mut found = None;
            for (_name, child) in &id_node.children {
                if let crate::parser::NodeClass::Instruction { door: '-', ref arg } = child.class {
                    if arg == "secret" && !child.children.is_empty() {
                        found = Some(child);
                        break;
                    }
                }
            }
            match found {
                Some(node) => node,
                None => return Response::error("identity has no password set"),
            }
        }
        None => return Response::error(format!("identity {} not found", identity_name)),
    };

    // The hash is the filename of the first child of -secret.
    let hash_filename = match secret_node.children.keys().next() {
        Some(f) => f.clone(),
        None => return Response::error("identity has no password set"),
    };

    // Release trie lock before CPU-intensive password verification.
    drop(trie_guard);

    let phc_hash = auth::filename_to_hash(&hash_filename);

    match auth::verify_password(password, &phc_hash) {
        Ok(true) => {
            // Upgrade session identity.
            ctx.session_manager.authenticate_session(ctx.session_id, identity_id);

            let tier = PrivilegeTier::from_name(identity_name);
            Response::success(Some(serde_json::json!({
                "identity_id": identity_id,
                "tier": format!("{:?}", tier),
            })))
        }
        Ok(false) => Response::error("invalid password"),
        Err(e) => Response::error(format!("authentication error: {}", e)),
    }
}

/// Create a new identity (Admin tier or higher required).
///
/// Request: `{"op": "create_identity", "args": {
///     "id": "601",
///     "password": "...",         // optional
///     "name": "alice",           // optional
///     "groups": ["developers"],  // optional
///     "uid": 502                 // optional — Unix UID mapping
/// }}`
async fn handle_create_identity(
    request: &Request,
    trie: &Arc<RwLock<Trie>>,
    effector: &Effector,
    ctx: &SessionContext,
) -> Response {
    // Check caller's privilege tier: must be Admin (4xx) or higher.
    let caller_id = ctx.identity_id();
    let caller_tier = PrivilegeTier::from_id(caller_id);
    if caller_tier > PrivilegeTier::Admin {
        // PrivilegeTier is Ord: Omni < Shadow < ... < Admin < Permissioned < ...
        // Greater means less privileged.
        return Response::error(format!(
            "create_identity requires Admin tier or higher (caller is {:?})",
            caller_tier
        ));
    }

    let args = match &request.args {
        Some(a) => a,
        None => return Response::error("args required for create_identity"),
    };

    let id_name = match args.get("id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return Response::error("args.id required (e.g. \"601\")"),
    };

    // Validate that id is numeric.
    if id_name.parse::<u64>().is_err() {
        return Response::error("args.id must be numeric");
    }

    // Check identity doesn't already exist.
    {
        let trie_guard = trie.read().unwrap();
        if trie_guard
            .get(&["hard", "identities", id_name])
            .is_some()
        {
            return Response::error(format!("identity {} already exists", id_name));
        }
    }

    let base = format!("hard/identities/{}", id_name);

    // Build effects for the identity filesystem structure.
    let mut effects: Vec<Effect> = vec![
        Effect::MakeDir {
            path: PathBuf::from(&base),
        },
        Effect::MakeDir {
            path: PathBuf::from(format!("{base}/-expected/type")),
        },
        Effect::Touch {
            path: PathBuf::from(format!("{base}/-expected/type/identity")),
        },
    ];

    // Optional: name
    if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
        effects.push(Effect::MakeDir {
            path: PathBuf::from(format!("{base}/-name")),
        });
        effects.push(Effect::Touch {
            path: PathBuf::from(format!("{base}/-name/{name}")),
        });
    }

    // Optional: groups
    if let Some(groups) = args.get("groups").and_then(|v| v.as_array()) {
        effects.push(Effect::MakeDir {
            path: PathBuf::from(format!("{base}/-group")),
        });
        for group in groups {
            if let Some(g) = group.as_str() {
                effects.push(Effect::Touch {
                    path: PathBuf::from(format!("{base}/-group/{g}")),
                });
            }
        }
    }

    // Optional: password
    if let Some(password) = args.get("password").and_then(|v| v.as_str()) {
        match auth::hash_password(password) {
            Ok(phc_hash) => {
                let filename = auth::hash_to_filename(&phc_hash);
                effects.push(Effect::MakeDir {
                    path: PathBuf::from(format!("{base}/-secret")),
                });
                effects.push(Effect::Touch {
                    path: PathBuf::from(format!("{base}/-secret/{filename}")),
                });
            }
            Err(e) => return Response::error(format!("password hash failed: {}", e)),
        }
    }

    // Optional: UID mapping
    if let Some(uid) = args.get("uid").and_then(|v| v.as_u64()) {
        effects.push(Effect::MakeDir {
            path: PathBuf::from(format!("{base}/-uid")),
        });
        effects.push(Effect::Touch {
            path: PathBuf::from(format!("{base}/-uid/{uid}")),
        });
    }

    // Execute all effects.
    for effect in &effects {
        if let Err(e) = effector.execute(effect).await {
            return Response::error(format!("create_identity failed: {}", e));
        }
    }

    info!("Identity {} created by identity {}", id_name, caller_id);

    Response::success(Some(serde_json::json!({
        "created": id_name,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alphabet::Alphabet;
    use crate::parser::NodeClass;
    use crate::trie::{Trie, TrieNode};
    use tempfile::TempDir;

    fn make_test_trie() -> Arc<RwLock<Trie>> {
        let dir = TempDir::new().unwrap();
        let reserved = dir.path().join("reserved");
        std::fs::create_dir_all(&reserved).unwrap();
        std::fs::File::create(reserved.join("€")).unwrap();
        std::fs::File::create(reserved.join("-")).unwrap();
        let alphabet = Alphabet::load(&reserved).unwrap();

        let mut trie = Trie {
            root: TrieNode::new(String::new(), NodeClass::Data(String::new()), false),
        };
        trie.insert(&["hard".into(), "types".into(), "job".into()], true, &alphabet);
        trie.insert(&["hard".into(), "types".into(), "worker".into()], true, &alphabet);
        trie.insert(&["jobs".into(), "0".into()], false, &alphabet);

        Arc::new(RwLock::new(trie))
    }

    fn make_test_ctx() -> SessionContext {
        // Minimal context for tests — no real session manager needed for basic handler tests.
        use crate::permissions::PermissionEngine;
        use crate::session::SessionManager;

        // Create a minimal trie for permissions/sessions.
        let dir = TempDir::new().unwrap();
        let reserved = dir.path().join("hard/reserved");
        std::fs::create_dir_all(&reserved).unwrap();
        std::fs::File::create(reserved.join("€")).unwrap();
        std::fs::File::create(reserved.join("-")).unwrap();
        let alphabet = Alphabet::load(&reserved).unwrap();
        let trie = Trie::build(dir.path(), &alphabet).unwrap();

        let permissions = Arc::new(PermissionEngine::load(&trie));
        let session_manager = Arc::new(SessionManager::load(&trie));

        // Create a session with identity 0 (Omni — always allowed).
        let session = session_manager.create_session(0, None);

        SessionContext {
            session_id: session.id,
            session_manager,
            permissions,
            enforce: false,
        }
    }

    #[tokio::test]
    async fn test_ping() {
        let trie = make_test_trie();
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());
        let ctx = make_test_ctx();

        let req = Request {
            op: "ping".to_string(),
            path: String::new(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector, &ctx).await;
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap(), serde_json::json!("pong"));
    }

    #[tokio::test]
    async fn test_ls() {
        let trie = make_test_trie();
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());
        let ctx = make_test_ctx();

        let req = Request {
            op: "ls".to_string(),
            path: "hard/types".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector, &ctx).await;
        assert!(resp.ok);

        let data = resp.data.unwrap();
        let list: Vec<String> = serde_json::from_value(data).unwrap();
        assert!(list.contains(&"job".to_string()));
        assert!(list.contains(&"worker".to_string()));
    }

    #[tokio::test]
    async fn test_ls_root() {
        let trie = make_test_trie();
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());
        let ctx = make_test_ctx();

        let req = Request {
            op: "ls".to_string(),
            path: "".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector, &ctx).await;
        assert!(resp.ok);

        let data = resp.data.unwrap();
        let list: Vec<String> = serde_json::from_value(data).unwrap();
        assert!(list.contains(&"hard".to_string()));
        assert!(list.contains(&"jobs".to_string()));
    }

    #[tokio::test]
    async fn test_ls_not_found() {
        let trie = make_test_trie();
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());
        let ctx = make_test_ctx();

        let req = Request {
            op: "ls".to_string(),
            path: "nonexistent".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector, &ctx).await;
        assert!(!resp.ok);
    }

    #[tokio::test]
    async fn test_touch_and_rm() {
        let trie = make_test_trie();
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());
        let ctx = make_test_ctx();

        // Touch.
        let req = Request {
            op: "touch".to_string(),
            path: "events/!test".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector, &ctx).await;
        assert!(resp.ok);
        assert!(dir.path().join("events/!test").exists());

        // Rm.
        let req = Request {
            op: "rm".to_string(),
            path: "events/!test".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector, &ctx).await;
        assert!(resp.ok);
        assert!(!dir.path().join("events/!test").exists());
    }

    #[tokio::test]
    async fn test_query() {
        let trie = make_test_trie();
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());
        let ctx = make_test_ctx();

        let req = Request {
            op: "query".to_string(),
            path: "hard/types".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector, &ctx).await;
        assert!(resp.ok);

        let data = resp.data.unwrap();
        let children = data["children"].as_array().unwrap();
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn test_unknown_op() {
        let trie = make_test_trie();
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());
        let ctx = make_test_ctx();

        let req = Request {
            op: "destroy_everything".to_string(),
            path: "".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector, &ctx).await;
        assert!(!resp.ok);
        assert!(resp.error.unwrap().contains("Unknown operation"));
    }
}
