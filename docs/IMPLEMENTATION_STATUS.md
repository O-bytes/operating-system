# 0-Bytes OS — Implementation Status

What actually works today, versus what remains aspirational. Kept deliberately short and factual.

## What works

**Core engine** (`pith/`, ~8k lines of Rust):

| Component | Status | Notes |
|-----------|--------|-------|
| Alphabet | Full | Self-describing loader from `hard/reserved/`; only `€` is hardcoded. NFC normalization for APFS. |
| Parser | Full | Segment classification: Data / Instruction / Pointer. |
| Trie | Full | In-memory index built via WalkDir (skips `pointers/unicodes`), incremental insert/remove, `leaf_paths()` helper. |
| Watcher | Functional | notify (kqueue/inotify) over 11 scopes; effector writes are filtered out via a pending-op set. |
| Dispatcher | Full | Scope routing + trie updates. |
| Effector | Full | The **only** module that writes. Effect vocabulary: `Touch`, `TouchWithMtime`, `Remove`, `Move` — the engine never writes a byte of file content. |
| Permissions | Full | deny > own > grant, wildcards, group inheritance, privilege tiers. Loaded from the trie at boot. |
| Auth | Full | argon2id; PHC hash stored in the filename under `-secret/`. |
| Sessions | Full | Unix UID → identity via UCred; upgrade via `authenticate`. |
| API | Full | Newline-JSON over Unix socket; 11 operations; per-session permission checks. |
| Boot loop | Full | Boot → watch → dispatch → react → write; scheduler tick (~900 ms); program re-evaluation (~1.2 s); graceful shutdown. |

**Subsystems** (all pure `event → Vec<Effect>`): events, channels, jobs, workers, scheduler, programs, databases, subscriptions, logs, states.

- **programs** — directory-encoded state machines: `!run` entry, `-states/{s}/-action/{touch,remove,schedule}/`, transitions gated on `-condition/` leaf paths. Deep action paths and leaf-only condition matching are supported. The `metronome` demo exercises the full loop.
- **scheduler** — a schedule fires when its mtime is in the past; it emits `events/!schedule_{name}` on every tick until re-armed.
- **jobs + workers** — assignment and execution of declared actions (touch / emit / write-as-filename / remove), auto-completion.
- **channels / subscriptions / events** — message sequencing, delivery events, per-identity mirrors.

## Known limitations

- Raw-filesystem permission enforcement is off by default (`enforce=false`); permissions are enforced on the API path.
- Schedules do not re-arm themselves; a due schedule fires on every tick until moved into the future.
- Advanced job semantics (priority, IO streams, retry policies) do not exist.
- No end-to-end integration test drives the real watcher; the two watcher tests that do are `#[ignore]`d (platform-dependent FSEvents latency).

## Tests & benchmarks

- `cargo test`: 108 unit tests, all green (2 ignored, see above). Tests build their own temp filesystems; none depend on the repo's `src/`.
- `cargo bench`: Criterion micro-benchmarks over alphabet, parser, trie, permissions, databases, effector, dispatch.
- `cargo run --release --bin stress`: boots a full engine against a temp filesystem and drives concurrent jobs + API load.

## Design invariants

1. Every file in `src/` is zero bytes: `find src -type f -size +0c | wc -l` → `0`.
2. The engine's only write vocabulary is `touch` / `touch -t` / `rm` / `mv` — content cannot be written, by construction.
3. Subsystems are pure; the Effector is the single writer; watcher loops are prevented by registering pending ops before writing.
4. The logic-door alphabet is data, not code: `hard/reserved/` defines the language.
