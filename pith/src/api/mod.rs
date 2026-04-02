/// Unix socket API server for Pith.
///
/// Protocol: newline-delimited JSON over Unix domain socket.
///
/// Usage:
///   echo '{"op":"ping"}' | socat - UNIX:/tmp/pith.sock
///   echo '{"op":"ls","path":"hard/types"}' | socat - UNIX:/tmp/pith.sock
///   echo '{"op":"touch","path":"events/!test"}' | socat - UNIX:/tmp/pith.sock

pub mod handlers;
pub mod protocol;

use std::path::Path;
use std::sync::{Arc, RwLock};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{debug, error, info, warn};

use crate::effector::Effector;
use crate::trie::Trie;

use self::handlers::handle_request;
use self::protocol::{Request, Response};

/// Start the API server on the given Unix socket path.
///
/// Returns a JoinHandle that runs until the listener is dropped.
pub async fn start_server(
    socket_path: &Path,
    trie: Arc<RwLock<Trie>>,
    effector: Effector,
) -> crate::error::Result<tokio::task::JoinHandle<()>> {
    // Remove stale socket if it exists.
    if socket_path.exists() {
        std::fs::remove_file(socket_path).map_err(|e| crate::error::PithError::Api {
            reason: format!("Cannot remove stale socket {}: {}", socket_path.display(), e),
        })?;
    }

    // Ensure parent directory exists.
    if let Some(parent) = socket_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| crate::error::PithError::Api {
                reason: format!("Cannot create socket directory: {}", e),
            })?;
        }
    }

    let listener = UnixListener::bind(socket_path).map_err(|e| crate::error::PithError::Api {
        reason: format!("Cannot bind to {}: {}", socket_path.display(), e),
    })?;

    info!("API server listening on {}", socket_path.display());

    let handle = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let trie = Arc::clone(&trie);
                    let effector = effector.clone();
                    tokio::spawn(async move {
                        handle_connection(stream, trie, effector).await;
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    });

    Ok(handle)
}

/// Handle a single client connection.
///
/// Reads newline-delimited JSON requests, processes each, writes JSON responses.
async fn handle_connection(
    stream: tokio::net::UnixStream,
    trie: Arc<RwLock<Trie>>,
    effector: Effector,
) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    debug!("Client connected");

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                debug!("Client disconnected");
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Parse the request.
                let response = match serde_json::from_str::<Request>(trimmed) {
                    Ok(request) => handle_request(&request, &trie, &effector).await,
                    Err(e) => Response::error(format!("Invalid JSON: {}", e)),
                };

                // Serialize and send the response.
                match serde_json::to_string(&response) {
                    Ok(json) => {
                        let msg = format!("{}\n", json);
                        if let Err(e) = writer.write_all(msg.as_bytes()).await {
                            warn!("Failed to write response: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to serialize response: {}", e);
                        break;
                    }
                }
            }
            Err(e) => {
                warn!("Read error: {}", e);
                break;
            }
        }
    }
}

/// Remove the socket file on shutdown.
pub fn cleanup_socket(socket_path: &Path) {
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
        info!("Removed socket: {}", socket_path.display());
    }
}
