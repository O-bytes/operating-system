/// Subsystem trait and registry.
///
/// Every subsystem is a pure function: (state, event) → Vec<Effect>.
/// Subsystems NEVER write to the filesystem directly — they return effects.
///
/// This makes them highly testable: feed synthetic events, assert effects.

pub mod channels;
pub mod databases;
pub mod events;
pub mod jobs;
pub mod logs;
pub mod programs;
pub mod scheduler;
pub mod states;
pub mod subscriptions;
pub mod workers;

use std::fmt;

use crate::dispatcher::Scope;
use crate::effector::Effect;

/// A filesystem event that has been parsed and classified.
#[derive(Debug, Clone)]
pub struct FsEvent {
    /// What happened.
    pub kind: FsEventKind,
    /// Relative path segments within the OS filesystem.
    pub segments: Vec<String>,
    /// Which scope this event belongs to.
    pub scope: Scope,
}

/// The kind of filesystem event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsEventKind {
    /// A file or directory was created (touch / mkdir).
    Assert,
    /// A file or directory was deleted (rm).
    Retract,
    /// A file or directory was renamed/moved (mv).
    Transform,
}

impl fmt::Display for FsEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Assert => write!(f, "Assert"),
            Self::Retract => write!(f, "Retract"),
            Self::Transform => write!(f, "Transform"),
        }
    }
}

/// Trait for all subsystems.
///
/// A subsystem handles events for a specific scope and returns
/// effects to be executed by the effector.
pub trait Subsystem: Send + Sync {
    /// Which scope this subsystem handles.
    fn scope(&self) -> Scope;

    /// Handle a filesystem event. Returns effects to execute.
    fn handle(&self, event: &FsEvent) -> Vec<Effect>;
}

/// Registry of all active subsystems.
pub struct SubsystemRegistry {
    subsystems: Vec<Box<dyn Subsystem>>,
}

impl SubsystemRegistry {
    pub fn new() -> Self {
        Self {
            subsystems: Vec::new(),
        }
    }

    /// Register a subsystem.
    pub fn register(&mut self, subsystem: Box<dyn Subsystem>) {
        tracing::info!("Registered subsystem for scope: {}", subsystem.scope().name());
        self.subsystems.push(subsystem);
    }

    /// Dispatch an event to the matching subsystem(s).
    /// Returns all effects produced.
    pub fn dispatch(&self, event: &FsEvent) -> Vec<Effect> {
        let mut effects = Vec::new();
        for subsystem in &self.subsystems {
            if subsystem.scope() == event.scope {
                effects.extend(subsystem.handle(event));
            }
        }
        effects
    }

    /// Get the number of registered subsystems.
    pub fn len(&self) -> usize {
        self.subsystems.len()
    }

    pub fn is_empty(&self) -> bool {
        self.subsystems.is_empty()
    }
}

impl Default for SubsystemRegistry {
    fn default() -> Self {
        Self::new()
    }
}
