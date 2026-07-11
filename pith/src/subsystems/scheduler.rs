/// Scheduler subsystem — handles `schedules/` scope.
///
/// Scheduled tasks are zero-byte files whose **mtime** encodes
/// the next firing time. The scheduler is now **active**:
///
/// - The `SchedulerSubsystem` receives events for schedule creation/removal and logs them.
/// - A periodic tick (wired in `boot::run`, ~900ms cadence) scans `schedules/` and fires
///   any task whose on-disk mtime is in the past by emitting `events/!schedule_{name}`.
///
/// This makes time-based autonomous behavior real in 0-Bytes OS.
/// Firing a task = touching `events/!schedule_{name}` + log entry.
///
/// Structure:
///   schedules/cleanup          — mtime = next fire time
///   schedules/daily_report     — mtime = next fire time

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use tracing::{debug, info, warn};

use crate::dispatcher::Scope;
use crate::effector::{Effect, Effector};
use crate::subsystems::logs::LogsSubsystem;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};
use crate::trie::Trie;

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

                // mtime-based firing is handled by the global scheduler::tick() called
                // periodically from the main engine loop in boot::run().
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

    #[tokio::test]
    async fn test_scheduler_tick_fires_past_due_and_skips_future() {
        use tempfile::TempDir;
        use crate::alphabet::Alphabet;
        use crate::trie::Trie;

        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Minimal reserved alphabet
        let reserved = root.join("hard/reserved");
        std::fs::create_dir_all(&reserved).unwrap();
        for ch in ['$', '-', '!', '€'] {
            std::fs::File::create(reserved.join(ch.to_string())).unwrap();
        }
        let alphabet = Alphabet::load(&reserved).unwrap();

        // Create schedules/ + two tasks
        let schedules = root.join("schedules");
        std::fs::create_dir_all(&schedules).unwrap();

        // Past due schedule (should fire)
        let past_task = schedules.join("heartbeat");
        std::fs::File::create(&past_task).unwrap();
        let past_time = SystemTime::now() - Duration::from_secs(5);
        let ft = filetime::FileTime::from_system_time(past_time);
        filetime::set_file_mtime(&past_task, ft).unwrap();

        // Future schedule (should not fire)
        let future_task = schedules.join("daily_report");
        std::fs::File::create(&future_task).unwrap();
        let future_time = SystemTime::now() + Duration::from_secs(3600);
        let ft = filetime::FileTime::from_system_time(future_time);
        filetime::set_file_mtime(&future_task, ft).unwrap();

        // Build trie (so tick can list schedules/)
        let trie = Trie::build(root, &alphabet).unwrap();

        // Create effector + events/ dir so firing can succeed
        std::fs::create_dir_all(root.join("events")).unwrap();
        let effector = Effector::new(root.to_path_buf());

        // Run one tick
        let fired = tick(&root.to_path_buf(), &Arc::new(RwLock::new(trie)), &effector).await;

        assert_eq!(fired, 1, "only the past-due schedule should have fired");

        // Verify the fired event was created on disk
        let fired_event = root.join("events/!schedule_heartbeat");
        assert!(fired_event.exists(), "firing should have created the signal file");

        // Future one should still exist but not have produced an event
        let future_event = root.join("events/!schedule_daily_report");
        assert!(!future_event.exists());
    }
}

/// Perform one scheduler tick.
///
/// Scans direct children under `schedules/` in the trie, reads their real on-disk
/// mtimes (the source of truth for "when this should fire"), and fires any that are due.
///
/// Returns the number of schedules that fired during this tick.
/// Firing a schedule emits `events/!schedule_{name}` + a log entry.
pub async fn tick(fs_root: &Path, trie: &Arc<RwLock<Trie>>, effector: &Effector) -> usize {
    let schedules_node = {
        let guard = trie.read().unwrap();
        guard.get(&["schedules"]).cloned()
    };

    let Some(node) = schedules_node else {
        return 0;
    };

    let mut fired = 0usize;

    for (name, child) in &node.children {
        // Only consider direct file children (actual scheduled tasks).
        // Skip the conventional "0" anchor if present.
        if name == "0" && child.children.is_empty() {
            continue;
        }

        if !child.is_file {
            continue;
        }

        let full_path = fs_root.join("schedules").join(name);

        // Read the authoritative next-fire time from the filesystem metadata.
        let mtime = match std::fs::metadata(&full_path).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(e) => {
                debug!("Scheduler tick: could not read mtime for {}: {}", name, e);
                continue;
            }
        };

        if let Some(effects) = check_schedule(name, mtime) {
            info!("Scheduler firing: {}", name);
            fired += 1;

            for effect in &effects {
                if let Err(e) = effector.execute(effect).await {
                    warn!("Scheduler effect failed for {}: {}", name, e);
                }
            }
        }
    }

    if fired > 0 {
        debug!("Scheduler tick: fired {} schedule(s)", fired);
    }

    fired
}
