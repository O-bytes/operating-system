/// Workers subsystem — handles `workers/` scope.
///
/// Workers are execution units mapped to engine tasks.
///
/// Structure:
///   workers/{id}/
///     -expected/type/worker    — Type declaration
///     -state/idle              — Current state: idle, busy, stopped
///     -identity/{id}           — Runs as this identity
///     -capacity/~{n}           — Max concurrent jobs
///     -assigned/
///       jobs/{job_id}          — Currently assigned jobs
///     #inbox/                  — IPC incoming
///     #outbox/                 — IPC outgoing
///
/// When a worker state changes, the subsystem:
///   - Logs the transition
///   - Emits event signals
///
/// When a job is assigned (-assigned/jobs/{id}):
///   - Logs the assignment
///   - Emits events/!worker_{id}_assigned_{job_id}

use std::path::PathBuf;

use tracing::info;

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::logs::LogsSubsystem;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};

pub struct WorkersSubsystem;

impl WorkersSubsystem {
    pub fn new() -> Self {
        Self
    }

    fn worker_id(event: &FsEvent) -> Option<&str> {
        if event.segments.len() >= 2 {
            Some(&event.segments[1])
        } else {
            None
        }
    }
}

impl Subsystem for WorkersSubsystem {
    fn scope(&self) -> Scope {
        Scope::Workers
    }

    fn handle(&self, event: &FsEvent) -> Vec<Effect> {
        let mut effects = Vec::new();

        let worker_id = match Self::worker_id(event) {
            Some(id) => id.to_string(),
            None => return effects,
        };

        // Skip the anchor "0" directory.
        if worker_id == "0" && event.segments.len() <= 2 {
            return effects;
        }

        match event.kind {
            FsEventKind::Assert => {
                // Detect state changes: workers/{id}/-state/{state_name}
                if event.segments.len() >= 4 && event.segments[2] == "-state" {
                    let state = &event.segments[3];
                    info!("Worker {} state: {}", worker_id, state);

                    effects.push(LogsSubsystem::log_effect(
                        &format!("worker {} state {}", worker_id, state),
                    ));
                    effects.push(Effect::Touch {
                        path: PathBuf::from(format!(
                            "events/!worker_{}_{}",
                            worker_id, state
                        )),
                    });
                }

                // Detect job assignment: workers/{id}/-assigned/jobs/{job_id}
                if event.segments.len() >= 5
                    && event.segments[2] == "-assigned"
                    && event.segments[3] == "jobs"
                {
                    let job_id = &event.segments[4];
                    info!("Worker {} assigned job {}", worker_id, job_id);

                    effects.push(LogsSubsystem::log_effect(
                        &format!("worker {} assigned job {}", worker_id, job_id),
                    ));
                    effects.push(Effect::Touch {
                        path: PathBuf::from(format!(
                            "events/!worker_{}_assigned_{}",
                            worker_id, job_id
                        )),
                    });
                }

                // Detect new worker creation
                if event.segments.len() == 5
                    && event.segments[2] == "-expected"
                    && event.segments[3] == "type"
                    && event.segments[4] == "worker"
                {
                    info!("New worker registered: {}", worker_id);
                    effects.push(LogsSubsystem::log_effect(
                        &format!("worker {} registered", worker_id),
                    ));
                }
            }
            FsEventKind::Retract => {
                // Detect state removal
                if event.segments.len() >= 4 && event.segments[2] == "-state" {
                    let state = &event.segments[3];
                    info!("Worker {} leaving state: {}", worker_id, state);

                    effects.push(Effect::Remove {
                        path: PathBuf::from(format!(
                            "events/!worker_{}_{}",
                            worker_id, state
                        )),
                    });
                }

                // Detect job unassignment
                if event.segments.len() >= 5
                    && event.segments[2] == "-assigned"
                    && event.segments[3] == "jobs"
                {
                    let job_id = &event.segments[4];
                    info!("Worker {} unassigned job {}", worker_id, job_id);

                    effects.push(LogsSubsystem::log_effect(
                        &format!("worker {} unassigned job {}", worker_id, job_id),
                    ));
                    effects.push(Effect::Remove {
                        path: PathBuf::from(format!(
                            "events/!worker_{}_assigned_{}",
                            worker_id, job_id
                        )),
                    });
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
    fn test_worker_state_change() {
        let sub = WorkersSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "workers".to_string(),
                "1".to_string(),
                "-state".to_string(),
                "idle".to_string(),
            ],
            scope: Scope::Workers,
        };

        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 2); // log + event

        match &effects[1] {
            Effect::Touch { path } => {
                assert_eq!(path.to_string_lossy(), "events/!worker_1_idle");
            }
            _ => panic!("Expected Touch"),
        }
    }

    #[test]
    fn test_worker_job_assignment() {
        let sub = WorkersSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "workers".to_string(),
                "1".to_string(),
                "-assigned".to_string(),
                "jobs".to_string(),
                "42".to_string(),
            ],
            scope: Scope::Workers,
        };

        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 2); // log + event

        let has_assign = effects.iter().any(|e| matches!(e,
            Effect::Touch { path } if path.to_string_lossy() == "events/!worker_1_assigned_42"
        ));
        assert!(has_assign);
    }

    #[test]
    fn test_worker_job_unassignment() {
        let sub = WorkersSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Retract,
            segments: vec![
                "workers".to_string(),
                "1".to_string(),
                "-assigned".to_string(),
                "jobs".to_string(),
                "42".to_string(),
            ],
            scope: Scope::Workers,
        };

        let effects = sub.handle(&event);
        // log + remove event
        assert_eq!(effects.len(), 2);

        let has_remove = effects.iter().any(|e| matches!(e,
            Effect::Remove { path } if path.to_string_lossy() == "events/!worker_1_assigned_42"
        ));
        assert!(has_remove);
    }

    #[test]
    fn test_new_worker_registered() {
        let sub = WorkersSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "workers".to_string(),
                "3".to_string(),
                "-expected".to_string(),
                "type".to_string(),
                "worker".to_string(),
            ],
            scope: Scope::Workers,
        };

        let effects = sub.handle(&event);
        let has_log = effects.iter().any(|e| matches!(e,
            Effect::Touch { path } if path.to_string_lossy().contains("worker 3 registered")
        ));
        assert!(has_log);
    }

    #[test]
    fn test_anchor_ignored() {
        let sub = WorkersSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["workers".to_string(), "0".to_string()],
            scope: Scope::Workers,
        };
        assert!(sub.handle(&event).is_empty());
    }
}
