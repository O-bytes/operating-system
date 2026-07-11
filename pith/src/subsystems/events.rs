/// Events subsystem — handles `events/` scope.
///
/// Signals are fire-and-forget: `!signal_name` files.
/// When a `!` signal is asserted, the subsystem records it in `-history/`.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tracing::{debug, info};

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};
use crate::trie::Trie;

pub struct EventsSubsystem {
    /// Optional shared trie for looking up subscribers when events fire.
    trie: Option<Arc<RwLock<Trie>>>,
}

impl EventsSubsystem {
    pub fn new() -> Self {
        Self { trie: None }
    }

    /// Create an EventsSubsystem that can notify subscribers by inspecting the trie.
    pub fn new_with_trie(trie: Arc<RwLock<Trie>>) -> Self {
        Self { trie: Some(trie) }
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

                // Notify subscribers if we have trie access.
                if let Some(ref _trie) = self.trie {
                    let notifications = self.collect_subscriber_notifications(signal_name, &timestamp.to_string());
                    effects.extend(notifications);
                } else {
                    debug!("EventsSubsystem has no trie – skipping subscriber notifications for {}", signal_name);
                }
            }
            FsEventKind::Retract => {
                info!("Signal retracted: {}", signal_name);
            }
            _ => {}
        }

        effects
    }
}

impl EventsSubsystem {
    /// Find all identities subscribed to this exact signal and produce notification effects.
    /// Creates observable notification leaves under each subscription:
    ///   subscriptions/{id}/events/!signal/-fired/{timestamp}
    fn collect_subscriber_notifications(&self, signal_name: &str, timestamp: &str) -> Vec<Effect> {
        let mut effects = Vec::new();

        let trie = match &self.trie {
            Some(t) => t,
            None => return effects,
        };

        let guard = trie.read().unwrap();

        // Look for subscriptions root
        let subs_root = match guard.get(&["subscriptions"]) {
            Some(node) => node,
            None => {
                debug!("No subscriptions root in trie");
                return effects;
            }
        };

        for (identity_id, identity_node) in &subs_root.children {
            // Check if this identity has a subscription for this exact signal
            // Path: subscriptions/{id}/events/!signal
            if let Some(events_node) = identity_node.children.get("events") {
                if events_node.children.get(signal_name).is_some() {
                    // Create a notification leaf
                    let notif_path = format!(
                        "subscriptions/{}/events/{}/-fired/{}",
                        identity_id, signal_name, timestamp
                    );
                    effects.push(Effect::Touch {
                        path: PathBuf::from(notif_path),
                    });

                    debug!("Notifying identity {} about signal !{}", identity_id, signal_name);
                }
            }
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

    #[tokio::test]
    async fn test_event_notifies_matching_subscribers() {
        use tempfile::TempDir;

        use crate::alphabet::Alphabet;
        use crate::effector::Effector;
        use crate::trie::Trie;

        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Minimal alphabet
        let reserved = root.join("hard/reserved");
        std::fs::create_dir_all(&reserved).unwrap();
        for ch in ['$', '-', '!', '€'] {
            std::fs::File::create(reserved.join(ch.to_string())).unwrap();
        }
        let alphabet = Alphabet::load(&reserved).unwrap();

        // Create subscription for identity 001 watching !heartbeat
        let sub_path = root.join("subscriptions/001/events/!heartbeat");
        std::fs::create_dir_all(&sub_path).unwrap();
        std::fs::File::create(sub_path.join("subscription")).unwrap(); // marker file

        // Build trie
        let trie = Trie::build(root, &alphabet).unwrap();
        let trie = Arc::new(RwLock::new(trie));

        // Create subsystem with trie
        let events_sub = EventsSubsystem::new_with_trie(Arc::clone(&trie));
        let effector = Effector::new(root.to_path_buf());

        // Simulate !heartbeat signal
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["events".to_string(), "!heartbeat".to_string()],
            scope: Scope::Events,
        };

        let effects = events_sub.handle(&event);

        // We expect at least history + one notification
        assert!(effects.len() >= 2);



        // Execute the effects so the notification file is actually created
        for eff in &effects {
            let _ = effector.execute(eff).await;
        }

        // Verify notification was created
        let notif_dir = root.join("subscriptions/001/events/!heartbeat/-fired");
        assert!(notif_dir.exists(), "Notification directory should have been created");

        let entries: Vec<_> = std::fs::read_dir(&notif_dir).unwrap().collect();
        assert!(!entries.is_empty(), "At least one notification timestamp should exist");
    }
}
