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
        -name/alice                 # Human-readable name
        -group/admin                # Group membership (one file per group)
        -group/developers           # Can belong to multiple groups
        §read/databases/            # Direct permission grant
        §write/jobs/
    002/
        ...
```

Identity numbers range from `001` to `777`. The number IS the identity — no content needed.

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

## Session Binding

Permissions are enforced per-session. When a developer connects to Pith (via CLI or SDK):

1. A session is created under `src/sessions/`
2. The session is bound to an identity
3. All operations through that session are checked against that identity's permissions

```
src/sessions/
    ~0001/
        -identity/042               # This session acts as identity 042
        -state/active               # Session state
```
