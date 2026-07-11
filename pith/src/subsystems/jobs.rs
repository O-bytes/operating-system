/// Jobs subsystem — handles `jobs/` scope.
///
/// Job lifecycle: pending → running → completed | failed
///
/// Structure:
///   jobs/{id}/
///     -expected/type/job       — Type declaration
///     -state/pending           — Current state (one file = current state)
///     -owner/{identity_id}     — Who owns this job
///     ^~{n}                    — Priority level
///     ¶input/(raw value)       — Input data
///     ¶output/(raw value)      — Output data
///     !started                 — Signal: job started
///     !completed               — Signal: job completed
///     !failed                  — Signal: job failed
///
/// When a new job directory appears with `-state/pending`, the subsystem:
///   - Emits a signal events/!job_{id}_pending
///   - Logs the creation
///
/// When state transitions to `running`:
///   - Emits events/!job_{id}_running
///
/// When `!completed` is touched:
///   - Emits events/!job_{id}_completed
///   - Logs completion

use std::path::PathBuf;

use tracing::info;

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::logs::LogsSubsystem;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};

pub struct JobsSubsystem;

impl JobsSubsystem {
    pub fn new() -> Self {
        Self
    }

    /// Extract the job ID from the event segments.
    /// segments: ["jobs", "{id}", ...]
    fn job_id(event: &FsEvent) -> Option<&str> {
        if event.segments.len() >= 2 {
            Some(&event.segments[1])
        } else {
            None
        }
    }
}

impl Subsystem for JobsSubsystem {
    fn scope(&self) -> Scope {
        Scope::Jobs
    }

    fn handle(&self, event: &FsEvent) -> Vec<Effect> {
        let mut effects = Vec::new();

        let job_id = match Self::job_id(event) {
            Some(id) => id.to_string(),
            None => return effects,
        };

        // Skip the anchor "0" directory events at top level.
        if job_id == "0" && event.segments.len() <= 2 {
            return effects;
        }

        match event.kind {
            FsEventKind::Assert => {
                // Detect state changes: jobs/{id}/-state/{state_name}
                if event.segments.len() >= 4 && event.segments[2] == "-state" {
                    let state = &event.segments[3];
                    info!("Job {} state: {}", job_id, state);

                    effects.push(LogsSubsystem::log_effect(
                        &format!("job {} state {}", job_id, state),
                    ));
                    effects.push(Effect::Touch {
                        path: PathBuf::from(format!("events/!job_{}_{}",  job_id, state)),
                    });
                }

                // Detect signals: jobs/{id}/!completed, !started, !failed
                if event.segments.len() >= 3 {
                    let last = &event.segments[event.segments.len() - 1];
                    if last.starts_with('!') {
                        let signal = &last[1..]; // strip the '!'
                        info!("Job {} signal: {}", job_id, signal);

                        effects.push(LogsSubsystem::log_effect(
                            &format!("job {} {}", job_id, signal),
                        ));
                        effects.push(Effect::Touch {
                            path: PathBuf::from(format!("events/!job_{}_{}", job_id, signal)),
                        });
                    }
                }

                // Detect new job creation (directory with -expected/type/job)
                if event.segments.len() == 5
                    && event.segments[2] == "-expected"
                    && event.segments[3] == "type"
                    && event.segments[4] == "job"
                {
                    info!("New job created: {}", job_id);
                    effects.push(LogsSubsystem::log_effect(
                        &format!("job {} created", job_id),
                    ));
                }
            }
            FsEventKind::Retract => {
                // Detect state removal: jobs/{id}/-state/{old_state}
                if event.segments.len() >= 4 && event.segments[2] == "-state" {
                    let state = &event.segments[3];
                    info!("Job {} leaving state: {}", job_id, state);

                    // Clean up the event signal for the old state.
                    effects.push(Effect::Remove {
                        path: PathBuf::from(format!("events/!job_{}_{}", job_id, state)),
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
    fn test_job_state_pending() {
        let sub = JobsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "jobs".to_string(),
                "1".to_string(),
                "-state".to_string(),
                "pending".to_string(),
            ],
            scope: Scope::Jobs,
        };

        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 2); // log + event signal

        match &effects[1] {
            Effect::Touch { path } => {
                assert_eq!(path.to_string_lossy(), "events/!job_1_pending");
            }
            _ => panic!("Expected Touch"),
        }
    }

    #[test]
    fn test_job_completed_signal() {
        let sub = JobsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "jobs".to_string(),
                "42".to_string(),
                "!completed".to_string(),
            ],
            scope: Scope::Jobs,
        };

        let effects = sub.handle(&event);
        assert!(effects.len() >= 2); // log + event

        let has_event = effects.iter().any(|e| matches!(e,
            Effect::Touch { path } if path.to_string_lossy() == "events/!job_42_completed"
        ));
        assert!(has_event, "Should emit job_42_completed event");
    }

    #[test]
    fn test_job_state_transition_cleanup() {
        let sub = JobsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Retract,
            segments: vec![
                "jobs".to_string(),
                "1".to_string(),
                "-state".to_string(),
                "pending".to_string(),
            ],
            scope: Scope::Jobs,
        };

        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 1); // remove old event signal

        match &effects[0] {
            Effect::Remove { path } => {
                assert_eq!(path.to_string_lossy(), "events/!job_1_pending");
            }
            _ => panic!("Expected Remove"),
        }
    }

    #[test]
    fn test_new_job_created() {
        let sub = JobsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "jobs".to_string(),
                "5".to_string(),
                "-expected".to_string(),
                "type".to_string(),
                "job".to_string(),
            ],
            scope: Scope::Jobs,
        };

        let effects = sub.handle(&event);
        // Should log the creation.
        let has_log = effects.iter().any(|e| matches!(e,
            Effect::Touch { path } if path.to_string_lossy().contains("job 5 created")
        ));
        assert!(has_log, "Should log job creation");
    }

    #[test]
    fn test_anchor_ignored() {
        let sub = JobsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["jobs".to_string(), "0".to_string()],
            scope: Scope::Jobs,
        };
        assert!(sub.handle(&event).is_empty());
    }
}
