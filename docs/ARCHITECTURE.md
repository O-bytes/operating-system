# 0-Bytes OS — Architecture

## Vision

An operating system where **no file ever contains data**. All information is encoded in:
- File and folder **names**
- Directory **hierarchy** (paths)
- File **existence or absence**
- File **metadata** (timestamps)

A Rust engine named **Pith** observes the filesystem, interprets it as a living system, and exposes it to any program in any language.

## The Four Primitives

Everything in 0-Bytes OS is built from four filesystem operations:

| Command | OS Semantic | Analogy |
|---------|------------|---------|
| `touch` | Assert / Signal / Allocate a bit | CPU write |
| `rm` | Retract / Deallocate / Negate | Memory free |
| `mv` | Transform / Rename / Reassign | Data transform |
| `mkdir` | Allocate scope / Open namespace | Memory alloc |

These are the instruction set. There is no other way to change state.

## Three Node Classes

Every filesystem entry is classified by how its name begins:

### Data Node
Name starts with a literal character (no reserved prefix). It IS the data.
```
blue, anxiety, rouge, 001
```

### Instruction Node
Name starts with a reserved logic door character. It encodes an operation.
```
-expected    → state pointer: "expected"
!completed   → signal: "completed"
§read        → permission: "read"
#main        → channel: "main"
```

### Pointer Node
Name starts with `€`. Escapes the next character from logic door interpretation.
```
€$price     → literal "$price", NOT a schema instruction
€€          → literal "€"
```

## The Path as Sentence

A complete path reads as a sentence, left to right:

```
src/hard/identities/001/-expected/type/identity
```
> "In the hard system, identities, slot 001, at state expected, of type identity."

```
src/databases/psychology/blue/.../anxiety
```
> "In databases, domain psychology, subject blue, ..., one result is anxiety."

- **Directories** are scopes — they qualify and group
- **Leaf files** are assertions — their existence states a truth

## Timestamps as Hidden State

Without violating the zero-byte rule, filesystem metadata carries numeric values:

| Metadata | Usage |
|----------|-------|
| **mtime** | Free numeric value per file. Set with `touch -t`. Used for scheduling, ordering, versioning. |
| **ctime** | Birth timestamp. Immutable. Establishes creation order. |
| **Comparison** | mtime ordering = event sequencing without a central clock. |
| **Future mtime** | A file with mtime in the future = scheduled event. The engine fires it when wall clock catches up. |

## System Components

### hard/ — The ROM
Immutable system definitions. The engine reads these at boot and protects them at runtime.
- `reserved/` — Logic door alphabet (one 0-byte file per character)
- `identities/` — Identity slots (001-777)
- `groups/` — Permission groups
- `types/` — Type system definitions

### states/ — Global State Machine
Current system state. File existence = state is active. Transitions are encoded as directory structures.

### jobs/ — Job Queue
Numbered job directories with lifecycle: pending → running → completed. Each job has state, owner, priority, input/output scopes.

### workers/ — Worker Pool
Execution units. The engine maps each worker to a thread/task. Workers pull from jobs, communicate via channels.

### channels/ — IPC
Message queues (`#channel_name/`) for ordered communication. Messages are sequenced files with raw value content.

### events/ — Event Log
Signals (`!event_name`) for fire-and-forget notifications. The engine watches for signal file creation and notifies subscribers.

### programs/ — Installed Programs
A "program" is a directory tree encoding a state machine. Install by copying. Run by touching `!run`.

### databases/ — Data Storage
Semantic data encoded purely in path hierarchies. Set membership, key-value pairs, cross-references, translations.

### pointers/ — Reference Tables
Lookup tables. The `unicodes/` subtree maps every Unicode codepoint to a nameable pointer.

### schedules/ — Timed Tasks
Files whose mtime encodes the next firing time. The engine's scheduler thread watches and fires on schedule.

### sessions/ — Active Sessions
One directory per active session, binding an identity to an API connection.

### subscriptions/ — Event Subscriptions
Each identity can subscribe to events. Paths mirror the events they watch.

### tmp/ — Temporary Space
Auto-cleaned by the engine on boot. Used for intermediate computations.

### logs/ — System Logs
Filesystem-based logging. Events are recorded as timestamped file creations.

## Pith — The Rust Engine

Pith is the reactor core. It does not "run" programs — it **observes the filesystem and reacts**.

### Event Loop

```
Filesystem (the hardware)
        │
        │ kqueue / inotify
        ▼
   ┌─────────┐
   │ Watcher  │  dedicated OS thread
   └────┬────┘
        │ channel (crossbeam/tokio mpsc)
        ▼
   ┌─────────┐
   │ Parser   │  path → instruction
   └────┬────┘
        ▼
   ┌──────────┐
   │Dispatcher│  routes by top-level scope
   └──┬──┬──┬┘
      │  │  │
      ▼  ▼  ▼
   [Subsystems]  jobs, states, workers, channels,
                 events, scheduler, permissions...
      │  │  │
      ▼  ▼  ▼
   ┌──────────┐
   │ Effector  │  touch / rm / mv / mkdir responses
   └──────────┘
```

### Boot Sequence
1. Load reserved alphabet from `hard/reserved/`
2. Build in-memory trie index by walking `src/`
3. Load identities and groups
4. Initialize all subsystems
5. Start filesystem watcher
6. `touch events/!boot`
7. Open API listener (Unix socket)
8. Enter event loop

### Shutdown Sequence
1. `touch events/!shutdown`
2. Stop watcher, drain event queue
3. Persist scheduler state (mtimes)
4. Clean `tmp/`
5. `rm events/!boot`
6. Exit

### Self-Describing Design
The engine reads `hard/reserved/` at boot — it does NOT hardcode the logic door alphabet. The reserved alphabet, types, permissions, and programs are all defined in the filesystem itself. The engine interprets them; it does not define them.

## Developer Access

### Level 1 — Raw Filesystem
Any shell, any language. `touch`, `rm`, `mv`, `mkdir`. Works immediately.

### Level 2 — CLI (`0b`)
Ergonomic commands that translate to filesystem operations.

### Level 3 — Rust SDK
Typed Rust crate with watchers, builders, and async event streams.

### Level 4 — Any Language SDK
Since the protocol IS the filesystem, any language with file I/O can interact with 0-Bytes OS.
