/// Events subsystem — handles `events/` scope.
///
/// Signals are fire-and-forget: `!signal_name` files.
/// When a `!` signal is asserted, the subsystem records it in `-history/`.

use std::path::PathBuf;

use tracing::info;

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};

pub struct EventsSubsystem;

impl EventsSubsystem {
    pub fn new() -> Self {
        Self
    }
}

impl Subsystem for EventsSubsystem {
    fn scope(&self) -> Scope {
        Scope::Events
    }

    fn handle(&self, event: &FsEvent) -> Vec<Effect> {
        let mut effects = Vec::new();

        if event.segments.len() < 2 {
            return effects;
        }

        let signal_name = &event.segments[event.segments.len() - 1];

        // Ignore writes to -history/ to avoid infinite loops.
        if event.segments.iter().any(|s| s == "-history") {
            return effects;
        }

        match event.kind {
            FsEventKind::Assert => {
                info!("Signal emitted: {}", signal_name);

                // Record in history with timestamp.
                let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f");
                let history_name = format!("{}_{}", timestamp, signal_name);
                effects.push(Effect::Touch {
                    path: PathBuf::from(format!("events/-history/{}", history_name)),
                });

                // TODO: notify subscribers from subscriptions/
            }
            FsEventKind::Retract => {
                info!("Signal retracted: {}", signal_name);
            }
            _ => {}
        }

        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_creates_history() {
        let sub = EventsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["events".to_string(), "!test_signal".to_string()],
            scope: Scope::Events,
        };

        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            Effect::Touch { path } => {
                let s = path.to_string_lossy();
                assert!(s.starts_with("events/-history/"));
                assert!(s.contains("!test_signal"));
            }
            _ => panic!("Expected Touch"),
        }
    }

    #[test]
    fn test_history_ignored() {
        let sub = EventsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["events".to_string(), "-history".to_string(), "old".to_string()],
            scope: Scope::Events,
        };
        assert!(sub.handle(&event).is_empty());
    }

    #[test]
    fn test_retract_no_effects() {
        let sub = EventsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Retract,
            segments: vec!["events".to_string(), "!done".to_string()],
            scope: Scope::Events,
        };
        assert!(sub.handle(&event).is_empty());
    }
}
