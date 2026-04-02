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
use crate::effector::{Effect, Effector};
use crate::trie::Trie;

/// Handle a single API request.
pub async fn handle_request(
    request: &Request,
    trie: &Arc<RwLock<Trie>>,
    effector: &Effector,
) -> Response {
    info!("API: {} {}", request.op, request.path);

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

    #[tokio::test]
    async fn test_ping() {
        let trie = make_test_trie();
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());

        let req = Request {
            op: "ping".to_string(),
            path: String::new(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector).await;
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap(), serde_json::json!("pong"));
    }

    #[tokio::test]
    async fn test_ls() {
        let trie = make_test_trie();
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());

        let req = Request {
            op: "ls".to_string(),
            path: "hard/types".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector).await;
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

        let req = Request {
            op: "ls".to_string(),
            path: "".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector).await;
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

        let req = Request {
            op: "ls".to_string(),
            path: "nonexistent".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector).await;
        assert!(!resp.ok);
    }

    #[tokio::test]
    async fn test_touch_and_rm() {
        let trie = make_test_trie();
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());

        // Touch.
        let req = Request {
            op: "touch".to_string(),
            path: "events/!test".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector).await;
        assert!(resp.ok);
        assert!(dir.path().join("events/!test").exists());

        // Rm.
        let req = Request {
            op: "rm".to_string(),
            path: "events/!test".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector).await;
        assert!(resp.ok);
        assert!(!dir.path().join("events/!test").exists());
    }

    #[tokio::test]
    async fn test_query() {
        let trie = make_test_trie();
        let dir = TempDir::new().unwrap();
        let effector = Effector::new(dir.path().to_path_buf());

        let req = Request {
            op: "query".to_string(),
            path: "hard/types".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector).await;
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

        let req = Request {
            op: "destroy_everything".to_string(),
            path: "".to_string(),
            args: None,
        };
        let resp = handle_request(&req, &trie, &effector).await;
        assert!(!resp.ok);
        assert!(resp.error.unwrap().contains("Unknown operation"));
    }
}
