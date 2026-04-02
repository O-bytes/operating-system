/// Wire protocol for the Pith API.
///
/// Newline-delimited JSON over Unix domain socket.
/// Debuggable with: echo '{"op":"ls","path":"hard/types"}' | socat - UNIX:/tmp/pith.sock

use serde::{Deserialize, Serialize};

/// A request from a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Operation: touch, rm, mv, mkdir, ls, query, status, ping
    pub op: String,
    /// Target path (relative to fs_root).
    #[serde(default)]
    pub path: String,
    /// Optional arguments (e.g., "to" path for mv, mtime for touch-mtime).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
}

/// A response to a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn success(data: Option<serde_json::Value>) -> Self {
        Self {
            ok: true,
            data,
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}
