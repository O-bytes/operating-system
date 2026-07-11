/// Channels subsystem — handles `channels/` scope.
///
/// Channels are ordered message queues for IPC.
/// Structure: `channels/#name/~{seq}/(message)`
///
/// Messages are strictly ordered by their `~seq` segment.
///
/// When a message is asserted:
///   - The subsystem emits a rich, queryable delivery event
///     (`events/!channel_{name}_{seq}`)
///   - This allows programs, workers, and subscriptions to react to IPC traffic
///
/// This makes real, observable message passing possible in 0-bytes.

use std::path::PathBuf;

use tracing::info;

use crate::dispatcher::Scope;
use crate::effector::Effect;
use crate::subsystems::logs::LogsSubsystem;
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
        if event.segments.len() < 3 {
            return Vec::new();
        }

        let channel_name = &event.segments[1]; // e.g. "#main"
        let seq_segment = &event.segments[2];  // e.g. "~0003"

        match event.kind {
            FsEventKind::Assert => {
                // Extract message content
                let content = if event.segments.len() >= 4 {
                    event.segments[3..].join("/")
                } else {
                    String::new()
                };

                info!(
                    "Channel {} posted {}: {}",
                    channel_name, seq_segment, content
                );

                // Make channels real: emit a rich delivery signal that other
                // components can subscribe to or react to.
                let safe_name = channel_name.trim_start_matches('#').to_string();
                let delivery_event = format!("!channel_{}_{}", safe_name, seq_segment);

                vec![
                    LogsSubsystem::log_effect(&format!(
                        "channel {} message {} posted",
                        channel_name, seq_segment
                    )),
                    Effect::Touch {
                        path: PathBuf::from(format!("events/{}", delivery_event)),
                    },
                ]
            }
            FsEventKind::Retract => {
                info!(
                    "Channel {} message {} consumed",
                    channel_name, seq_segment
                );

                let safe_name = channel_name.trim_start_matches('#').to_string();
                let consumed_event = format!("!channel_{}_{}_consumed", safe_name, seq_segment);

                vec![
                    LogsSubsystem::log_effect(&format!(
                        "channel {} message {} consumed",
                        channel_name, seq_segment
                    )),
                    Effect::Touch {
                        path: PathBuf::from(format!("events/{}", consumed_event)),
                    },
                ]
            }
            _ => Vec::new(),
        }
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

        // Channels now emit real delivery events
        let effects = sub.handle(&event);
        assert_eq!(effects.len(), 2); // log + delivery event

        let has_delivery = effects.iter().any(|e| {
            matches!(e, Effect::Touch { path } if path.to_string_lossy().contains("!channel_main_~0001"))
        });
        assert!(has_delivery);
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
        assert_eq!(effects.len(), 2); // log + rich consumed event

        let has_consumed_event = effects.iter().any(|e| {
            matches!(e, Effect::Touch { path } if path.to_string_lossy().contains("!channel_errors_~0001_consumed"))
        });
        assert!(has_consumed_event);
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
