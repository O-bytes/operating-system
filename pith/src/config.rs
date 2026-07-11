use std::path::PathBuf;

/// Configuration for the Pith engine.
#[derive(Debug, Clone)]
pub struct PithConfig {
    /// Root path of the 0-bytes filesystem (the `src/` directory).
    pub fs_root: PathBuf,

    /// Path for the Unix domain socket API.
    pub socket_path: PathBuf,

    /// Log level filter (e.g., "info", "debug", "trace").
    pub log_level: String,

    /// Whether to enforce permissions on raw filesystem changes.
    /// Phase 1: false (raw FS = debug mode).
    /// Phase 2: true (detect unauthorized changes and revert).
    pub enforce_permissions: bool,
}

impl Default for PithConfig {
    fn default() -> Self {
        Self {
            fs_root: PathBuf::from("../src"),
            socket_path: PathBuf::from("/tmp/pith.sock"),
            log_level: "info".to_string(),
            enforce_permissions: false,
        }
    }
}
