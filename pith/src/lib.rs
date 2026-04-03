//! # Pith — The Rust engine for 0-Bytes OS
//!
//! Pith observes a zero-byte filesystem, interprets it as a living operating system,
//! and exposes it to developers via a Unix socket API.
//!
//! ## Philosophy
//!
//! The filesystem IS the computer. No file ever contains data.
//! All information is encoded in names, paths, existence, and metadata.
//!
//! Four primitives: `touch` (assert), `rm` (retract), `mv` (transform), `mkdir` (allocate).
//!
//! ## Architecture
//!
//! ```text
//! Filesystem → Watcher → Parser → Dispatcher → Subsystems → Effector → Filesystem
//! ```
//!
//! The engine reads `hard/reserved/` at boot to discover the logic door alphabet.
//! The only hardcoded value is `€` (U+20AC) — the escape axiom.

pub mod alphabet;
pub mod api;
pub mod auth;
pub mod boot;
pub mod config;
pub mod dispatcher;
pub mod effector;
pub mod error;
pub mod identity;
pub mod parser;
pub mod permissions;
pub mod session;
pub mod subsystems;
pub mod trie;
pub mod watcher;
