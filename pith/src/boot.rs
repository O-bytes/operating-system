/// Boot and shutdown sequences for the Pith engine.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::signal;
use tracing::info;

use crate::alphabet::Alphabet;
use crate::api;
use crate::config::PithConfig;
use crate::dispatcher::Dispatcher;
use crate::effector::{self, Effect, Effector};
use crate::error::Result;
use crate::permissions::PermissionEngine;
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
    pub permissions: PermissionEngine,
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
        crate::error::PithError::Boot {
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
    let trie = Trie::build(&fs_root, &alphabet)?;
    let node_count = trie.total_nodes();

    // 3. Load permissions.
    let permissions = PermissionEngine::load(&trie);

    // 4. Clean tmp/.
    effector::clean_tmp(&fs_root).await?;

    // 5. Wrap trie in Arc<RwLock> for shared access.
    let trie = Arc::new(RwLock::new(trie));

    // 6. Create effector.
    let effector = Effector::new(fs_root.clone());

    // 7. Create dispatcher.
    let dispatcher = Dispatcher::new(Arc::clone(&trie), alphabet.clone());

    // 8. Register subsystems.
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

    // 9. Start filesystem watcher.
    let watcher = FsWatcher::start(&fs_root, effector.clone())?;

    // 10. Signal boot.
    effector
        .execute(&Effect::Touch {
            path: PathBuf::from("events/!boot"),
        })
        .await?;

    // 11. Log boot event.
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
        effector,
        dispatcher,
        watcher,
        subsystems,
        config: config.clone(),
    })
}

/// Run the engine: start API server + event loop, block until Ctrl+C.
pub async fn run(engine: &PithEngine) -> Result<()> {
    // Start the API server.
    let _api_handle = api::start_server(
        &engine.config.socket_path,
        Arc::clone(&engine.trie),
        engine.effector.clone(),
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

    // 3. Clean tmp/.
    effector::clean_tmp(&engine.config.fs_root).await?;

    // 4. Remove signals.
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

    // 5. Remove socket.
    api::cleanup_socket(&engine.config.socket_path);

    info!("Pith engine stopped.");
    Ok(())
}
