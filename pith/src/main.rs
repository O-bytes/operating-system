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

    /// Initialize the filesystem: create the admin identity (001) with a password.
    ///
    /// Run this before `pith start` on a fresh filesystem, or use it for
    /// non-interactive provisioning in CI/Docker with `--password`.
    Init {
        /// Path to the 0-bytes filesystem root.
        #[arg(short, long, default_value = "../src")]
        root: PathBuf,

        /// Admin password (if omitted, prompts interactively).
        #[arg(short, long)]
        password: Option<String>,

        /// Log level (trace, debug, info, warn, error).
        #[arg(short, long, default_value = "info")]
        log_level: String,
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

        Commands::Init {
            root,
            password,
            log_level,
        } => {
            // Initialize tracing.
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| EnvFilter::new(&log_level)),
                )
                .init();

            // Canonicalize fs_root.
            let fs_root = root.canonicalize().map_err(|e| {
                format!("Cannot resolve root {}: {}", root.display(), e)
            })?;

            // Load alphabet and trie to check current state.
            let reserved_dir = fs_root.join("hard/reserved");
            let alphabet = pith::alphabet::Alphabet::load(&reserved_dir)?;
            let trie = pith::trie::Trie::build(&fs_root, &alphabet)?;

            if boot::has_admin_with_password(&trie) {
                println!("Admin identity with a password already exists. Nothing to do.");
                return Ok(());
            }

            // Get the password (from flag or interactive prompt).
            let pwd = match password {
                Some(p) => {
                    if p.len() < 8 {
                        eprintln!("Error: password must be at least 8 characters.");
                        std::process::exit(1);
                    }
                    p
                }
                None => {
                    if !pith::auth::is_interactive() {
                        eprintln!("Error: no --password provided and stdin is not a terminal.");
                        eprintln!("Usage: pith init --root <path> --password <pwd>");
                        std::process::exit(1);
                    }
                    pith::auth::prompt_password_interactive("Init")?
                }
            };

            // Provision admin identity on disk.
            boot::provision_admin_identity(&fs_root, &pwd)?;

            println!("✓ Admin identity 001 created successfully.");
            println!("  Filesystem: {}", fs_root.join("hard/identities/001").display());
            println!("  You can now run `pith start`.");
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
