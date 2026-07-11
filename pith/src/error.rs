use std::path::PathBuf;

/// Unified error type for the Pith engine.
///
/// Philosophy: errors at the subsystem level should NOT crash the engine.
/// A failed job or malformed path is logged and reported, not a panic.
/// `panic!` only on unrecoverable corruption (e.g., `hard/reserved/` missing).
#[derive(Debug, thiserror::Error)]
pub enum PithError {
    #[error("IO error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Parse error in segment '{segment}': {reason}")]
    Parse { segment: String, reason: String },

    #[error("Permission denied: identity {identity} cannot {verb} at {path}")]
    Permission {
        identity: u64,
        verb: String,
        path: PathBuf,
    },

    #[error("Path not found: {path}")]
    NotFound { path: PathBuf },

    #[error("Invalid path: {path} — {reason}")]
    InvalidPath { path: PathBuf, reason: String },

    #[error("Boot failure: {reason}")]
    Boot { reason: String },

    #[error("Subsystem error in {subsystem}: {reason}")]
    Subsystem { subsystem: String, reason: String },

    #[error("Watcher error: {reason}")]
    Watcher { reason: String },

    #[error("API error: {reason}")]
    Api { reason: String },

    #[error("Session error: {reason}")]
    Session { reason: String },

    #[error("Authentication error: {reason}")]
    Auth { reason: String },
}

pub type Result<T> = std::result::Result<T, PithError>;
