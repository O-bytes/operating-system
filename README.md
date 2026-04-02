# 0-Bytes OS

An operating system where **no file ever contains data**.

All information is encoded in file and folder **names**, directory **hierarchy**, file **existence**, and **metadata** (timestamps). The filesystem IS the computer. Every `touch` is a CPU instruction, every `rm` is memory deallocation, every `mv` is a data transformation, every `mkdir` is memory allocation.

A Rust engine called **Pith** observes the filesystem, interprets it as a living system, and exposes it to any program in any language.

## The Four Primitives

| Command | OS Semantic |
|---------|------------|
| `touch` | Assert / Signal / Allocate a bit |
| `rm`    | Retract / Deallocate / Negate |
| `mv`    | Transform / Rename / Reassign |
| `mkdir` | Allocate scope / Open namespace |

There is no other way to change state.

## Three Node Classes

Every filesystem entry is classified by how its name begins:

```
blue          → Data node       (the name IS the value)
-expected     → Instruction node (- is a logic door: "state pointer")
€$price       → Pointer node    (€ escapes: literal "$price", not schema)
```

## Logic Doors

Logic doors are reserved characters that act as transformer functions. The alphabet is **self-describing**: the engine reads `src/hard/reserved/` at boot. Add a file, extend the language.

`€` (U+20AC) is the only hardcoded value — the **axiom**. It escapes the next character from logic door interpretation.

| Char | Name | Char | Name | Char | Name |
|------|------|------|------|------|------|
| `€` | Escape | `$` | Schema | `-` | State |
| `!` | Signal | `#` | Channel | `§` | Permission |
| `~` | Number | `@` | Dict Key | `:` | Binding |
| `[` `]` | Array | `{` `}` | Object | `(` `)` | Raw Value |
| `*` | Compiled | `+` | Constant | `\|` | Value Sep |
| `,` | Object Sep | `_` | Wildcard | `^` | Priority |
| `&` | Async | `?` | Query | `%` | Modulo |
| `<` | Input | `>` | Output | `=` | Assert |
| `;` | Sequence | `¶` | Process | `∂` | Delta |
| `λ` | Lambda | `∴` | Then | `∵` | Because |
| `∞` | Loop | `▶` | Start | `⏸` | Pause |
| `⏹` | Stop | `⌚` | Timer | | |

## The Path as Sentence

A path reads left-to-right as a sentence:

```
src/hard/identities/001/-expected/type/identity
     │         │     │      │       │      │
   scope     scope  slot  state:  scope   leaf
                          expected
```

> "In the hard system, identities, slot 001, at state expected, of type identity."

## Filesystem Layout

```
src/
├── hard/                    # ROM — immutable system definitions
│   ├── reserved/            # 38 logic door files (the alphabet)
│   ├── identities/          # Identity slots (unbounded)
│   ├── groups/              # Permission groups (system, admin, developers, guests)
│   └── types/               # Type definitions (identity, job, worker, program, ...)
├── states/                  # Global state machine
├── jobs/                    # Job queue (lifecycle: pending → running → completed)
├── workers/                 # Worker pool
├── channels/                # IPC message queues (#system, #errors)
├── events/                  # Fire-and-forget signals (!boot, !shutdown, ...)
├── programs/                # Installed programs (state machines as directory trees)
├── databases/               # Semantic data in path hierarchies
├── pointers/                # Reference tables (65,536 Unicode codepoints)
├── schedules/               # Timed tasks (mtime = next fire time)
├── sessions/                # Active API sessions
├── subscriptions/           # Event subscriptions per identity
├── logs/                    # Timestamped log entries
└── tmp/                     # Temporary space (cleaned on boot)
```

## Pith — The Rust Engine

Pith observes the filesystem and reacts. It does not run programs — it interprets filesystem changes as instructions.

### Architecture

```
Filesystem (the hardware)
        │
        │ kqueue / inotify
        ▼
   ┌─────────┐
   │ Watcher  │  watches 11 scopes recursively
   └────┬────┘
        │
   ┌────▼────┐
   │ Parser   │  classifies segments (Data/Instruction/Pointer)
   └────┬────┘
        │
   ┌────▼──────┐
   │ Dispatcher │  routes by scope, updates in-memory trie
   └──┬──┬──┬──┘
      │  │  │
      ▼  ▼  ▼
   10 Subsystems   events, channels, logs, states, jobs,
                   workers, scheduler, programs, databases,
                   subscriptions
      │  │  │
      ▼  ▼  ▼
   ┌──────────┐
   │ Effector  │  touch / rm / mv / mkdir (with loop avoidance)
   └──────────┘
```

### Quick Start

```bash
cd pith
cargo build
cargo run -- start --root ../src
```

Pith boots, loads the 38 logic doors, builds an in-memory trie of ~3200 nodes, loads 777 identities and 4 permission groups, starts watching the filesystem, opens a Unix socket API on `/tmp/pith.sock`, and enters its event loop.

### API

Pith exposes a newline-delimited JSON API over a Unix domain socket.

```python
import socket, json

def pith(op, path="", args=None):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect("/tmp/pith.sock")
    req = {"op": op, "path": path}
    if args: req["args"] = args
    s.sendall((json.dumps(req) + "\n").encode())
    data = b""
    while b"\n" not in data:
        data += s.recv(4096)
    s.close()
    return json.loads(data)

pith("ping")                            # → {"ok": true, "data": "pong"}
pith("status")                          # → {"ok": true, "data": {"status": "running", "nodes": 3228}}
pith("ls", "hard/types")                # → ["channel","database","event","identity","job","program","schema","worker"]
pith("touch", "events/!hello")          # creates the signal file
pith("rm", "events/!hello")             # removes it
pith("db_query", "colors")              # → ["∆psychology∆blue"]
pith("mv", "tmp/a", {"to": "tmp/b"})   # renames a to b
```

**Operations:** `ping`, `status`, `ls`, `query`, `touch`, `mkdir`, `rm`, `mv`, `db_query`

Since the protocol is the filesystem, any language with file I/O can also interact directly:

```bash
touch src/events/'!my_signal'     # emit a signal
rm src/events/'!my_signal'        # retract it
mkdir -p src/jobs/1/-state        # create a job
touch src/jobs/1/-state/pending   # set its state
```

## Permission System

Permissions are encoded in the filesystem using the `§` logic door. No Unix chmod/chown — a custom overlay enforced by Pith.

```
src/hard/identities/001/
    -group/system              # group membership
    §read/_                    # can read everything (wildcard)

src/hard/groups/developers/
    §read/databases            # can read databases/
    §write/jobs                # can write to jobs/
    §execute/workers           # can execute workers

src/hard/groups/guests/
    §read/databases            # can read databases/
    §deny/hard                 # explicitly denied access to hard/
```

Resolution: **deny > own > grant > default deny**.

Identity tiers derived from the first digit: 0xx=omni, 1xx=shadow, 2xx=superroot, 3xx=root, 4xx=admin, 5xx=permissioned, 6xx=user, 7xx=shared, 8xx=guest, 9xx=digitalconsciousness.

## Documentation

| Document | Content |
|----------|---------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Vision, primitives, event loop, boot/shutdown |
| [docs/RESERVED_VALUES.md](docs/RESERVED_VALUES.md) | Complete logic door alphabet, the `€` escape mechanism |
| [docs/NAMING.md](docs/NAMING.md) | Naming grammar, segment classification, path-as-sentence |
| [docs/PERMISSIONS.md](docs/PERMISSIONS.md) | Identity model, `§` verbs, resolution algorithm |
| [docs/FILESYSTEM.md](docs/FILESYSTEM.md) | Full filesystem layout, scaling strategy |

## Project Structure

```
0-bytes/
├── src/              # The 0-bytes OS filesystem (zero-byte files only)
├── docs/             # Architecture documentation
├── pith/             # The Rust engine
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs           # CLI entry point
│       ├── alphabet.rs       # Self-describing logic door loader
│       ├── parser.rs         # Segment classifier
│       ├── trie.rs           # In-memory filesystem index
│       ├── identity.rs       # Identity + privilege tiers
│       ├── permissions.rs    # Permission engine
│       ├── watcher.rs        # Filesystem watcher
│       ├── dispatcher.rs     # Event routing + trie updates
│       ├── effector.rs       # Filesystem writer
│       ├── api/              # Unix socket server
│       └── subsystems/       # 10 reactive subsystems
└── .gitmodules       # pointers + databases submodules
```

**28 Rust source files. 86 tests. ~1600 lines of implementation.**

## License

TBD
