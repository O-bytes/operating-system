# Filesystem Layout

## Overview

```
src/
├── hard/                    # ROM — immutable system definitions
│   ├── reserved/            # Logic door alphabet
│   ├── identities/          # Identity slots (001-777)
│   ├── groups/              # Permission groups
│   └── types/               # Type system definitions
│
├── states/                  # Global state machine
├── jobs/                    # Job queue
├── workers/                 # Worker pool
├── channels/                # IPC communication channels
├── events/                  # Event signals
├── programs/                # Installed programs
├── databases/               # Data storage (submodule)
├── pointers/                # Reference tables (submodule)
│   └── unicodes/            # Unicode codepoint table
├── schedules/               # Timed/recurring tasks
├── sessions/                # Active sessions
├── subscriptions/           # Event subscriptions
├── logs/                    # System logs
└── tmp/                     # Temporary space (cleaned on boot)
```

## hard/ — The ROM

System definitions that are loaded at boot and protected at runtime. Only `superroot` (2xx) identities can modify these.

### hard/reserved/

One zero-byte file per logic door character. The engine reads this directory to discover the alphabet — it does NOT hardcode reserved characters.

```
hard/reserved/
├── €    # escape
├── $    # schema
├── [    # array open
├── ]    # array close
├── |    # value separator
├── ,    # object separator
├── -    # state pointer
├── *    # compiled
├── +    # constant
├── {    # object open
├── }    # object close
├── (    # raw open
├── )    # raw close
├── @    # dict key
├── ~    # number
├── :    # binding
├── #    # channel
├── !    # signal
├── ?    # query
├── ^    # priority
├── &    # async
├── %    # modulo
├── <    # input
├── >    # output
├── =    # assert
├── ;    # sequence
├── _    # void/wildcard
├── §    # permission
├── ¶    # process
├── ∂    # delta
├── λ    # lambda
├── ∴    # then
├── ∵    # because
├── ∞    # loop
├── ▶    # start
├── ⏸    # pause
├── ⏹    # stop
└── ⌚    # timer
```

### hard/identities/

777 identity slots. Each is a numbered directory containing its properties.

```
hard/identities/
├── 001/
│   ├── -expected/type/identity
│   ├── -name/{name}
│   ├── -group/{group_name}
│   ├── §read/{scope}
│   └── §write/{scope}
├── 002/
│   └── ...
└── 777/
    └── ...
```

### hard/groups/

Permission groups. Identities reference groups via `-group/{name}` files.

```
hard/groups/
├── system/
│   ├── §read/_
│   ├── §write/_
│   └── §execute/_
├── admin/
│   └── ...
└── guests/
    └── ...
```

### hard/types/

Type definitions used by `-expected/type/{type_name}` patterns.

```
hard/types/
├── identity
├── job
├── worker
├── program
├── channel
├── event
├── database
└── schema
```

## states/ — Global State Machine

The system's global state. File existence = state is active.

```
states/
├── 0                        # State 0 (idle/boot) — active if file exists
└── -transitions/
    └── 0/
        └── 1/               # Transition from state 0 to state 1
            ├── -condition/   # What triggers this transition
            └── -action/      # What to do when transitioning
```

## jobs/ — Job Queue

Numbered job directories with full lifecycle management.

```
jobs/
├── 0                        # Null job (system anchor)
└── {id}/
    ├── -expected/type/job
    ├── -state/{state}       # pending, running, completed, failed
    ├── -owner/{identity_id}
    ├── ^~{n}                # Priority level
    ├── ¶input/              # Process input scope
    │   └── (raw value)
    ├── ¶output/             # Process output scope
    │   └── (raw value)
    ├── !started             # Signal: job started (mtime = when)
    └── !completed           # Signal: job completed (existence = done)
```

## workers/ — Worker Pool

Execution units mapped to engine threads/tasks.

```
workers/
├── 0                        # Null worker (system anchor)
└── {id}/
    ├── -expected/type/worker
    ├── -state/{state}       # idle, busy, stopped
    ├── -identity/{id}       # Runs as this identity
    ├── -capacity/~{n}       # Max concurrent jobs
    ├── -assigned/
    │   └── jobs/{job_id}    # Currently assigned jobs
    ├── #inbox/              # IPC: incoming messages
    └── #outbox/             # IPC: outgoing messages
```

## channels/ — IPC

Ordered message queues for inter-process communication.

```
channels/
├── #system/                 # System broadcast channel
│   ├── ~0001/(message)      # Message 1 (sequenced)
│   └── ~0002/(message)      # Message 2
└── #errors/                 # Error reporting channel
```

## events/ — Event Signals

Fire-and-forget signals. The engine watches for `!`-prefixed file creation.

```
events/
├── !boot                    # System booted (exists while running)
├── !shutdown                # System shutting down
└── -history/                # Historical events
```

## programs/ — Installed Programs

A program is a directory tree encoding a state machine.

```
programs/
└── {name}/
    ├── -expected/type/program
    ├── -entry/
    │   └── -state/init      # Starting state
    ├── -states/
    │   ├── init/
    │   │   ├── -action/     # What to do in this state
    │   │   └── -transitions/
    │   │       └── {next}/
    │   │           └── -condition/
    │   └── {other_states}/
    ├── ¶input/              # Program input schema
    └── ¶output/             # Program output schema
```

Install: copy the directory tree into `src/programs/`.
Run: `touch src/programs/{name}/!run`.

## databases/ — Data Storage

Semantic data encoded in path hierarchies. (Git submodule)

```
databases/
├── colors/
│   └── blue/
│       └── psychology/
│           └── ∆psychology∆blue     # Cross-reference
├── psychology/
│   └── blue/
│       └── .../
│           ├── anxiety              # Set member
│           └── bad_sleep            # Set member
└── translations/
    └── english/
        └── french/
            └── databases/colors/
                ├── blue/bleu        # Key → value
                └── red/rouge        # Key → value
```

## pointers/ — Reference Tables

Lookup tables and symbol registries. (Git submodule)

```
pointers/
└── unicodes/                        # One dir per Unicode codepoint
    ├── 0000/ㅤ                      # U+0000 → blank pointer file
    ├── 0001/ㅤ
    └── ... (65535 entries)
```

The Hangul Filler `ㅤ` (U+3164) is an existence marker — visually blank, semantically present.

## schedules/ — Timed Tasks

Files whose **mtime** encodes the next firing time. The engine scans and fires when wall clock reaches the mtime.

```
schedules/
└── {task_name}              # mtime = next firing time (zero-byte file)
```

## sessions/ — Active Sessions

One directory per active API/CLI connection.

```
sessions/
└── ~{id}/
    ├── -identity/{id}       # Bound identity
    └── -state/active        # Session state
```

## subscriptions/ — Event Subscriptions

Each identity's event subscriptions. Paths mirror the events they watch.

```
subscriptions/
└── {identity_id}/
    ├── events/!boot         # Subscribed to boot event
    └── jobs/{id}/!completed # Subscribed to specific job completion
```

## tmp/ — Temporary Space

Auto-cleaned by the engine on boot. Used for intermediate computations and atomic operations.

## logs/ — System Logs

Filesystem-based log entries. Each log is a timestamped zero-byte file.

```
logs/
└── {timestamp}/(log message)
```

## Scaling Strategy

### Numeric Sharding
When directories grow large, introduce sharding by prefix:
```
jobs/00/0001/    # Jobs 0000-0099
jobs/01/0100/    # Jobs 0100-0199
```

### Archival
Completed entities move to `-archive/` subdirectories:
```
jobs/-archive/0001/
```

### Git Submodules
Independent subtrees (databases, pointers) are separate git repos linked as submodules. This allows independent versioning and distribution.
