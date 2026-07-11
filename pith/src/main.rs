use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::info;
use tracing_subscriber::EnvFilter;

use pith::api::protocol::Response;
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

            println!();
            println!("  ✓ Pith is running");
            println!("    Socket:  {}", config.socket_path.display());
            println!("    Root:    {}", config.fs_root.display());
            println!();
            println!("  Try:  pith status");
            println!("        pith stop");
            println!();

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
            match query_status(&socket).await {
                Ok(resp) => {
                    if resp.ok {
                        if let Some(data) = resp.data {
                            println!("Pith is running (socket: {})", socket.display());
                            println!();

                            if let Some(v) = data.get("nodes") {
                                println!("  {:<14} {}", "nodes:", v);
                            }
                            if let Some(v) = data.get("identities") {
                                println!("  {:<14} {}", "identities:", v);
                            }
                            if let Some(v) = data.get("groups") {
                                println!("  {:<14} {}", "groups:", v);
                            }
                            if let Some(v) = data.get("logic_doors") {
                                println!("  {:<14} {}", "logic doors:", v);
                            }
                            if let Some(status) = data.get("status") {
                                println!("  {:<14} {}", "status:", status);
                            }
                        } else {
                            println!("Pith is running (socket: {})", socket.display());
                        }
                    } else {
                        eprintln!("Status request failed: {}", resp.error.unwrap_or_default());
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to connect to Pith at {}: {}", socket.display(), e);
                    eprintln!("Is the engine running? Try: pith start --socket {}", socket.display());
                    std::process::exit(1);
                }
            }
        }

        Commands::Stop { socket } => {
            println!("Sending shutdown signal to Pith at {}...", socket.display());

            match send_api_request(&socket, "touch", "events/!shutdown", None).await {
                Ok(resp) => {
                    if resp.ok {
                        println!("✓ Shutdown signal sent. The engine will exit gracefully.");
                    } else {
                        eprintln!("Failed to send shutdown: {}", resp.error.unwrap_or_default());
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to connect to Pith: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}

/// Send a JSON request to the Pith Unix socket API and return the Response.
async fn send_api_request(
    socket: &PathBuf,
    op: &str,
    path: &str,
    args: Option<serde_json::Value>,
) -> Result<Response, Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect(socket).await?;

    let mut req = serde_json::json!({
        "op": op,
        "path": path,
    });
    if let Some(a) = args {
        req["args"] = a;
    }

    let line = format!("{}\n", req);
    stream.write_all(line.as_bytes()).await?;

    let (reader, _) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let mut response_line = String::new();
    reader.read_line(&mut response_line).await?;

    let response: Response = serde_json::from_str(response_line.trim())?;
    Ok(response)
}

/// Convenience wrapper for the status command.
async fn query_status(socket: &PathBuf) -> Result<Response, Box<dyn std::error::Error>> {
    send_api_request(socket, "status", "", None).await
}
