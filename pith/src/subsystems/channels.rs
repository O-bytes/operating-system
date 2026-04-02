/// Channels subsystem — handles `channels/` scope.
///
/// Channels are ordered message queues for IPC.
/// Structure: `channels/#name/~{seq}/(message)`
///
/// When a message is asserted (touched), the subsystem:
///   - Logs the message receipt
///   - In future phases: notifies channel subscribers, validates sequence

use tracing::info;

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::{FsEvent, FsEventKind, Subsystem};

pub struct ChannelsSubsystem;

impl ChannelsSubsystem {
    pub fn new() -> Self {
        Self
    }
}

impl Subsystem for ChannelsSubsystem {
    fn scope(&self) -> Scope {
        Scope::Channels
    }

    fn handle(&self, event: &FsEvent) -> Vec<Effect> {
        // channels/#name/~seq/(message)
        // segments: ["channels", "#name", "~0001", "(hello)"]
        if event.segments.len() < 3 {
            return Vec::new();
        }

        let channel_name = &event.segments[1];
        let msg_segment = if event.segments.len() >= 3 {
            &event.segments[2]
        } else {
            return Vec::new();
        };

        match event.kind {
            FsEventKind::Assert => {
                // Extract message content from deeper segments if present.
                let content = if event.segments.len() >= 4 {
                    event.segments[3..].join("/")
                } else {
                    String::new()
                };

                info!(
                    "Channel {} message {}: {}",
                    channel_name, msg_segment, content
                );

                // TODO: notify channel subscribers, validate ordering
            }
            FsEventKind::Retract => {
                info!(
                    "Channel {} message consumed: {}",
                    channel_name, msg_segment
                );
            }
            _ => {}
        }

        // No effects for now — channels are passive receivers.
        // Future: auto-sequence, notify subscribers.
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_message_assert() {
        let sub = ChannelsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "channels".to_string(),
                "#main".to_string(),
                "~0001".to_string(),
                "(hello world)".to_string(),
            ],
            scope: Scope::Channels,
        };

        // No effects yet (passive), but should not panic.
        let effects = sub.handle(&event);
        assert!(effects.is_empty());
    }

    #[test]
    fn test_channel_message_retract() {
        let sub = ChannelsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Retract,
            segments: vec![
                "channels".to_string(),
                "#errors".to_string(),
                "~0001".to_string(),
            ],
            scope: Scope::Channels,
        };

        let effects = sub.handle(&event);
        assert!(effects.is_empty());
    }

    #[test]
    fn test_short_path_ignored() {
        let sub = ChannelsSubsystem::new();
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["channels".to_string()],
            scope: Scope::Channels,
        };
        assert!(sub.handle(&event).is_empty());
    }
}
