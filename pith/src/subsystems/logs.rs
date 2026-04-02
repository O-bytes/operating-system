/// Logs subsystem — handles `logs/` scope.
///
/// Provides a logging facility: any subsystem can request a log entry
/// by producing an Effect::Touch on `logs/{timestamp}/(message)`.
///
/// The logs subsystem itself watches for new log entries and can
/// enforce rotation (keeping only the last N entries).

use tracing::debug;

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};

pub struct LogsSubsystem;

impl LogsSubsystem {
    pub fn new() -> Self {
        Self
    }

    /// Create a log effect that other subsystems can include in their effects.
    ///
    /// This is a helper for producing log entries from any subsystem.
    pub fn log_effect(message: &str) -> Effect {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f");
        Effect::Touch {
            path: std::path::PathBuf::from(format!("logs/{}_({})", timestamp, message)),
        }
    }
}

impl Subsystem for LogsSubsystem {
    fn scope(&self) -> Scope {
        Scope::Logs
    }

    fn handle(&self, event: &FsEvent) -> Vec<Effect> {
        match event.kind {
            FsEventKind::Assert => {
                if event.segments.len() >= 2 {
                    debug!("Log entry: {}", event.segments[1..].join("/"));
                }
            }
            _ => {}
        }

        // TODO: implement log rotation when entry count exceeds MAX_LOG_ENTRIES
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_effect_format() {
        let effect = LogsSubsystem::log_effect("boot completed");
        match effect {
            Effect::Touch { path } => {
                let s = path.to_string_lossy();
                assert!(s.starts_with("logs/"));
                assert!(s.contains("(boot completed)"));
            }
            _ => panic!("Expected Touch"),
        }
    }

    #[test]
    fn test_handle_assert_no_effects() {
        let sub = LogsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["logs".to_string(), "20260402_(test)".to_string()],
            scope: Scope::Logs,
        };
        // Log subsystem is passive for now.
        assert!(sub.handle(&event).is_empty());
    }
}
