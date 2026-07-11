/// States subsystem — handles `states/` scope.
///
/// The global state machine. File existence = state is active.
///
/// Structure:
///   states/0              — State 0 active (file exists)
///   states/1              — State 1 active (file exists)
///   states/-transitions/
///     0/1/                — Transition rule: from state 0 to state 1
///       -condition/...    — What triggers the transition
///       -action/...       — What to do when transitioning
///
/// The subsystem tracks which states are active and logs transitions.

use std::path::PathBuf;

use tracing::info;

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::logs::LogsSubsystem;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};

pub struct StatesSubsystem;

impl StatesSubsystem {
    pub fn new() -> Self {
        Self
    }
}

impl Subsystem for StatesSubsystem {
    fn scope(&self) -> Scope {
        Scope::States
    }

    fn handle(&self, event: &FsEvent) -> Vec<Effect> {
        let mut effects = Vec::new();

        if event.segments.len() < 2 {
            return effects;
        }

        // Ignore transition definitions — they are structural, not events.
        if event.segments.iter().any(|s| s == "-transitions") {
            return effects;
        }

        let state_name = &event.segments[1];

        match event.kind {
            FsEventKind::Assert => {
                info!("State activated: {}", state_name);
                effects.push(LogsSubsystem::log_effect(
                    &format!("state {} activated", state_name),
                ));

                // Emit a signal for state change.
                effects.push(Effect::Touch {
                    path: PathBuf::from(format!("events/!state_{}_active", state_name)),
                });
            }
            FsEventKind::Retract => {
                info!("State deactivated: {}", state_name);
                effects.push(LogsSubsystem::log_effect(
                    &format!("state {} deactivated", state_name),
                ));

                // Remove state signal.
                effects.push(Effect::Remove {
                    path: PathBuf::from(format!("events/!state_{}_active", state_name)),
                });
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
    fn test_state_activated() {
        let sub = StatesSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["states".to_string(), "1".to_string()],
            scope: Scope::States,
        };

        let effects = sub.handle(&event);
        // Should produce: log + event signal
        assert_eq!(effects.len(), 2);

        // First: log entry
        match &effects[0] {
            Effect::Touch { path } => {
                assert!(path.to_string_lossy().contains("state 1 activated"));
            }
            _ => panic!("Expected log Touch"),
        }

        // Second: event signal
        match &effects[1] {
            Effect::Touch { path } => {
                assert_eq!(path.to_string_lossy(), "events/!state_1_active");
            }
            _ => panic!("Expected event Touch"),
        }
    }

    #[test]
    fn test_state_deactivated() {
        let sub = StatesSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Retract,
            segments: vec!["states".to_string(), "1".to_string()],
            scope: Scope::States,
        };

        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 2);

        match &effects[1] {
            Effect::Remove { path } => {
                assert_eq!(path.to_string_lossy(), "events/!state_1_active");
            }
            _ => panic!("Expected Remove"),
        }
    }

    #[test]
    fn test_transition_definitions_ignored() {
        let sub = StatesSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "states".to_string(),
                "-transitions".to_string(),
                "0".to_string(),
                "1".to_string(),
            ],
            scope: Scope::States,
        };

        assert!(sub.handle(&event).is_empty());
    }
}
