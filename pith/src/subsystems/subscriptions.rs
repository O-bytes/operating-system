/// Subscriptions subsystem — handles `subscriptions/` scope.
///
/// Each identity can subscribe to events. The subscription path mirrors
/// the event it watches:
///
///   subscriptions/{identity_id}/events/!boot     — watch boot event
///   subscriptions/{identity_id}/jobs/1/!completed — watch job 1 completion
///
/// When an event fires, the events subsystem (in future phases) checks
/// subscriptions and notifies matching identities.

use tracing::info;

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::logs::LogsSubsystem;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};

pub struct SubscriptionsSubsystem;

impl SubscriptionsSubsystem {
    pub fn new() -> Self {
        Self
    }
}

impl Subsystem for SubscriptionsSubsystem {
    fn scope(&self) -> Scope {
        Scope::Subscriptions
    }

    fn handle(&self, event: &FsEvent) -> Vec<Effect> {
        let mut effects = Vec::new();

        if event.segments.len() < 3 {
            return effects;
        }

        let identity_id = &event.segments[1];
        let watch_path = event.segments[2..].join("/");

        match event.kind {
            FsEventKind::Assert => {
                info!(
                    "Subscription: identity {} watching {}",
                    identity_id, watch_path
                );
                effects.push(LogsSubsystem::log_effect(
                    &format!("subscribe {} → {}", identity_id, watch_path),
                ));
            }
            FsEventKind::Retract => {
                info!(
                    "Unsubscribe: identity {} from {}",
                    identity_id, watch_path
                );
                effects.push(LogsSubsystem::log_effect(
                    &format!("unsubscribe {} → {}", identity_id, watch_path),
                ));
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
    fn test_subscription_created() {
        let sub = SubscriptionsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "subscriptions".to_string(),
                "001".to_string(),
                "events".to_string(),
                "!boot".to_string(),
            ],
            scope: Scope::Subscriptions,
        };

        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 1); // log
        match &effects[0] {
            Effect::Touch { path } => {
                let s = path.to_string_lossy();
                assert!(s.contains("subscribe 001"));
                assert!(s.contains("events/!boot"));
            }
            _ => panic!("Expected log Touch"),
        }
    }

    #[test]
    fn test_unsubscription() {
        let sub = SubscriptionsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Retract,
            segments: vec![
                "subscriptions".to_string(),
                "042".to_string(),
                "jobs".to_string(),
                "1".to_string(),
                "!completed".to_string(),
            ],
            scope: Scope::Subscriptions,
        };

        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn test_short_path_ignored() {
        let sub = SubscriptionsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["subscriptions".to_string()],
            scope: Scope::Subscriptions,
        };
        assert!(sub.handle(&event).is_empty());
    }
}
