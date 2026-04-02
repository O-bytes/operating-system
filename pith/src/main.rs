use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

use pith::boot;
use pith::config::PithConfig;

#[derive(Parser)]
#[command(
    name = "pith",
    about = "Pith — The Rust engine for 0-Bytes OS",
    long_about = "Observes a zero-byte filesystem, interprets it as a living operating system,\nand exposes it to developers via a Unix socket API.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Pith engine.
    Start {
        /// Path to the 0-bytes filesystem root (the `src/` directory).
        #[arg(short, long, default_value = "../src")]
        root: PathBuf,

        /// Path for the Unix domain socket API.
        #[arg(short, long, default_value = "/tmp/pith.sock")]
        socket: PathBuf,

        /// Log level (trace, debug, info, warn, error).
        #[arg(short, long, default_value = "info")]
        log_level: String,

        /// Enforce permissions on raw filesystem changes.
        #[arg(long, default_value_t = false)]
        enforce: bool,
    },

    /// Show engine status.
    Status {
        /// Path for the Unix domain socket API.
        #[arg(short, long, default_value = "/tmp/pith.sock")]
        socket: PathBuf,
    },

    /// Stop the running engine.
    Stop {
        /// Path for the Unix domain socket API.
        #[arg(short, long, default_value = "/tmp/pith.sock")]
        socket: PathBuf,
    },
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start {
            root,
            socket,
            log_level,
            enforce,
        } => {
            // Initialize tracing.
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| EnvFilter::new(&log_level)),
                )
                .init();

            let config = PithConfig {
                fs_root: root,
                socket_path: socket,
                log_level,
                enforce_permissions: enforce,
            };

            info!("Starting Pith engine...");

            let engine = boot::boot(&config).await?;

            info!(
                "Pith is alive: {} logic doors, {} identities, {} groups",
                engine.alphabet.len(),
                engine.permissions.identity_count(),
                engine.permissions.group_count(),
            );

            // Run the event loop until Ctrl+C.
            boot::run(&engine).await?;

            // Graceful shutdown.
            boot::shutdown(&engine).await?;
        }

        Commands::Status { socket } => {
            // TODO: Phase 7 — connect to socket, query status
            println!("Status check not yet implemented (socket: {})", socket.display());
        }

        Commands::Stop { socket } => {
            // TODO: Phase 7 — connect to socket, send shutdown
            println!("Stop not yet implemented (socket: {})", socket.display());
        }
    }

    Ok(())
}
