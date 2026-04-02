/// Programs subsystem — handles `programs/` scope.
///
/// A program is a directory tree encoding a state machine:
///   programs/{name}/
///     -expected/type/program     — Type declaration
///     -entry/-state/init         — Starting state
///     -states/
///       init/
///         -action/touch/{path}   — What to do in this state
///         -transitions/
///           ready/               — Next state name
///             -condition/{path}  — Condition: file must exist
///       ready/
///         -action/touch/{path}
///
/// Lifecycle:
///   1. touch programs/{name}/!run  → engine starts the program
///   2. Engine reads -entry/-state/{init} → sets current state
///   3. Executes -action/* effects for the current state
///   4. Checks -transitions: if condition met, transitions to next state
///   5. touch programs/{name}/!completed when no more transitions

use std::path::PathBuf;

use tracing::info;

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::logs::LogsSubsystem;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};

pub struct ProgramsSubsystem;

impl ProgramsSubsystem {
    pub fn new() -> Self {
        Self
    }
}

impl Subsystem for ProgramsSubsystem {
    fn scope(&self) -> Scope {
        Scope::Programs
    }

    fn handle(&self, event: &FsEvent) -> Vec<Effect> {
        let mut effects = Vec::new();

        if event.segments.len() < 2 {
            return effects;
        }

        let program_name = &event.segments[1];

        match event.kind {
            FsEventKind::Assert => {
                // Detect !run signal
                if event.segments.len() >= 3 && event.segments[2] == "!run" {
                    info!("Program started: {}", program_name);

                    effects.push(LogsSubsystem::log_effect(
                        &format!("program {} started", program_name),
                    ));
                    effects.push(Effect::Touch {
                        path: PathBuf::from(format!(
                            "events/!program_{}_started",
                            program_name
                        )),
                    });
                }

                // Detect !completed signal
                if event.segments.len() >= 3 && event.segments[2] == "!completed" {
                    info!("Program completed: {}", program_name);

                    effects.push(LogsSubsystem::log_effect(
                        &format!("program {} completed", program_name),
                    ));
                    effects.push(Effect::Touch {
                        path: PathBuf::from(format!(
                            "events/!program_{}_completed",
                            program_name
                        )),
                    });
                }

                // Detect new program registration
                if event.segments.len() == 5
                    && event.segments[2] == "-expected"
                    && event.segments[3] == "type"
                    && event.segments[4] == "program"
                {
                    info!("Program installed: {}", program_name);
                    effects.push(LogsSubsystem::log_effect(
                        &format!("program {} installed", program_name),
                    ));
                }
            }
            FsEventKind::Retract => {
                // Detect !run removal (program stopped externally)
                if event.segments.len() >= 3 && event.segments[2] == "!run" {
                    info!("Program stopped: {}", program_name);
                    effects.push(LogsSubsystem::log_effect(
                        &format!("program {} stopped", program_name),
                    ));
                }
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
    fn test_program_run() {
        let sub = ProgramsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "programs".to_string(),
                "my_app".to_string(),
                "!run".to_string(),
            ],
            scope: Scope::Programs,
        };

        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 2); // log + event

        let has_event = effects.iter().any(|e| matches!(e,
            Effect::Touch { path } if path.to_string_lossy() == "events/!program_my_app_started"
        ));
        assert!(has_event);
    }

    #[test]
    fn test_program_completed() {
        let sub = ProgramsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "programs".to_string(),
                "my_app".to_string(),
                "!completed".to_string(),
            ],
            scope: Scope::Programs,
        };

        let effects = sub.handle(&event);
        let has_event = effects.iter().any(|e| matches!(e,
            Effect::Touch { path } if path.to_string_lossy() == "events/!program_my_app_completed"
        ));
        assert!(has_event);
    }

    #[test]
    fn test_program_installed() {
        let sub = ProgramsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "programs".to_string(),
                "my_app".to_string(),
                "-expected".to_string(),
                "type".to_string(),
                "program".to_string(),
            ],
            scope: Scope::Programs,
        };

        let effects = sub.handle(&event);
        let has_log = effects.iter().any(|e| matches!(e,
            Effect::Touch { path } if path.to_string_lossy().contains("program my_app installed")
        ));
        assert!(has_log);
    }

    #[test]
    fn test_program_stopped() {
        let sub = ProgramsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Retract,
            segments: vec![
                "programs".to_string(),
                "my_app".to_string(),
                "!run".to_string(),
            ],
            scope: Scope::Programs,
        };

        let effects = sub.handle(&event);
        let has_log = effects.iter().any(|e| matches!(e,
            Effect::Touch { path } if path.to_string_lossy().contains("program my_app stopped")
        ));
        assert!(has_log);
    }
}
