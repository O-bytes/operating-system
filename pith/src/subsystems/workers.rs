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
use std::sync::{Arc, RwLock};

use tracing::{debug, info};

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::logs::LogsSubsystem;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};
use crate::trie::Trie;

pub struct WorkersSubsystem {
    trie: Option<Arc<RwLock<Trie>>>,
}

impl WorkersSubsystem {
    pub fn new() -> Self {
        Self { trie: None }
    }

    pub fn new_with_trie(trie: Arc<RwLock<Trie>>) -> Self {
        Self { trie: Some(trie) }
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

        // When a job is assigned, try to execute it if we have the trie
        if event.segments.len() >= 5
            && event.segments[2] == "-assigned"
            && event.segments[3] == "jobs"
        {
            let job_id = &event.segments[4];
            if let Some(ref trie_arc) = self.trie {
                let exec_effects = self.execute_assigned_job(&worker_id, job_id, trie_arc);
                effects.extend(exec_effects);
            }
        }

        effects
    }
}

impl WorkersSubsystem {
    /// When a worker is assigned a job, look for declared actions in the job
    /// and execute them. This is what makes Jobs + Workers real.
    fn execute_assigned_job(
        &self,
        worker_id: &str,
        job_id: &str,
        trie: &Arc<RwLock<Trie>>,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        let guard = trie.read().unwrap();

        let job_root = match guard.get(&["jobs", job_id]) {
            Some(j) => j,
            None => return effects,
        };

        // Look for actions under jobs/{id}/-action/touch/*
        if let Some(action_root) = job_root.children.get("-action") {
            if let Some(touch_actions) = action_root.children.get("touch") {
                for target in touch_actions.leaf_paths() {
                    debug!("Worker {} executing job {} action: touch {}", worker_id, job_id, target);
                    effects.push(Effect::Touch {
                        path: PathBuf::from(target),
                    });
                }
            }

            // Support simple "emit" actions for observability and composition
            if let Some(emit_actions) = action_root.children.get("emit") {
                for signal in emit_actions.children.keys() {
                    let signal_name = if signal.starts_with('!') { signal.clone() } else { format!("!{}", signal) };
                    debug!("Worker {} executing job {} action: emit {}", worker_id, job_id, signal_name);
                    effects.push(Effect::Touch {
                        path: PathBuf::from(format!("events/{}", signal_name)),
                    });
                }
            }

            // Support writing small content (very useful for reports, logs, state)
            // In 0-bytes, "content" lives in the *filename* of the final segment (e.g. (report text)).
            // This matches the channel message convention: channels/#name/~seq/(message)
            if let Some(write_actions) = action_root.children.get("write") {
                for (path, content_node) in &write_actions.children {
                    // Support two styles:
                    // 1. write/some/path/(the actual content here)  → creates some/path/(the actual content here)
                    // 2. write/some/path  with children under it     → last child becomes the (content)
                    let mut full_path = path.clone();
                    let mut content = String::new();

                    if !content_node.children.is_empty() {
                        // Content lives in child segments — reconstruct as path + (last segment as content)
                        let mut parts = vec![path.clone()];
                        for (k, _) in &content_node.children {
                            parts.push(k.clone());
                        }
                        if parts.len() > 1 {
                            content = parts.pop().unwrap();
                            full_path = parts.join("/");
                        } else {
                            content = parts[0].clone();
                        }
                    } else {
                        full_path = path.clone();
                    }

                    let target = if content.is_empty() {
                        full_path.clone()
                    } else {
                        // Encode content in the final segment using the canonical (content) form
                        format!("{}/({})", full_path, content)
                    };

                    debug!("Worker {} executing job {} action: write {}", worker_id, job_id, target);
                    effects.push(Effect::Touch {
                        path: PathBuf::from(target),
                    });
                }
            }

            // Support removal (healing / cleanup actions)
            if let Some(remove_actions) = action_root.children.get("remove") {
                for path in remove_actions.leaf_paths() {
                    debug!("Worker {} executing job {} action: remove {}", worker_id, job_id, path);
                    effects.push(Effect::Remove {
                        path: PathBuf::from(path),
                    });
                }
            }
        }

        // After performing actions, automatically complete the job (v1 behavior for simple jobs).
        // In a more advanced system, the actions themselves or the worker would decide when to complete.
        if !effects.is_empty() {
            effects.push(Effect::Touch {
                path: PathBuf::from(format!("jobs/{}/-state/completed", job_id)),
            });
            effects.push(Effect::Touch {
                path: PathBuf::from(format!("events/!job_{}_completed", job_id)),
            });
            info!("Worker {} completed job {} (actions executed)", worker_id, job_id);
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
