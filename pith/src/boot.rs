/// Boot and shutdown sequences for the Pith engine.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::signal;
use tracing::info;

use crate::alphabet::Alphabet;
use crate::api;
use crate::auth;
use crate::config::PithConfig;
use crate::dispatcher::Dispatcher;
use crate::effector::{self, Effect, Effector};
use crate::error::{PithError, Result};
use crate::parser::NodeClass;
use crate::permissions::PermissionEngine;
use crate::session::{self, SessionManager};
use crate::subsystems::channels::ChannelsSubsystem;
use crate::subsystems::events::EventsSubsystem;
use crate::subsystems::jobs::JobsSubsystem;
use crate::subsystems::logs::LogsSubsystem;
use crate::subsystems::states::StatesSubsystem;
use crate::subsystems::workers::WorkersSubsystem;
use crate::subsystems::scheduler::SchedulerSubsystem;
use crate::subsystems::programs::ProgramsSubsystem;
use crate::subsystems::databases::DatabasesSubsystem;
use crate::subsystems::subscriptions::SubscriptionsSubsystem;
use crate::subsystems::SubsystemRegistry;
use crate::trie::Trie;
use crate::watcher::FsWatcher;

/// The running Pith engine.
pub struct PithEngine {
    pub alphabet: Alphabet,
    pub trie: Arc<RwLock<Trie>>,
    pub permissions: Arc<PermissionEngine>,
    pub session_manager: Arc<SessionManager>,
    pub effector: Effector,
    pub dispatcher: Dispatcher,
    pub watcher: FsWatcher,
    pub subsystems: SubsystemRegistry,
    pub config: PithConfig,
}

/// Boot the Pith engine.
pub async fn boot(config: &PithConfig) -> Result<PithEngine> {
    // Canonicalize fs_root to absolute path (required for FSEvents on macOS).
    let fs_root = config.fs_root.canonicalize().map_err(|e| {
        PithError::Boot {
            reason: format!("Cannot resolve fs_root {}: {}", config.fs_root.display(), e),
        }
    })?;
    let config = PithConfig {
        fs_root: fs_root.clone(),
        ..config.clone()
    };

    info!("Pith engine booting from {}", fs_root.display());

    // 1. Load the self-describing alphabet.
    let reserved_dir = fs_root.join("hard/reserved");
    let alphabet = Alphabet::load(&reserved_dir)?;

    // 2. Build the in-memory trie index.
    let mut trie = Trie::build(&fs_root, &alphabet)?;

    // 2.5. First-boot check: ensure at least one Omni identity has a password.
    let fs_modified = ensure_admin_password(&trie, &fs_root)?;
    if fs_modified {
        // Rebuild trie to pick up newly created files.
        trie = Trie::build(&fs_root, &alphabet)?;
    }

    let node_count = trie.total_nodes();

    // 3. Load permissions.
    let permissions = Arc::new(PermissionEngine::load(&trie));

    // 4. Load session manager (UID→identity mapping from trie).
    let session_manager = Arc::new(SessionManager::load(&trie));

    // 5. Clean tmp/ and sessions/ (orphans from previous run).
    effector::clean_tmp(&fs_root).await?;
    session::clean_sessions(&fs_root).await?;

    // 6. Wrap trie in Arc<RwLock> for shared access.
    let trie = Arc::new(RwLock::new(trie));

    // 7. Create effector.
    let effector = Effector::new(fs_root.clone());

    // 8. Create dispatcher.
    let dispatcher = Dispatcher::new(Arc::clone(&trie), alphabet.clone());

    // 9. Register subsystems.
    let mut subsystems = SubsystemRegistry::new();
    subsystems.register(Box::new(EventsSubsystem::new()));
    subsystems.register(Box::new(ChannelsSubsystem::new()));
    subsystems.register(Box::new(LogsSubsystem::new()));
    subsystems.register(Box::new(StatesSubsystem::new()));
    subsystems.register(Box::new(JobsSubsystem::new()));
    subsystems.register(Box::new(WorkersSubsystem::new()));
    subsystems.register(Box::new(SchedulerSubsystem::new()));
    subsystems.register(Box::new(ProgramsSubsystem::new()));
    subsystems.register(Box::new(DatabasesSubsystem::new()));
    subsystems.register(Box::new(SubscriptionsSubsystem::new()));

    // 10. Start filesystem watcher.
    let watcher = FsWatcher::start(&fs_root, effector.clone())?;

    // 11. Signal boot.
    effector
        .execute(&Effect::Touch {
            path: PathBuf::from("events/!boot"),
        })
        .await?;

    // 12. Log boot event.
    effector
        .execute(&LogsSubsystem::log_effect("pith boot complete"))
        .await?;

    info!(
        "Boot complete: {} logic doors, {} nodes, {} identities, {} groups, {} subsystems",
        alphabet.len(),
        node_count,
        permissions.identity_count(),
        permissions.group_count(),
        subsystems.len(),
    );

    Ok(PithEngine {
        alphabet,
        trie,
        permissions,
        session_manager,
        effector,
        dispatcher,
        watcher,
        subsystems,
        config: config.clone(),
    })
}

/// Check if any Omni-tier (0xx) identity has a password (`-secret`).
/// If none does, prompt interactively and create one for identity 001.
///
/// Returns `true` if filesystem was modified (trie rebuild needed).
///
/// Uses raw `std::fs` because the effector doesn't exist yet during boot.
fn ensure_admin_password(trie: &Trie, fs_root: &Path) -> Result<bool> {
    if has_admin_with_password(trie) {
        return Ok(false); // Normal boot — at least one Omni has a password.
    }

    // First boot: no Omni identity has a password.
    if !auth::is_interactive() {
        return Err(PithError::Boot {
            reason: "No admin identity with a password found. \
                     Run `pith init` first, or start from an interactive terminal."
                .to_string(),
        });
    }

    let password = auth::prompt_password_interactive("First Boot")?;
    provision_admin_identity(fs_root, &password)?;

    info!("Admin identity 001 created with password.");
    Ok(true)
}

/// Check if any 0xx identity in the trie has a `-secret` child with content.
pub fn has_admin_with_password(trie: &Trie) -> bool {
    let ids_node = match trie.get(&["hard", "identities"]) {
        Some(node) => node,
        None => return false,
    };

    ids_node.children.iter().any(|(name, node)| {
        // Only check 0xx (Omni) tier identities.
        if !name.starts_with('0') {
            return false;
        }
        // Check for -secret instruction child with at least one hash file.
        node.children.values().any(|child| {
            matches!(
                &child.class,
                NodeClass::Instruction { door: '-', arg } if arg == "secret"
            ) && !child.children.is_empty()
        })
    })
}

/// Create the admin identity 001 on disk with a password and full permissions.
///
/// Creates:
///   hard/identities/001/-expected/type/identity
///   hard/identities/001/-name/admin
///   hard/identities/001/-secret/{encoded_hash}
///   hard/identities/001/§read/_
///   hard/identities/001/§write/_
///   hard/identities/001/§execute/_
///   hard/identities/001/§own/_
pub fn provision_admin_identity(fs_root: &Path, password: &str) -> Result<()> {
    let base = fs_root.join("hard/identities/001");

    let phc_hash = auth::hash_password(password)?;
    let hash_filename = auth::hash_to_filename(&phc_hash);

    // Helper to create dir + touch file.
    let touch = |relative: &str| -> Result<()> {
        let path = base.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| PithError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        std::fs::File::create(&path).map_err(|e| PithError::Io {
            path: path.clone(),
            source: e,
        })?;
        Ok(())
    };

    // Type declaration.
    touch("-expected/type/identity")?;

    // Name.
    touch("-name/admin")?;

    // Password hash (encoded in filename).
    touch(&format!("-secret/{}", hash_filename))?;

    // Full permissions: read, write, execute, own on everything.
    touch("§read/_")?;
    touch("§write/_")?;
    touch("§execute/_")?;
    touch("§own/_")?;

    info!(
        "Provisioned admin identity 001 at {}",
        base.display()
    );
    Ok(())
}

/// Run the engine: start API server + event loop, block until Ctrl+C.
pub async fn run(engine: &PithEngine) -> Result<()> {
    // Start the API server.
    let _api_handle = api::start_server(
        &engine.config.socket_path,
        Arc::clone(&engine.trie),
        engine.effector.clone(),
        Arc::clone(&engine.session_manager),
        Arc::clone(&engine.permissions),
        engine.config.enforce_permissions,
    )
    .await?;

    info!("Pith event loop started — press Ctrl+C to stop");

    let mut event_count: u64 = 0;
    let poll_interval = Duration::from_millis(50);

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("Shutdown signal received (Ctrl+C)");
                break;
            }
            _ = tokio::time::sleep(poll_interval) => {
                while let Some(watch_event) = engine.watcher.poll(Duration::from_millis(1)) {
                    if let Some(fs_event) = engine.dispatcher.dispatch(&watch_event) {
                        event_count += 1;
                        let effects = engine.subsystems.dispatch(&fs_event);
                        for effect in &effects {
                            if let Err(e) = engine.effector.execute(effect).await {
                                tracing::error!("Effect failed: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    // Drain remaining events.
    info!("Draining remaining events...");
    for watch_event in engine.watcher.drain() {
        if let Some(fs_event) = engine.dispatcher.dispatch(&watch_event) {
            event_count += 1;
            let effects = engine.subsystems.dispatch(&fs_event);
            for effect in &effects {
                let _ = engine.effector.execute(effect).await;
            }
        }
    }

    info!("Event loop stopped after {} events", event_count);
    Ok(())
}

/// Graceful shutdown.
pub async fn shutdown(engine: &PithEngine) -> Result<()> {
    info!("Shutting down Pith engine...");

    // 1. Signal shutdown.
    let _ = engine
        .effector
        .execute(&Effect::Touch {
            path: PathBuf::from("events/!shutdown"),
        })
        .await;

    // 2. Log shutdown.
    let _ = engine
        .effector
        .execute(&LogsSubsystem::log_effect("pith shutdown"))
        .await;

    // 3. Clean up all active sessions.
    let active_ids = engine.session_manager.active_session_ids();
    for session_id in active_ids {
        if let Some(session) = engine.session_manager.destroy_session(session_id) {
            let effects = SessionManager::session_effects_destroy(&session);
            for effect in &effects {
                let _ = engine.effector.execute(effect).await;
            }
        }
    }

    // 4. Clean tmp/ and sessions/.
    effector::clean_tmp(&engine.config.fs_root).await?;
    session::clean_sessions(&engine.config.fs_root).await?;

    // 5. Remove signals.
    for signal in &["events/!boot", "events/!shutdown"] {
        let full = engine.config.fs_root.join(signal);
        if full.exists() {
            let _ = engine
                .effector
                .execute(&Effect::Remove {
                    path: PathBuf::from(*signal),
                })
                .await;
        }
    }

    // 6. Remove socket.
    api::cleanup_socket(&engine.config.socket_path);

    info!("Pith engine stopped.");
    Ok(())
}
