/// Scheduler subsystem — handles `schedules/` scope.
///
/// Scheduled tasks are zero-byte files whose **mtime** encodes
/// the next firing time. The scheduler:
///   - On boot: scans all files, reads mtimes, builds a fire queue
///   - On new file: adds to queue
///   - On file removal: removes from queue
///   - Periodically: checks if any task's mtime <= now, fires it
///
/// Firing a task = touching `events/!schedule_{name}`.
///
/// Structure:
///   schedules/cleanup          — mtime = next fire time
///   schedules/daily_report     — mtime = next fire time

use std::path::PathBuf;

use tracing::info;

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::logs::LogsSubsystem;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};

pub struct SchedulerSubsystem;

impl SchedulerSubsystem {
    pub fn new() -> Self {
        Self
    }
}

impl Subsystem for SchedulerSubsystem {
    fn scope(&self) -> Scope {
        Scope::Schedules
    }

    fn handle(&self, event: &FsEvent) -> Vec<Effect> {
        let mut effects = Vec::new();

        if event.segments.len() < 2 {
            return effects;
        }

        let task_name = &event.segments[1];

        match event.kind {
            FsEventKind::Assert => {
                info!("Schedule registered: {}", task_name);
                effects.push(LogsSubsystem::log_effect(
                    &format!("schedule {} registered", task_name),
                ));

                // The actual mtime-based firing is handled by the scheduler tick
                // in the engine's event loop (future enhancement).
                // For now, we just register and log.
            }
            FsEventKind::Retract => {
                info!("Schedule removed: {}", task_name);
                effects.push(LogsSubsystem::log_effect(
                    &format!("schedule {} removed", task_name),
                ));
            }
            _ => {}
        }

        effects
    }
}

/// Check a schedule file and return a fire effect if mtime <= now.
///
/// Called by the engine's tick loop to poll scheduled tasks.
pub fn check_schedule(task_name: &str, mtime: std::time::SystemTime) -> Option<Vec<Effect>> {
    let now = std::time::SystemTime::now();
    if mtime <= now {
        let mut effects = Vec::new();
        info!("Schedule fired: {}", task_name);

        effects.push(Effect::Touch {
            path: PathBuf::from(format!("events/!schedule_{}", task_name)),
        });
        effects.push(LogsSubsystem::log_effect(
            &format!("schedule {} fired", task_name),
        ));

        Some(effects)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn test_schedule_registered() {
        let sub = SchedulerSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["schedules".to_string(), "cleanup".to_string()],
            scope: Scope::Schedules,
        };

        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 1); // log
        match &effects[0] {
            Effect::Touch { path } => {
                assert!(path.to_string_lossy().contains("schedule cleanup registered"));
            }
            _ => panic!("Expected log Touch"),
        }
    }

    #[test]
    fn test_schedule_removed() {
        let sub = SchedulerSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Retract,
            segments: vec!["schedules".to_string(), "cleanup".to_string()],
            scope: Scope::Schedules,
        };

        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn test_check_schedule_past_fires() {
        let past = SystemTime::now() - Duration::from_secs(10);
        let effects = check_schedule("cleanup", past);
        assert!(effects.is_some());

        let effects = effects.unwrap();
        let has_event = effects.iter().any(|e| matches!(e,
            Effect::Touch { path } if path.to_string_lossy() == "events/!schedule_cleanup"
        ));
        assert!(has_event);
    }

    #[test]
    fn test_check_schedule_future_skips() {
        let future = SystemTime::now() + Duration::from_secs(3600);
        let effects = check_schedule("backup", future);
        assert!(effects.is_none());
    }
}
