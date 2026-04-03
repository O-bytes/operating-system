# Permission System

## Philosophy

0-Bytes OS does NOT use native Unix file permissions (chmod, chown). Instead, permissions are encoded entirely in the filesystem structure using the `§` (U+00A7) logic door.

The Rust engine (Pith) reads these permission trees at boot and enforces them as a custom overlay on every operation.

## Identity Model

### Identities

Each identity is a numbered slot under `src/hard/identities/`:

```
src/hard/identities/
    001/
        -expected/type/identity     # Type declaration
        -name/admin                 # Human-readable name
        -secret/.argon2id.v=19...   # Password hash (in the FILENAME, zero bytes)
        -uid/501                    # Unix UID mapping (for session auto-binding)
        -group/system               # Group membership (one file per group)
        -group/developers           # Can belong to multiple groups
        §read/_                     # Direct permission grant (wildcard)
        §write/jobs/
        §own/databases/translations/
    601/
        -expected/type/identity
        -name/alice
        -secret/.argon2id.v=19...   # Each identity can have its own password
        -uid/502                    # Maps Unix UID 502 to this identity
        -group/developers
        §read/databases/
        §write/jobs/
```

Identity numbers are unbounded. The number IS the identity — no content needed. The privilege tier is derived from the **first digit** of the identity number.

### Privilege Hierarchy

The first 10 identity prefixes carry special meaning:

| Prefix | Role | Description |
|--------|------|-------------|
| 0xx | omni | Omniscient — above the system, can observe everything |
| 1xx | shadow | Shadow — invisible system processes |
| 2xx | superroot | Superroot — can modify hard/ (the ROM) |
| 3xx | root | Root — full system administration |
| 4xx | admin | Admin — can manage identities and groups |
| 5xx | permissioned | Permissioned — elevated custom permissions |
| 6xx | user | User — standard access |
| 7xx | shared | Shared — multi-tenant shared access |
| 8xx | guest | Guest — minimal read access |
| 9xx | digitalconsciousness | AI/autonomous agent identity |

## Permission Encoding

### The § Logic Door

A permission rule is a path starting with `§` followed by a verb, then the target scope:

```
§{verb}/{target_path...}
```

### Verbs

| Verb | Meaning |
|------|---------|
| `read` | Can observe (ls, query) the target scope |
| `write` | Can create/delete files in the target scope (touch, rm) |
| `execute` | Can trigger processes/workers in the target scope |
| `own` | Full control — can also grant permissions to others |
| `deny` | Explicit denial — overrides ALL grants |

### Examples

```
§read/databases/                     # Can read anything under databases/
§write/jobs/                         # Can create/delete jobs
§execute/workers/                    # Can start/stop workers
§own/databases/translations/         # Owns the translations subtree
§deny/hard/                          # Explicitly denied access to hard/
§read/_                              # Wildcard: can read EVERYTHING
```

### Wildcards

The `_` (void) logic door acts as a wildcard in permission paths:

```
§read/_                              # Read access to everything
§write/jobs/_                        # Write access to all jobs
§execute/workers/_                   # Execute access to all workers
```

## Group System

Groups aggregate permissions. They live under `src/hard/groups/`:

```
src/hard/groups/
    system/
        §read/_
        §write/_
        §execute/_
    admin/
        §read/_
        §write/hard/identities/
        §write/hard/groups/
        §execute/workers/
    developers/
        §read/databases/
        §write/jobs/
        §execute/workers/
    guests/
        §read/databases/
        §deny/hard/
```

An identity joins a group by having a `-group/{name}` file:
```
src/hard/identities/042/-group/developers
```

## Resolution Algorithm

When the engine receives an instruction, before dispatching:

1. **Identify the actor** — Which identity is making this request? (Determined by the active session.)

2. **Collect all permissions**:
   - Direct permissions: `§` entries under the identity's directory
   - Group permissions: for each `-group/{name}`, collect `§` entries from `hard/groups/{name}/`

3. **Check deny first** — Any matching `§deny/` rule? If yes → **REJECT**. Deny always wins.

4. **Check grants** — Any matching `§read/`, `§write/`, `§execute/`, or `§own/` rule that covers the target path? If yes → **ALLOW**.

5. **Default deny** — No matching rule? → **REJECT**. Everything is denied by default.

### Path Matching

A permission path matches a target path if:
- The permission path is a prefix of the target path (directory-level grant)
- Or they are exactly equal
- Or the permission path uses `_` wildcard, which matches any segment

```
Permission: §read/databases/
Target:     databases/colors/blue
Match:      YES (prefix match)

Permission: §write/jobs/
Target:     databases/colors/blue
Match:      NO

Permission: §read/_
Target:     anything
Match:      YES (wildcard)
```

## Authentication

### Password Storage

Passwords are hashed with **argon2id** and stored as zero-byte files whose **name** encodes the PHC-format hash. This preserves the fundamental rule: no file ever contains data.

```
src/hard/identities/001/-secret/.argon2id.v=19.m=19456,t=2,p=1.SALT.DIGEST
```

The PHC format (`$argon2id$v=19$m=19456,t=2,p=1$salt$digest`) is made filename-safe by replacing `$` with `.` (reversible, since `.` never appears inside PHC field values).

### First Boot

On first start, if no Omni-tier (0xx) identity has a `-secret/` child, Pith either:
- Prompts interactively for a password (if running in a TTY)
- Requires `pith init --password <pwd>` to have been run first (non-interactive environments)

This creates identity 001 with full permissions (`§read/_`, `§write/_`, `§execute/_`, `§own/_`).

### Authentication Flow

1. Client connects to the Unix socket → starts as Guest (800)
2. Client sends: `{"op": "authenticate", "args": {"identity": "001", "password": "..."}}`
3. Pith looks up `-secret/` in the trie, decodes the filename back to a PHC hash, verifies with argon2id
4. On success: session identity is **upgraded** from Guest to the authenticated identity
5. All subsequent operations use the new identity's permissions

### UID Auto-Binding

Identities can be mapped to Unix UIDs via `-uid/{unix_uid}` files:

```
src/hard/identities/601/-uid/502    # Unix UID 502 → identity 601
```

When a client connects, Pith extracts the peer UID via `peer_cred()` and automatically resolves it to the mapped identity (skipping the Guest default). The client can still call `authenticate` to switch to a different identity.

## Session Binding

Permissions are enforced per-session. When a client connects to Pith:

1. Pith extracts the connecting process's UID/PID via `peer_cred()` (Unix socket credentials)
2. The UID is resolved to an identity via the `-uid/` mapping (or defaults to Guest 800)
3. A session directory is created under `src/sessions/`
4. The client can upgrade their identity via the `authenticate` op
5. All operations are checked against the session's current identity permissions
6. On disconnect, the session is destroyed and its filesystem entries are cleaned up

```
src/sessions/
    ~0001/
        -identity/042               # This session acts as identity 042
        -uid/501                    # Peer Unix UID
        -state/active               # Session state
```

### Identity Management

Authenticated users with **Admin tier (4xx) or higher** can create new identities via the API:

```json
{"op": "create_identity", "args": {
    "id": "601",
    "name": "alice",
    "password": "secret123",
    "groups": ["developers"],
    "uid": 502
}}
```

This creates the full identity directory tree on disk. The `password`, `name`, `groups`, and `uid` fields are all optional.
