# Naming Conventions

## Grammar

Every file or directory name is parsed by its first character:

```
NAME := €LITERAL          → Pointer node (escape: treat rest as literal)
      | RESERVED REST     → Instruction node (first char = logic door)
      | DATA              → Data node (plain text, the name IS the value)
```

Where `RESERVED` is any character present in `src/hard/reserved/`.

## Segment Classification

When the engine parses a path, it splits by `/` and classifies each segment:

| First char | Classification | Example | Reading |
|-----------|---------------|---------|---------|
| Logic door | Instruction | `-expected` | state: expected |
| `€` | Escaped pointer | `€$price` | literal "$price" |
| Anything else | Data | `blue` | data value "blue" |

## Path as Sentence

A complete path is read left-to-right. Each directory pushes a context. The leaf file is the assertion.

```
databases / translations / english / french / colors / blue / bleu
    │            │            │         │        │       │      │
  scope       scope        source    target    scope    key   value
```

> "In databases, in translations, from english to french, in colors, blue translates to bleu."

```
hard / identities / 001 / -expected / type / identity
  │        │         │        │         │       │
scope    scope     slot    state:     scope   leaf
                          expected
```

> "In the hard system, identity 001, at state expected, of type identity."

## Naming Rules

### Data Nodes
- Use lowercase with underscores: `bad_sleep`, `list_of_effects`
- Numbers are valid: `001`, `777`
- Spaces should be avoided (use `_`)
- Unicode data is valid: `bleu`, `rouge`

### Instruction Nodes
- Always start with a logic door character
- The rest of the name is the argument: `-state` = state pointer to "state"
- Can be nested: `-expected/type/identity` = state "expected" containing type "identity"

### Pointer Nodes
- Always start with `€`
- Used to escape a logic door character when it must be treated as data
- `€#hashtag` = data "#hashtag", not a channel instruction

### Cross-References
- Use `∆` (U+2206) markers for bidirectional links
- Format: `∆{source_scope}∆{target_scope}`
- Example: `∆psychology∆blue` in `colors/blue/psychology/` links back to `psychology/blue/`

### Numeric Values
- Use `~` prefix for numbers: `~42`, `~5`
- Can combine with other doors: `^~5` = priority 5

### Raw Values
- Use `()` for literal text: `(hello world)`
- Everything between `(` and `)` is a raw string, no logic door interpretation

## Reserved Characters in Names

If a filename must contain a reserved character as data (not as an instruction), escape it with `€`:

| You want | You write | Why |
|----------|-----------|-----|
| A file named `$100` | `€$100` | `$` is a logic door (schema); `€` escapes it |
| A file named `#channel` | `€#channel` | `#` is a logic door (IPC); `€` escapes it |
| A file named `€` | `€€` | `€` itself needs escaping |
| A file named `hello` | `hello` | No logic door prefix, no escape needed |

## Directory Depth Semantics

The depth of a path carries meaning:

| Depth | Role | Example |
|-------|------|---------|
| 1 | System scope | `hard/`, `jobs/`, `databases/` |
| 2 | Domain | `hard/identities/`, `databases/colors/` |
| 3 | Entity | `hard/identities/001/`, `databases/colors/blue/` |
| 4+ | Properties/Details | `001/-expected/type/identity` |

## Filesystem Constraints

- Maximum filename length: 255 bytes (filesystem limit)
- Forbidden characters in names: `/` (path separator), null byte
- All other Unicode characters are valid in filenames
- Case sensitivity: preserved and significant (`Blue` ≠ `blue`)
