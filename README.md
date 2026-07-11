# 0-Bytes OS

An operating system in which **every file is empty**.

All information lives in file **names**, directory **hierarchy**, file **existence**, and **modification times**. The filesystem *is* the computer. Every `touch` is a CPU instruction, every `rm` a deallocation, every `mv` a data transform, every `mkdir` a memory allocation.

A small Rust engine named **Pith** watches the filesystem, interprets it as a living system, and exposes it to any program in any language.

```bash
find src -type f -size +0c | wc -l
# 0
```

This is not infrastructure. It is a conceptual piece: an inquiry into how small a computational substrate can be, built seriously enough to actually run.

## The idea

Four primitives are the entire instruction set. There is no other way to change state.

| Command | OS semantic |
|---------|-------------|
| `touch` | Assert / Signal / Allocate a bit |
| `rm`    | Retract / Deallocate / Negate |
| `mv`    | Transform / Rename / Reassign |
| `mkdir` | Allocate a scope / Open a namespace |

Four carriers hold all information:

- **Existence** — a file being there states a truth; its absence denies it
- **Name** — the value itself (`blue`, `anxiety`, `001`)
- **Position** — the path qualifies the value, like grammar qualifies a word
- **mtime** — a free numeric register per file; a future mtime is a scheduled event

## Node classes

Every filesystem entry is classified by how its name begins:

```
blue          → data node        (the name IS the value)
-expected     → instruction node (- is a logic door: "state pointer")
€$price       → pointer node     (€ escapes: literal "$price", not a schema)
```

Logic doors are reserved characters that act as transformation functions. The alphabet is **self-describing**: the engine reads `src/hard/reserved/` at boot. Add a file, extend the language. Only `€` (the escape) is hardcoded — the axiom.

| Char | Name | Char | Name | Char | Name |
|------|------|------|------|------|------|
| `€` | Escape | `$` | Schema | `-` | State |
| `!` | Signal | `#` | Channel | `§` | Permission |
| `~` | Number | `@` | Dict key | `:` | Binding |
| `[` `]` | Array | `{` `}` | Object | `(` `)` | Raw value |
| `∞` | Loop | `λ` | Lambda | `∴` | Therefore |

*(38 doors in total — see [docs/RESERVED_VALUES.md](docs/RESERVED_VALUES.md).)*

## The path as a sentence

A path reads left to right, like a sentence:

```
src/hard/identities/001/-expected/type/identity
     │         │     │      │       │      │
   scope     scope  slot  state:  scope  leaf
                          expected
```

> "In the hard system, identities, slot 001, at the expected state, of type identity."

Directories are scopes — they qualify and group. Leaf files are assertions — their existence states a truth.

## Filesystem layout

```
src/
├── hard/            # ROM — immutable system definitions
│   ├── reserved/    # 38 logic-door files (the alphabet)
│   ├── identities/  # identity slots
│   ├── groups/      # permission groups
│   └── types/       # type definitions
├── states/          # global state machine
├── jobs/            # task queue (pending → running → completed)
├── workers/         # worker pool
├── channels/        # IPC message queues (#system, #errors)
├── events/          # fire-and-forget signals (!boot, !shutdown, ...)
├── programs/        # installed programs — state machines as directory trees
├── databases/       # semantic data in path hierarchies
├── pointers/        # reference tables (65,536 Unicode code points)
├── schedules/       # scheduled tasks (mtime = next firing)
├── sessions/        # active API sessions
├── subscriptions/   # per-identity event subscriptions
├── logs/            # timestamped log entries
└── tmp/             # scratch space (cleared at boot)
```

## Pith — the engine

```
Filesystem (the hardware)
      │  kqueue / inotify
      ▼
  Watcher ──► Parser ──► Dispatcher ──► 10 subsystems ──► Effector
              classify    update the     react to          touch / rm / mv
              segments    in-memory      events, emit      (with loop
                          trie           effects           avoidance)
```

Pith never executes programs — it interprets filesystem changes as instructions. Subsystems are pure functions from event to effects; the Effector is the only module that writes. The engine itself never writes a byte of content: its whole effect vocabulary is `touch`, `touch -t`, `rm`, `mv`.

## Quick start

```bash
0b install     # symlink bin/0b into ~/bin (auto-detects the checkout)
0b init        # provision the admin identity
0b up          # start the engine

# in another terminal:
0b demo        # start the metronome
```

The demo is a **metronome**: a two-state machine (`tick ⇄ tock`) encoded entirely as empty files, advanced forever by the scheduler. Watch it beat:

```bash
watch -n 1 ls src/programs/metronome/-state src/tmp
```

Every beat is a zero-byte file appearing, moving, disappearing. The machine runs; nothing is ever written.

### A program is a directory

The metronome's full source code:

```
programs/metronome/
├── -expected/type/program
├── -entry/-state/tick
└── -states/
    ├── tick/
    │   ├── -action/touch/tmp/tick
    │   └── -transitions/tock/-condition/events/!schedule_metronome
    └── tock/
        ├── -action/touch/tmp/tock
        └── -transitions/tick/-condition/events/!schedule_metronome
```

States, transitions, conditions, actions — all topology. `touch programs/metronome/!run` starts it.

## Permissions & identity

Permissions are encoded with the `§` door and enforced by Pith (resolution: deny > own > grant). Identities carry privilege tiers in their first digit (0xx = omni … 8xx = guest). Passwords are argon2id hashes stored — faithfully — in the **filename**:

```
hard/identities/001/-secret/.argon2id.v=19.m=19456,t=2,p=1.SALT.DIGEST
```

See [docs/PERMISSIONS.md](docs/PERMISSIONS.md).

## API

Pith exposes newline-delimited JSON over a Unix socket. Any language with file I/O can also just `touch` and `rm` directly.

```python
pith("ping")                      # → {"ok": true, "data": "pong"}
pith("ls", "hard/types")          # → ["channel", "database", "event", ...]
pith("touch", "events/!hello")    # create a signal
pith("db_query", "colors")        # set query over path hierarchies
```

Operations: `ping`, `status`, `ls`, `query`, `touch`, `mkdir`, `rm`, `mv`, `db_query`, `authenticate`, `create_identity`.

## How fast is nothing?

Benchmarks are beside the point of a conceptual piece, but the question "what does computing with empty files cost?" turns out to have interesting answers (Apple Silicon, `cargo bench` / `cargo run --release --bin stress`):

| Operation | Rate |
|---|---|
| Read a state (in-memory trie lookup) | 8.5M/s (0.1 µs) |
| Check a permission | 6.9–20M/s |
| Create a signal (`touch`) | 14k/s (70 µs) |
| Full job lifecycle (create → run → complete) | ~1,500/s |

Reading state is essentially free; writing state costs what the filesystem charges for an inode. The engine adds less than a microsecond on top. The bottleneck of a zero-byte computer is the filesystem itself — which is the point.

## Documentation

| Document | Content |
|----------|---------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Vision, primitives, event loop |
| [docs/RESERVED_VALUES.md](docs/RESERVED_VALUES.md) | The full logic-door alphabet, `€` escaping |
| [docs/NAMING.md](docs/NAMING.md) | Naming grammar, path-as-sentence |
| [docs/PERMISSIONS.md](docs/PERMISSIONS.md) | Identity model, `§` verbs, resolution |
| [docs/FILESYSTEM.md](docs/FILESYSTEM.md) | Complete filesystem organization |
| [docs/IMPLEMENTATION_STATUS.md](docs/IMPLEMENTATION_STATUS.md) | What actually works today |

## License

To be determined.
