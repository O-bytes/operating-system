//! Criterion micro-benchmarks for Pith core components.
//!
//! Run: cargo bench
//! Report: target/criterion/report/index.html

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use std::path::PathBuf;
use tempfile::TempDir;

use pith::alphabet::Alphabet;
use pith::parser::{classify_segment, parse_path};
use pith::permissions::{PermissionEngine, PermissionRule, Verb};
use pith::trie::Trie;

// ---------------------------------------------------------------------------
// Helpers: build a realistic test filesystem
// ---------------------------------------------------------------------------

fn create_reserved_dir(root: &std::path::Path) -> PathBuf {
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
    reserved
}

fn create_identity(root: &std::path::Path, id: &str, groups: &[&str], permissions: &[(&str, &str)]) {
    let id_dir = root.join(format!("hard/identities/{}", id));
    let type_dir = id_dir.join("-expected/type");
    std::fs::create_dir_all(&type_dir).unwrap();
    std::fs::File::create(type_dir.join("identity")).unwrap();

    for group in groups {
        let g = id_dir.join(format!("-group/{}", group));
        std::fs::create_dir_all(g.parent().unwrap()).unwrap();
        std::fs::File::create(&g).unwrap();
    }

    for (verb, target) in permissions {
        let p = id_dir.join(format!("§{}/{}", verb, target));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::File::create(&p).unwrap();
    }
}

fn create_group(root: &std::path::Path, name: &str, rules: &[(&str, &str)]) {
    for (verb, target) in rules {
        let p = root.join(format!("hard/groups/{}/§{}/{}", name, verb, target));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::File::create(&p).unwrap();
    }
}

fn create_jobs(root: &std::path::Path, count: usize) {
    for i in 1..=count {
        let job = root.join(format!("jobs/{}", i));
        let state = job.join("-state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::File::create(state.join("pending")).unwrap();
        let typ = job.join("-expected/type");
        std::fs::create_dir_all(&typ).unwrap();
        std::fs::File::create(typ.join("job")).unwrap();
    }
}

fn create_database(root: &std::path::Path) {
    let paths = [
        "databases/colors/blue/psychology/∆psychology∆blue",
        "databases/colors/red/warm",
        "databases/colors/green/nature",
        "databases/psychology/blue/effects/anxiety",
        "databases/psychology/blue/effects/bad_sleep",
        "databases/psychology/blue/effects/calm",
        "databases/psychology/red/effects/anger",
        "databases/translations/en/fr/colors/blue/bleu",
        "databases/translations/en/fr/colors/red/rouge",
        "databases/translations/en/es/colors/blue/azul",
    ];
    for p in paths {
        let full = root.join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::File::create(&full).unwrap();
    }
}

/// Build a realistic filesystem with N identities and M jobs.
fn build_test_fs(n_identities: usize, n_jobs: usize) -> (TempDir, Alphabet) {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    let reserved = create_reserved_dir(root);

    // Groups
    create_group(root, "system", &[("read", "_"), ("write", "_"), ("execute", "_")]);
    create_group(root, "developers", &[("read", "databases"), ("write", "jobs"), ("execute", "workers")]);
    create_group(root, "guests", &[("read", "databases"), ("deny", "hard")]);

    // Types
    let types_dir = root.join("hard/types");
    std::fs::create_dir_all(&types_dir).unwrap();
    for t in ["identity", "job", "worker", "program", "channel", "event", "database", "schema"] {
        std::fs::File::create(types_dir.join(t)).unwrap();
    }

    // Identities
    for i in 1..=n_identities {
        let id = format!("{:03}", i);
        let group = if i <= 10 { "system" } else if i <= 500 { "developers" } else { "guests" };
        create_identity(root, &id, &[group], &[]);
    }

    // Jobs
    create_jobs(root, n_jobs);

    // Scopes
    for scope in ["states", "workers", "channels", "events", "programs", "schedules", "sessions", "subscriptions", "logs", "tmp"] {
        std::fs::create_dir_all(root.join(scope)).unwrap();
    }
    std::fs::File::create(root.join("states/0")).unwrap();
    std::fs::File::create(root.join("workers/0")).unwrap();

    // Database
    create_database(root);

    let alphabet = Alphabet::load(&reserved).unwrap();
    (dir, alphabet)
}

// ---------------------------------------------------------------------------
// Benchmarks: Alphabet
// ---------------------------------------------------------------------------

fn bench_alphabet(c: &mut Criterion) {
    let dir = TempDir::new().unwrap();
    create_reserved_dir(dir.path());
    let reserved = dir.path().join("hard/reserved");

    c.bench_function("alphabet/load_38_doors", |b| {
        b.iter(|| {
            black_box(Alphabet::load(&reserved).unwrap());
        });
    });

    let alphabet = Alphabet::load(&reserved).unwrap();

    c.bench_function("alphabet/is_reserved_hit", |b| {
        b.iter(|| {
            black_box(alphabet.is_reserved('§'));
        });
    });

    c.bench_function("alphabet/is_reserved_miss", |b| {
        b.iter(|| {
            black_box(alphabet.is_reserved('z'));
        });
    });
}

// ---------------------------------------------------------------------------
// Benchmarks: Parser
// ---------------------------------------------------------------------------

fn bench_parser(c: &mut Criterion) {
    let (dir, alphabet) = build_test_fs(10, 0);

    let mut group = c.benchmark_group("parser/classify_segment");

    group.bench_function("data_node", |b| {
        b.iter(|| black_box(classify_segment("blue", &alphabet)));
    });

    group.bench_function("instruction_node", |b| {
        b.iter(|| black_box(classify_segment("-expected", &alphabet)));
    });

    group.bench_function("pointer_node", |b| {
        b.iter(|| black_box(classify_segment("€$price", &alphabet)));
    });

    group.bench_function("unicode_instruction", |b| {
        b.iter(|| black_box(classify_segment("§read", &alphabet)));
    });

    group.bench_function("long_data_name", |b| {
        b.iter(|| {
            black_box(classify_segment(
                "list_of_effects_on_humans_when_receiving_more_than_16_hours_per_day_at_13%",
                &alphabet,
            ))
        });
    });

    group.finish();

    let root = dir.path();
    c.bench_function("parser/parse_full_path_6_segments", |b| {
        let path = root.join("hard/identities/001/-expected/type/identity");
        b.iter(|| black_box(parse_path(&path, root, &alphabet, true)));
    });
}

// ---------------------------------------------------------------------------
// Benchmarks: Trie
// ---------------------------------------------------------------------------

fn bench_trie(c: &mut Criterion) {
    // Small trie (10 identities, 10 jobs)
    {
        let (dir, alphabet) = build_test_fs(10, 10);
        c.bench_function("trie/build_small_10id_10job", |b| {
            b.iter(|| black_box(Trie::build(dir.path(), &alphabet).unwrap()));
        });
    }

    // Medium trie (100 identities, 100 jobs)
    {
        let (dir, alphabet) = build_test_fs(100, 100);

        c.bench_function("trie/build_medium_100id_100job", |b| {
            b.iter(|| black_box(Trie::build(dir.path(), &alphabet).unwrap()));
        });

        let trie = Trie::build(dir.path(), &alphabet).unwrap();

        c.bench_function("trie/lookup_shallow_2_segments", |b| {
            b.iter(|| black_box(trie.get(&["hard", "types"])));
        });

        c.bench_function("trie/lookup_deep_6_segments", |b| {
            b.iter(|| {
                black_box(trie.get(&[
                    "hard", "identities", "042", "-expected", "type", "identity",
                ]))
            });
        });

        c.bench_function("trie/list_children_types", |b| {
            b.iter(|| black_box(trie.list(&["hard", "types"])));
        });

        c.bench_function("trie/list_children_100_identities", |b| {
            b.iter(|| black_box(trie.list(&["hard", "identities"])));
        });

        c.bench_function("trie/list_children_100_jobs", |b| {
            b.iter(|| black_box(trie.list(&["jobs"])));
        });

        c.bench_function("trie/total_nodes_count", |b| {
            b.iter(|| black_box(trie.total_nodes()));
        });
    }

    // Large trie (500 identities, 500 jobs)
    {
        let (dir, alphabet) = build_test_fs(500, 500);
        c.bench_function("trie/build_large_500id_500job", |b| {
            b.iter(|| black_box(Trie::build(dir.path(), &alphabet).unwrap()));
        });
    }

    // Insert/remove
    {
        let (dir, alphabet) = build_test_fs(10, 10);
        let trie = Trie::build(dir.path(), &alphabet).unwrap();

        c.bench_function("trie/insert_3_segments", |b| {
            b.iter_batched(
                || trie.clone(),
                |mut t| {
                    t.insert(
                        &["events".into(), "!bench".into(), "test".into()],
                        true,
                        &alphabet,
                    );
                    black_box(t);
                },
                BatchSize::SmallInput,
            );
        });

        c.bench_function("trie/remove_leaf", |b| {
            b.iter_batched(
                || {
                    let mut t = trie.clone();
                    t.insert(
                        &["events".into(), "!bench".into()],
                        true,
                        &alphabet,
                    );
                    t
                },
                |mut t| {
                    black_box(t.remove(&["events", "!bench"]));
                },
                BatchSize::SmallInput,
            );
        });
    }
}

// ---------------------------------------------------------------------------
// Benchmarks: Permissions
// ---------------------------------------------------------------------------

fn bench_permissions(c: &mut Criterion) {
    // Build FS with realistic permissions
    let (dir, alphabet) = build_test_fs(100, 50);
    let trie = Trie::build(dir.path(), &alphabet).unwrap();
    let engine = PermissionEngine::load(&trie);

    c.bench_function("permissions/load_100_identities", |b| {
        b.iter(|| black_box(PermissionEngine::load(&trie)));
    });

    c.bench_function("permissions/check_allowed_developer", |b| {
        b.iter(|| {
            black_box(engine.check(100, Verb::Read, &["databases", "colors", "blue"]));
        });
    });

    c.bench_function("permissions/check_denied_guest", |b| {
        b.iter(|| {
            black_box(engine.check(600, Verb::Write, &["hard", "reserved"]));
        });
    });

    c.bench_function("permissions/check_wildcard_system", |b| {
        b.iter(|| {
            black_box(engine.check(1, Verb::Write, &["jobs", "42", "-state", "running"]));
        });
    });

    c.bench_function("permissions/rule_matches_prefix", |b| {
        let rule = PermissionRule {
            verb: Verb::Read,
            target: vec!["databases".to_string()],
        };
        b.iter(|| {
            black_box(rule.matches(&["databases", "colors", "blue", "psychology"]));
        });
    });

    c.bench_function("permissions/rule_matches_wildcard", |b| {
        let rule = PermissionRule {
            verb: Verb::Read,
            target: vec!["_".to_string()],
        };
        b.iter(|| {
            black_box(rule.matches(&["any", "path", "at", "all"]));
        });
    });
}

// ---------------------------------------------------------------------------
// Benchmarks: Database query
// ---------------------------------------------------------------------------

fn bench_database(c: &mut Criterion) {
    let (dir, alphabet) = build_test_fs(10, 10);
    let trie = Trie::build(dir.path(), &alphabet).unwrap();

    c.bench_function("database/query_set_small", |b| {
        b.iter(|| {
            black_box(pith::subsystems::databases::query_set(
                &trie,
                &["psychology", "blue", "effects"],
            ));
        });
    });

    c.bench_function("database/query_translation", |b| {
        b.iter(|| {
            black_box(pith::subsystems::databases::query_set(
                &trie,
                &["translations", "en", "fr", "colors", "blue"],
            ));
        });
    });

    c.bench_function("database/query_nonexistent", |b| {
        b.iter(|| {
            black_box(pith::subsystems::databases::query_set(
                &trie,
                &["nonexistent", "path"],
            ));
        });
    });
}

// ---------------------------------------------------------------------------
// Benchmarks: Effector (filesystem write throughput)
// ---------------------------------------------------------------------------

fn bench_effector(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("effector/touch_single_file", |b| {
        b.iter_batched(
            || {
                let dir = TempDir::new().unwrap();
                let effector = pith::effector::Effector::new(dir.path().to_path_buf());
                (dir, effector)
            },
            |(dir, effector)| {
                rt.block_on(async {
                    effector
                        .execute(&pith::effector::Effect::Touch {
                            path: PathBuf::from("test/file"),
                        })
                        .await
                        .unwrap();
                });
                black_box(&dir);
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("effector/mkdir_nested", |b| {
        b.iter_batched(
            || {
                let dir = TempDir::new().unwrap();
                let effector = pith::effector::Effector::new(dir.path().to_path_buf());
                (dir, effector)
            },
            |(dir, effector)| {
                rt.block_on(async {
                    effector
                        .execute(&pith::effector::Effect::MakeDir {
                            path: PathBuf::from("a/b/c/d/e"),
                        })
                        .await
                        .unwrap();
                });
                black_box(&dir);
            },
            BatchSize::SmallInput,
        );
    });
}

// ---------------------------------------------------------------------------
// Benchmarks: Subsystem dispatch
// ---------------------------------------------------------------------------

fn bench_subsystem_dispatch(c: &mut Criterion) {
    use pith::dispatcher::Scope;
    use pith::subsystems::{FsEvent, FsEventKind, SubsystemRegistry};
    use pith::subsystems::events::EventsSubsystem;
    use pith::subsystems::jobs::JobsSubsystem;
    use pith::subsystems::channels::ChannelsSubsystem;
    use pith::subsystems::logs::LogsSubsystem;
    use pith::subsystems::states::StatesSubsystem;
    use pith::subsystems::workers::WorkersSubsystem;

    let mut registry = SubsystemRegistry::new();
    registry.register(Box::new(EventsSubsystem::new()));
    registry.register(Box::new(ChannelsSubsystem::new()));
    registry.register(Box::new(LogsSubsystem::new()));
    registry.register(Box::new(StatesSubsystem::new()));
    registry.register(Box::new(JobsSubsystem::new()));
    registry.register(Box::new(WorkersSubsystem::new()));

    c.bench_function("subsystem/dispatch_event_signal", |b| {
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["events".into(), "!test_signal".into()],
            scope: Scope::Events,
        };
        b.iter(|| black_box(registry.dispatch(&event)));
    });

    c.bench_function("subsystem/dispatch_job_state_change", |b| {
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["jobs".into(), "42".into(), "-state".into(), "running".into()],
            scope: Scope::Jobs,
        };
        b.iter(|| black_box(registry.dispatch(&event)));
    });

    c.bench_function("subsystem/dispatch_worker_assignment", |b| {
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec![
                "workers".into(), "1".into(), "-assigned".into(),
                "jobs".into(), "42".into(),
            ],
            scope: Scope::Workers,
        };
        b.iter(|| black_box(registry.dispatch(&event)));
    });

    c.bench_function("subsystem/dispatch_no_matching_scope", |b| {
        let event = FsEvent {
            kind: FsEventKind::Assert,
            segments: vec!["tmp".into(), "garbage".into()],
            scope: Scope::Tmp,
        };
        b.iter(|| black_box(registry.dispatch(&event)));
    });
}

// ---------------------------------------------------------------------------
// Register all benchmark groups
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_alphabet,
    bench_parser,
    bench_trie,
    bench_permissions,
    bench_database,
    bench_effector,
    bench_subsystem_dispatch,
);
criterion_main!(benches);
