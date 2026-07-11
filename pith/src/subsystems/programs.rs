/// Programs subsystem — handles `programs/` scope.
///
/// Programs are directory trees that encode executable state machines.
///
/// Current capabilities (strong v1):
/// - On `!run`: enter initial state from `-entry/-state/`
/// - Activate state marker + run `touch` actions from `-states/{state}/-action/touch/`
/// - **Automatic transitions**: if a transition's `-condition/` files exist, move to next state,
///   run its actions, and emit transition events (all synchronously on `!run` for now).

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tracing::{debug, info};

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::logs::LogsSubsystem;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};
use crate::trie::{Trie, TrieNode};

pub struct ProgramsSubsystem {
    trie: Option<Arc<RwLock<Trie>>>,
}

impl ProgramsSubsystem {
    pub fn new() -> Self {
        Self { trie: None }
    }

    pub fn new_with_trie(trie: Arc<RwLock<Trie>>) -> Self {
        Self { trie: Some(trie) }
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
                // Detect !run signal → start program execution
                if event.segments.len() >= 3 && event.segments[2] == "!run" {
                    info!("Program run requested: {}", program_name);

                    effects.push(LogsSubsystem::log_effect(
                        &format!("program {} started", program_name),
                    ));
                    effects.push(Effect::Touch {
                        path: PathBuf::from(format!(
                            "events/!program_{}_started",
                            program_name
                        )),
                    });

                    // If we have trie access, try to actually execute the program
                    if let Some(ref trie_arc) = self.trie {
                        let exec_effects = self.execute_program(program_name, trie_arc);
                        effects.extend(exec_effects);
                    } else {
                        debug!("ProgramsSubsystem has no trie – basic signal only for {}", program_name);
                    }
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

impl ProgramsSubsystem {
    /// Attempt to execute a program using its filesystem definition.
    /// v1 supports:
    ///   - Reading initial state from -entry/-state/
    ///   - Activating the state
    ///   - Executing simple `touch` actions
    fn execute_program(&self, name: &str, trie: &Arc<RwLock<Trie>>) -> Vec<Effect> {
        let mut effects = Vec::new();
        let guard = trie.read().unwrap();

        let prog_root = match guard.get(&["programs", name]) {
            Some(node) => node,
            None => return effects,
        };

        // 1. Find initial state from -entry/-state/
        let initial_state = if let Some(entry) = prog_root.children.get("-entry") {
            if let Some(state_dir) = entry.children.get("-state") {
                state_dir.children.keys().next().cloned()
            } else {
                None
            }
        } else {
            None
        };

        let state = match initial_state {
            Some(s) => s,
            None => {
                debug!("Program {} has no -entry/-state/ – cannot execute", name);
                return effects;
            }
        };

        info!("Program {} entering initial state: {}", name, state);

        // 2. Activate current state (existence of this file = current state)
        effects.push(Effect::Touch {
            path: PathBuf::from(format!("programs/{}/-state/{}", name, state)),
        });

        // Emit state entry event
        effects.push(Effect::Touch {
            path: PathBuf::from(format!("events/!program_{}_entered_{}", name, state)),
        });

        // 3. Execute actions under -states/{state}/-action/touch/*
        let mut current_state = state.clone();
        Self::execute_state_actions(&prog_root, &current_state, &mut effects);

        // 4. Perform any immediate transitions that conditions allow (v1)
        let max_transitions = 10;
        for _ in 0..max_transitions {
            if let Some(next_state) = self.find_ready_transition(&prog_root, &current_state, &guard) {
                // Leave current state
                let old_state = current_state.clone();

                // Transition
                effects.push(Effect::Remove {
                    path: PathBuf::from(format!("programs/{}/-state/{}", name, old_state)),
                });
                effects.push(Effect::Touch {
                    path: PathBuf::from(format!("programs/{}/-state/{}", name, next_state)),
                });

                effects.push(Effect::Touch {
                    path: PathBuf::from(format!(
                        "events/!program_{}_transitioned_{}_to_{}",
                        name, old_state, next_state
                    )),
                });

                info!("Program {} transitioned: {} -> {}", name, old_state, next_state);

                current_state = next_state.clone();

                // Execute actions of the new state
                Self::execute_state_actions(&prog_root, &current_state, &mut effects);
            } else {
                break;
            }
        }

        effects
    }

    /// Execute the declared actions of a given state (touch, schedule, remove).
    fn execute_state_actions(
        prog_root: &TrieNode,
        state: &str,
        effects: &mut Vec<Effect>,
    ) {
        if let Some(states) = prog_root.children.get("-states") {
            if let Some(state_node) = states.children.get(state) {
                if let Some(actions) = state_node.children.get("-action") {
                    if let Some(touch_actions) = actions.children.get("touch") {
                        for target in touch_actions.leaf_paths() {
                            debug!("Program action: touch {}", target);
                            effects.push(Effect::Touch {
                                path: PathBuf::from(target),
                            });
                        }
                    }

                    // Support creating schedules (extremely powerful for autonomous agents)
                    if let Some(schedule_actions) = actions.children.get("schedule") {
                        for (schedule_name, _) in &schedule_actions.children {
                            let schedule_path = format!("schedules/{}", schedule_name);
                            // Arm it ~2 minutes in the future (for demo). In a real system this would be configurable.
                            let future_mtime = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
                            effects.push(Effect::TouchWithMtime {
                                path: PathBuf::from(schedule_path),
                                mtime: future_mtime,
                            });
                        }
                    }

                    // Support remove/ actions — lets programs claim/consume channel messages,
                    // clean up, or heal directly (symmetric to workers)
                    if let Some(remove_actions) = actions.children.get("remove") {
                        for target in remove_actions.leaf_paths() {
                            debug!("Program action: remove {}", target);
                            effects.push(Effect::Remove {
                                path: PathBuf::from(target),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Find the first next state whose conditions are satisfied.
    fn find_ready_transition(
        &self,
        prog_root: &TrieNode,
        current_state: &str,
        guard: &std::sync::RwLockReadGuard<Trie>,
    ) -> Option<String> {
        let states = prog_root.children.get("-states")?;
        let state_node = states.children.get(current_state)?;
        let transitions = state_node.children.get("-transitions")?;

        for (next_state, trans_node) in &transitions.children {
            if let Some(conditions) = trans_node.children.get("-condition") {
                // A transition is ready if *any* full path declared under -condition/
                // exists in the trie. Only leaf paths count: a condition like
                // events/!foo must not be satisfied by the mere existence of events/.
                if Self::conditions_satisfied(conditions, guard) {
                    return Some(next_state.clone());
                }
            }
        }

        None
    }

    /// True if any leaf path declared under a -condition/ subtree exists in the trie.
    fn conditions_satisfied(conditions: &TrieNode, guard: &std::sync::RwLockReadGuard<Trie>) -> bool {
        conditions
            .leaf_paths()
            .iter()
            .any(|path| guard.get(&path.split('/').collect::<Vec<_>>()).is_some())
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

    /// Build a minimal filesystem with a two-state program and return its trie.
    fn setup_program_fs() -> (tempfile::TempDir, crate::alphabet::Alphabet) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let reserved = root.join("hard/reserved");
        std::fs::create_dir_all(&reserved).unwrap();
        for door in ["$", "-", "!", "§", "€"] {
            std::fs::File::create(reserved.join(door)).unwrap();
        }

        // programs/metro: currently in state `tick`, moves to `tock`
        // when the leaf condition events/!go exists.
        let prog = root.join("programs/metro");
        std::fs::create_dir_all(prog.join("-state")).unwrap();
        std::fs::File::create(prog.join("-state/tick")).unwrap();
        let cond = prog.join("-states/tick/-transitions/tock/-condition/events");
        std::fs::create_dir_all(&cond).unwrap();
        std::fs::File::create(cond.join("!go")).unwrap();
        // `tock` touches a nested path when entered.
        let touch = prog.join("-states/tock/-action/touch/tmp");
        std::fs::create_dir_all(&touch).unwrap();
        std::fs::File::create(touch.join("beat")).unwrap();

        // events/ scope exists but is empty — must NOT satisfy the condition.
        std::fs::create_dir_all(root.join("events")).unwrap();

        let alphabet = crate::alphabet::Alphabet::load(&reserved).unwrap();
        (dir, alphabet)
    }

    #[test]
    fn test_condition_requires_leaf_not_prefix() {
        let (dir, alphabet) = setup_program_fs();
        let trie = Trie::build(dir.path(), &alphabet).unwrap();
        let trie = Arc::new(RwLock::new(trie));

        // events/ exists but events/!go does not: no transition may fire.
        let effects = reevaluate_programs(&trie);
        assert!(effects.is_empty(), "prefix events/ must not satisfy the condition");

        // Once the leaf events/!go exists, the transition fires.
        trie.write().unwrap().insert(
            &["events".to_string(), "!go".to_string()],
            true,
            &alphabet,
        );
        let effects = reevaluate_programs(&trie);
        let transitioned = effects.iter().any(|e| matches!(e,
            Effect::Touch { path } if path.to_string_lossy() == "programs/metro/-state/tock"
        ));
        assert!(transitioned, "leaf events/!go must trigger the transition");
    }

    #[test]
    fn test_touch_action_supports_deep_paths() {
        let (dir, alphabet) = setup_program_fs();
        let trie = Trie::build(dir.path(), &alphabet).unwrap();
        let trie = Arc::new(RwLock::new(trie));
        trie.write().unwrap().insert(
            &["events".to_string(), "!go".to_string()],
            true,
            &alphabet,
        );

        let effects = reevaluate_programs(&trie);
        let deep_touch = effects.iter().any(|e| matches!(e,
            Effect::Touch { path } if path.to_string_lossy() == "tmp/beat"
        ));
        let dir_touch = effects.iter().any(|e| matches!(e,
            Effect::Touch { path } if path.to_string_lossy() == "tmp"
        ));
        assert!(deep_touch, "-action/touch/tmp/beat must touch tmp/beat");
        assert!(!dir_touch, "the intermediate directory must not be touched");
    }
}

/// Public reevaluation function called from the engine main loop.
/// This is the key piece that makes Programs reactive over time.
pub fn reevaluate_programs(trie: &Arc<RwLock<Trie>>) -> Vec<Effect> {
    let mut effects = Vec::new();
    let guard = trie.read().unwrap();

    let programs_root = match guard.get(&["programs"]) {
        Some(p) => p,
        None => return effects,
    };

    for (prog_name, prog_node) in &programs_root.children {
        if let Some(state_dir) = prog_node.children.get("-state") {
            if state_dir.children.is_empty() {
                continue;
            }

            let current_state = state_dir.children.keys().next().unwrap().clone();

            // Check transitions
            if let Some(states_node) = prog_node.children.get("-states") {
                if let Some(current_state_node) = states_node.children.get(&current_state) {
                    if let Some(transitions) = current_state_node.children.get("-transitions") {
                        for (next_state, trans_node) in &transitions.children {
                            let ready = trans_node
                                .children
                                .get("-condition")
                                .is_some_and(|conditions| {
                                    ProgramsSubsystem::conditions_satisfied(conditions, &guard)
                                });

                            if ready {
                                // Execute transition
                                effects.push(Effect::Remove {
                                    path: PathBuf::from(format!("programs/{}/-state/{}", prog_name, current_state)),
                                });
                                effects.push(Effect::Touch {
                                    path: PathBuf::from(format!("programs/{}/-state/{}", prog_name, next_state)),
                                });
                                effects.push(Effect::Touch {
                                    path: PathBuf::from(format!(
                                        "events/!program_{}_transitioned_{}_to_{}",
                                        prog_name, current_state, next_state
                                    )),
                                });

                                info!("Program {} reactively advanced: {} → {}", prog_name, current_state, next_state);

                                // Run new state actions
                                ProgramsSubsystem::execute_state_actions(prog_node, next_state, &mut effects);

                                break; // one transition per program per pass
                            }
                        }
                    }
                }
            }
        }
    }

    effects
}
