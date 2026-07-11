//! Stress test and benchmark runner for 0-Bytes OS.
//!
//! Builds a temporary filesystem, boots Pith, and hammers it with operations
//! measuring throughput, latency, and correctness under load.
//!
//! Usage:
//!   cargo run --bin stress
//!   cargo run --bin stress -- --jobs 1000 --workers 50 --api-clients 10

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use tokio::net::UnixStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "stress", about = "0-Bytes OS stress test & benchmark")]
struct Args {
    /// Number of jobs to create in the filesystem throughput test.
    #[arg(long, default_value_t = 500)]
    jobs: usize,

    /// Number of workers to register.
    #[arg(long, default_value_t = 20)]
    workers: usize,

    /// Number of identities to create.
    #[arg(long, default_value_t = 200)]
    identities: usize,

    /// Number of concurrent API clients for the API stress test.
    #[arg(long, default_value_t = 5)]
    api_clients: usize,

    /// Number of API requests per client.
    #[arg(long, default_value_t = 200)]
    api_requests: usize,

    /// Database depth (number of nested categories).
    #[arg(long, default_value_t = 50)]
    db_entries: usize,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct Timer {
    label: String,
    start: Instant,
}

impl Timer {
    fn start(label: &str) -> Self {
        Self {
            label: label.to_string(),
            start: Instant::now(),
        }
    }

    fn stop(self) -> Duration {
        let elapsed = self.start.elapsed();
        elapsed
    }

    fn stop_print(self, count: usize) -> Duration {
        let elapsed = self.start.elapsed();
        let per_op = if count > 0 {
            elapsed / count as u32
        } else {
            elapsed
        };
        let ops_per_sec = if elapsed.as_secs_f64() > 0.0 {
            count as f64 / elapsed.as_secs_f64()
        } else {
            f64::INFINITY
        };
        println!(
            "  {:<45} {:>8.2}ms  {:>10} ops  {:>10.0} ops/s  {:>8.1}us/op",
            self.label,
            elapsed.as_secs_f64() * 1000.0,
            count,
            ops_per_sec,
            per_op.as_secs_f64() * 1_000_000.0,
        );
        elapsed
    }
}

fn create_reserved(root: &std::path::Path) {
    let reserved = root.join("hard/reserved");
    std::fs::create_dir_all(&reserved).unwrap();
    let chars = [
        '€', '$', '[', ']', '|', ',', '-', '*', '+', '{', '}', '(', ')', '@', '~', ':',
        '#', '!', '?', '^', '&', '%', '<', '>', '=', ';', '_', '§', '¶', '∂', 'λ', '∴',
        '∵', '∞', '▶', '⏸', '⏹', '⌚',
    ];
    for ch in chars {
        std::fs::File::create(reserved.join(ch.to_string())).unwrap();
    }
}

fn create_scopes(root: &std::path::Path) {
    for scope in [
        "states", "jobs", "workers", "channels", "events", "programs",
        "schedules", "sessions", "subscriptions", "logs", "tmp", "databases",
    ] {
        std::fs::create_dir_all(root.join(scope)).unwrap();
    }
    std::fs::File::create(root.join("states/0")).unwrap();
    std::fs::File::create(root.join("workers/0")).unwrap();
    std::fs::File::create(root.join("jobs/0")).unwrap();

    // Groups
    for (group, rules) in [
        ("system", vec![("read", "_"), ("write", "_"), ("execute", "_")]),
        ("developers", vec![("read", "databases"), ("write", "jobs"), ("execute", "workers")]),
        ("guests", vec![("read", "databases"), ("deny", "hard")]),
    ] {
        for (verb, target) in rules {
            let p = root.join(format!("hard/groups/{}/§{}/{}", group, verb, target));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::File::create(&p).unwrap();
        }
    }

    // Types
    let types_dir = root.join("hard/types");
    std::fs::create_dir_all(&types_dir).unwrap();
    for t in ["identity", "job", "worker", "program", "channel", "event", "database", "schema"] {
        std::fs::File::create(types_dir.join(t)).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Test 1: Filesystem write throughput (raw touch/mkdir/rm)
// ---------------------------------------------------------------------------

fn test_fs_write_throughput(root: &std::path::Path, n_jobs: usize) {
    println!("\n--- Filesystem Write Throughput (raw) ---");

    // Touch N files
    let t = Timer::start("touch (create 0-byte files)");
    for i in 0..n_jobs {
        let path = root.join(format!("tmp/bench_{}", i));
        std::fs::File::create(&path).unwrap();
    }
    t.stop_print(n_jobs);

    // Stat N files (read metadata)
    let t = Timer::start("stat (read metadata)");
    for i in 0..n_jobs {
        let path = root.join(format!("tmp/bench_{}", i));
        let _ = std::fs::metadata(&path).unwrap();
    }
    t.stop_print(n_jobs);

    // Rename N files
    let t = Timer::start("mv (rename files)");
    for i in 0..n_jobs {
        let from = root.join(format!("tmp/bench_{}", i));
        let to = root.join(format!("tmp/benchmv_{}", i));
        std::fs::rename(&from, &to).unwrap();
    }
    t.stop_print(n_jobs);

    // Remove N files
    let t = Timer::start("rm (delete files)");
    for i in 0..n_jobs {
        let path = root.join(format!("tmp/benchmv_{}", i));
        std::fs::remove_file(&path).unwrap();
    }
    t.stop_print(n_jobs);

    // Mkdir nested
    let t = Timer::start("mkdir -p (create nested dirs)");
    for i in 0..n_jobs {
        let path = root.join(format!("tmp/dir_{}/a/b/c", i));
        std::fs::create_dir_all(&path).unwrap();
    }
    t.stop_print(n_jobs);

    // Cleanup
    let _ = std::fs::remove_dir_all(root.join("tmp"));
    std::fs::create_dir_all(root.join("tmp")).unwrap();
}

// ---------------------------------------------------------------------------
// Test 2: Identity & permission creation throughput
// ---------------------------------------------------------------------------

fn test_identity_creation(root: &std::path::Path, n_identities: usize) {
    println!("\n--- Identity Creation Throughput ---");

    let t = Timer::start("create identities (dir + type + group)");
    for i in 1..=n_identities {
        let id = format!("{:04}", i);
        let id_dir = root.join(format!("hard/identities/{}", id));
        let type_dir = id_dir.join("-expected/type");
        std::fs::create_dir_all(&type_dir).unwrap();
        std::fs::File::create(type_dir.join("identity")).unwrap();
        let group_dir = id_dir.join("-group");
        std::fs::create_dir_all(&group_dir).unwrap();
        let group = if i <= 10 { "system" } else if i <= 100 { "developers" } else { "guests" };
        std::fs::File::create(group_dir.join(group)).unwrap();
    }
    t.stop_print(n_identities);
}

// ---------------------------------------------------------------------------
// Test 3: Trie build performance
// ---------------------------------------------------------------------------

fn test_trie_build(root: &std::path::Path) {
    println!("\n--- Trie Build Performance ---");

    let reserved = root.join("hard/reserved");
    let alphabet = pith::alphabet::Alphabet::load(&reserved).unwrap();

    let t = Timer::start("trie build (full filesystem walk)");
    let trie = pith::trie::Trie::build(root, &alphabet).unwrap();
    let elapsed = t.stop();
    let nodes = trie.total_nodes();
    println!(
        "  {:<45} {:>8.2}ms  {:>10} nodes",
        "trie build",
        elapsed.as_secs_f64() * 1000.0,
        nodes,
    );

    // Lookup throughput
    let t = Timer::start("trie lookup (random paths, 10k ops)");
    let n = 10_000;
    for i in 0..n {
        let id = format!("{:04}", (i % 200) + 1);
        let _ = trie.get(&["hard", "identities", &id, "-expected", "type", "identity"]);
    }
    t.stop_print(n);

    // List throughput
    let t = Timer::start("trie list (identities children)");
    let n = 10_000;
    for _ in 0..n {
        let _ = trie.list(&["hard", "identities"]);
    }
    t.stop_print(n);
}

// ---------------------------------------------------------------------------
// Test 4: Permission resolution throughput
// ---------------------------------------------------------------------------

fn test_permission_resolution(root: &std::path::Path) {
    println!("\n--- Permission Resolution Throughput ---");

    let reserved = root.join("hard/reserved");
    let alphabet = pith::alphabet::Alphabet::load(&reserved).unwrap();
    let trie = pith::trie::Trie::build(root, &alphabet).unwrap();

    let t = Timer::start("permission engine load");
    let engine = pith::permissions::PermissionEngine::load(&trie);
    let elapsed = t.stop();
    println!(
        "  {:<45} {:>8.2}ms  {:>10} identities  {} groups",
        "permission engine load",
        elapsed.as_secs_f64() * 1000.0,
        engine.identity_count(),
        engine.group_count(),
    );

    let n = 100_000;

    let t = Timer::start("check allowed (developer read db)");
    for _ in 0..n {
        let _ = engine.check(50, pith::permissions::Verb::Read, &["databases", "colors", "blue"]);
    }
    t.stop_print(n);

    let t = Timer::start("check denied (guest write hard)");
    for _ in 0..n {
        let _ = engine.check(500, pith::permissions::Verb::Write, &["hard", "reserved"]);
    }
    t.stop_print(n);

    let t = Timer::start("check wildcard (system write deep)");
    for _ in 0..n {
        let _ = engine.check(1, pith::permissions::Verb::Write, &["jobs", "42", "-state", "running"]);
    }
    t.stop_print(n);
}

// ---------------------------------------------------------------------------
// Test 5: Parser throughput
// ---------------------------------------------------------------------------

fn test_parser_throughput(root: &std::path::Path) {
    println!("\n--- Parser Throughput ---");

    let reserved = root.join("hard/reserved");
    let alphabet = pith::alphabet::Alphabet::load(&reserved).unwrap();

    let n = 100_000;
    let segments = [
        "blue", "-expected", "€$price", "§read", "!completed",
        "#main", "~42", "(hello world)", "001", "∆psychology∆blue",
    ];

    let t = Timer::start("classify_segment (mixed types, 100k)");
    for i in 0..n {
        let seg = segments[i % segments.len()];
        let _ = pith::parser::classify_segment(seg, &alphabet);
    }
    t.stop_print(n);
}

// ---------------------------------------------------------------------------
// Test 6: Job lifecycle throughput
// ---------------------------------------------------------------------------

fn test_job_lifecycle(root: &std::path::Path, n_jobs: usize) {
    println!("\n--- Job Lifecycle Throughput ---");

    // Create N jobs with full structure
    let t = Timer::start("create jobs (type + state + owner)");
    for i in 1..=n_jobs {
        let job = root.join(format!("jobs/{}", i));
        let type_dir = job.join("-expected/type");
        std::fs::create_dir_all(&type_dir).unwrap();
        std::fs::File::create(type_dir.join("job")).unwrap();

        let state_dir = job.join("-state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::File::create(state_dir.join("pending")).unwrap();

        let owner_dir = job.join("-owner");
        std::fs::create_dir_all(&owner_dir).unwrap();
        std::fs::File::create(owner_dir.join("001")).unwrap();
    }
    t.stop_print(n_jobs);

    // Transition all jobs: pending → running
    let t = Timer::start("transition pending → running");
    for i in 1..=n_jobs {
        let pending = root.join(format!("jobs/{}/-state/pending", i));
        let running = root.join(format!("jobs/{}/-state/running", i));
        std::fs::remove_file(&pending).unwrap();
        std::fs::File::create(&running).unwrap();
    }
    t.stop_print(n_jobs);

    // Complete all jobs: running → completed + signal
    let t = Timer::start("complete jobs (state + signal)");
    for i in 1..=n_jobs {
        let running = root.join(format!("jobs/{}/-state/running", i));
        let completed = root.join(format!("jobs/{}/-state/completed", i));
        std::fs::remove_file(&running).unwrap();
        std::fs::File::create(&completed).unwrap();
        std::fs::File::create(root.join(format!("jobs/{}/!completed", i))).unwrap();
    }
    t.stop_print(n_jobs);

    // Cleanup
    for i in 1..=n_jobs {
        let _ = std::fs::remove_dir_all(root.join(format!("jobs/{}", i)));
    }
}

// ---------------------------------------------------------------------------
// Test 7: Database query throughput
// ---------------------------------------------------------------------------

fn test_database_query(root: &std::path::Path, n_entries: usize) {
    println!("\n--- Database Build & Query ---");

    // Create database entries
    let t = Timer::start("create database entries");
    for i in 0..n_entries {
        let category = format!("cat_{}", i / 10);
        let entry = format!("entry_{}", i);
        let path = root.join(format!("databases/bench/{}/{}", category, entry));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::File::create(&path).unwrap();
    }
    t.stop_print(n_entries);

    // Build trie with database
    let reserved = root.join("hard/reserved");
    let alphabet = pith::alphabet::Alphabet::load(&reserved).unwrap();
    let trie = pith::trie::Trie::build(root, &alphabet).unwrap();

    // Query throughput
    let n = 10_000;
    let t = Timer::start("database query_set (10k queries)");
    for i in 0..n {
        let cat = format!("cat_{}", i % (n_entries / 10));
        let _ = pith::subsystems::databases::query_set(&trie, &["bench", &cat]);
    }
    t.stop_print(n);

    // Cleanup
    let _ = std::fs::remove_dir_all(root.join("databases/bench"));
}

// ---------------------------------------------------------------------------
// Test 8: API throughput (Unix socket)
// ---------------------------------------------------------------------------

async fn api_client(
    socket_path: PathBuf,
    n_requests: usize,
    client_id: usize,
    total_ops: Arc<AtomicU64>,
    total_errors: Arc<AtomicU64>,
) -> Duration {
    let start = Instant::now();
    let stream = match UnixStream::connect(&socket_path).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("  Client {} cannot connect: {}", client_id, e);
            total_errors.fetch_add(n_requests as u64, Ordering::Relaxed);
            return start.elapsed();
        }
    };

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let ops = ["ping", "status", "ls"];
    let paths = ["", "", "hard/types"];

    for i in 0..n_requests {
        let op = ops[i % ops.len()];
        let path = paths[i % paths.len()];
        let req = format!("{{\"op\":\"{}\",\"path\":\"{}\"}}\n", op, path);

        if writer.write_all(req.as_bytes()).await.is_err() {
            total_errors.fetch_add(1, Ordering::Relaxed);
            continue;
        }

        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                total_errors.fetch_add(1, Ordering::Relaxed);
                break;
            }
            Ok(_) => {
                total_ops.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                total_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    start.elapsed()
}

async fn test_api_throughput(socket_path: PathBuf, n_clients: usize, n_requests: usize) {
    println!("\n--- API Throughput (Unix Socket) ---");
    println!(
        "  {} clients x {} requests = {} total",
        n_clients,
        n_requests,
        n_clients * n_requests
    );

    let total_ops = Arc::new(AtomicU64::new(0));
    let total_errors = Arc::new(AtomicU64::new(0));

    let start = Instant::now();

    let mut handles = Vec::new();
    for client_id in 0..n_clients {
        let sp = socket_path.clone();
        let ops = Arc::clone(&total_ops);
        let errs = Arc::clone(&total_errors);
        handles.push(tokio::spawn(async move {
            api_client(sp, n_requests, client_id, ops, errs).await
        }));
    }

    let mut client_durations = Vec::new();
    for handle in handles {
        if let Ok(d) = handle.await {
            client_durations.push(d);
        }
    }

    let total_elapsed = start.elapsed();
    let ops = total_ops.load(Ordering::Relaxed);
    let errs = total_errors.load(Ordering::Relaxed);
    let ops_per_sec = ops as f64 / total_elapsed.as_secs_f64();
    let avg_latency = if ops > 0 {
        total_elapsed.as_secs_f64() * 1_000_000.0 / ops as f64
    } else {
        0.0
    };

    println!(
        "  {:<45} {:>8.2}ms total",
        "concurrent API test", total_elapsed.as_secs_f64() * 1000.0,
    );
    println!(
        "  {:<45} {:>10} ops  {:>10.0} ops/s  {:>8.1}us/op avg",
        "results", ops, ops_per_sec, avg_latency,
    );
    if errs > 0 {
        println!("  {:<45} {:>10} errors", "WARNING", errs);
    }
}

// ---------------------------------------------------------------------------
// Test 9: Subsystem dispatch throughput
// ---------------------------------------------------------------------------

fn test_subsystem_dispatch_throughput() {
    println!("\n--- Subsystem Dispatch Throughput ---");

    use pith::dispatcher::Scope;
    use pith::subsystems::{FsEvent, FsEventKind, SubsystemRegistry};
    use pith::subsystems::events::EventsSubsystem;
    use pith::subsystems::jobs::JobsSubsystem;
    use pith::subsystems::channels::ChannelsSubsystem;
    use pith::subsystems::logs::LogsSubsystem;
    use pith::subsystems::states::StatesSubsystem;
    use pith::subsystems::workers::WorkersSubsystem;
    use pith::subsystems::scheduler::SchedulerSubsystem;
    use pith::subsystems::programs::ProgramsSubsystem;
    use pith::subsystems::databases::DatabasesSubsystem;
    use pith::subsystems::subscriptions::SubscriptionsSubsystem;

    let mut registry = SubsystemRegistry::new();
    registry.register(Box::new(EventsSubsystem::new()));
    registry.register(Box::new(ChannelsSubsystem::new()));
    registry.register(Box::new(LogsSubsystem::new()));
    registry.register(Box::new(StatesSubsystem::new()));
    registry.register(Box::new(JobsSubsystem::new()));
    registry.register(Box::new(WorkersSubsystem::new()));
    registry.register(Box::new(SchedulerSubsystem::new()));
    registry.register(Box::new(ProgramsSubsystem::new()));
    registry.register(Box::new(DatabasesSubsystem::new()));
    registry.register(Box::new(SubscriptionsSubsystem::new()));

    let events = vec![
        FsEvent { kind: FsEventKind::Assert, segments: vec!["events".into(), "!test".into()], scope: Scope::Events },
        FsEvent { kind: FsEventKind::Assert, segments: vec!["jobs".into(), "1".into(), "-state".into(), "pending".into()], scope: Scope::Jobs },
        FsEvent { kind: FsEventKind::Assert, segments: vec!["workers".into(), "1".into(), "-state".into(), "idle".into()], scope: Scope::Workers },
        FsEvent { kind: FsEventKind::Assert, segments: vec!["channels".into(), "#main".into(), "~0001".into()], scope: Scope::Channels },
        FsEvent { kind: FsEventKind::Assert, segments: vec!["states".into(), "1".into()], scope: Scope::States },
        FsEvent { kind: FsEventKind::Assert, segments: vec!["programs".into(), "app".into(), "!run".into()], scope: Scope::Programs },
    ];

    let n = 100_000;
    let t = Timer::start("dispatch 100k events (mixed scopes)");
    for i in 0..n {
        let event = &events[i % events.len()];
        let _ = registry.dispatch(event);
    }
    t.stop_print(n);
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let args = Args::parse();

    println!("==========================================================");
    println!("  0-Bytes OS Stress Test & Benchmark");
    println!("==========================================================");
    println!("  Jobs: {}  Workers: {}  Identities: {}  DB entries: {}",
        args.jobs, args.workers, args.identities, args.db_entries);
    println!("  API clients: {}  Requests/client: {}",
        args.api_clients, args.api_requests);

    // Create temporary filesystem
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    println!("  Temp FS: {}", root.display());
    println!("==========================================================");

    // Setup
    create_reserved(root);
    create_scopes(root);

    // Run tests
    test_fs_write_throughput(root, args.jobs);
    test_identity_creation(root, args.identities);
    test_trie_build(root);
    test_permission_resolution(root);
    test_parser_throughput(root);
    test_job_lifecycle(root, args.jobs);
    test_database_query(root, args.db_entries);
    test_subsystem_dispatch_throughput();

    // API test requires a running Pith instance
    let socket_path = root.join("pith.sock");
    let config = pith::config::PithConfig {
        fs_root: root.to_path_buf(),
        socket_path: socket_path.clone(),
        log_level: "warn".to_string(),
        enforce_permissions: false,
    };

    println!("\n--- Booting Pith for API test ---");
    let boot_start = Instant::now();
    let engine = pith::boot::boot(&config).await.unwrap();
    let boot_elapsed = boot_start.elapsed();
    println!(
        "  {:<45} {:>8.2}ms  {} nodes  {} identities",
        "full boot",
        boot_elapsed.as_secs_f64() * 1000.0,
        engine.trie.read().unwrap().total_nodes(),
        engine.permissions.identity_count(),
    );

    // Start API server
    let _api = pith::api::start_server(
        &socket_path,
        std::sync::Arc::clone(&engine.trie),
        engine.effector.clone(),
        std::sync::Arc::clone(&engine.session_manager),
        std::sync::Arc::clone(&engine.permissions),
        engine.config.enforce_permissions,
    )
    .await
    .unwrap();

    // Brief wait for server to be ready
    tokio::time::sleep(Duration::from_millis(100)).await;

    test_api_throughput(socket_path.clone(), args.api_clients, args.api_requests).await;

    // Shutdown
    pith::boot::shutdown(&engine).await.unwrap();
    pith::api::cleanup_socket(&socket_path);

    println!("\n==========================================================");
    println!("  Benchmark complete.");
    println!("==========================================================");
}
