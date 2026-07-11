# Reserved Values — The Logic Door Alphabet

## Philosophy

In {€}, reserved values are **logic doors**: single characters that act as transformer functions. When a filename or path segment begins with a logic door, it becomes an **instruction node** — not data.

The engine discovers the alphabet at boot by reading `src/hard/reserved/`. Each zero-byte file in that directory IS a logic door declaration. The alphabet is **self-describing**: add a file, extend the language.

## The € Escape Mechanism

`€` (U+20AC) is the universal escape. Its rule is absolute:

> **The character immediately following `€` is NEVER interpreted as a logic door.**

This exists because:
1. It prevents cyclical escaping problems — `\` collides with network/shell escaping. `€` is used by no other system.
2. It gives {€} its name and its fundamental power: the ability to contain any string safely.
3. In a {€} compiled string, `€` is the only logic door that can suppress the next logic door.

### Example

- `$schema` → instruction: resolve schema named "schema"
- `€$schema` → pointer: literal string "$schema", not an instruction
- `€€` → pointer: literal `€` character

## Core Logic Doors (Data Structures)

These logic doors define data structure operations. They compose in filenames and in {€} compiled strings.

| Char | Unicode | Name | Semantics |
|------|---------|------|-----------|
| `€` | U+20AC | Escape | Next char is literal, not a logic door |
| `$` | U+0024 | Schema | Next chars point to a schema definition |
| `[` | U+005B | Array Open | Begins a list/array; next char is its element type |
| `]` | U+005D | Array Close | Ends a list/array definition |
| `\|` | U+007C | Value Separator | Completes one value definition, begins another |
| `,` | U+002C | Object Separator | Completes one object definition, begins another |
| `-` | U+002D | State | Next chars point to a state |
| `*` | U+002A | Compiled | Next value is of "compiled with {€}" type |
| `+` | U+002B | Constant | Next chars point to a constant |
| `{` | U+007B | Object Open | Begins an object; next char is its type |
| `}` | U+007D | Object Close | Ends an object definition |
| `(` | U+0028 | Raw Open | Next chars until `)` are a raw/literal value |
| `)` | U+0029 | Raw Close | Ends a raw value definition |
| `@` | U+0040 | Dict Key | Next chars are the key of the dictionary at pointer @ |
| `~` | U+007E | Number | Defines a number starting at next char until next non-digit char |
| `:` | U+003A | Binding | Binds a name to a value (key:value in a scope) |

## OS Logic Doors (Operations & Control)

These logic doors extend the alphabet for operating system primitives.

| Char | Unicode | Name | Semantics |
|------|---------|------|-----------|
| `#` | U+0023 | Channel | Defines or references an IPC communication channel |
| `!` | U+0021 | Signal | Emits or receives a signal/event (fire-and-forget) |
| `?` | U+003F | Query | Requests/reads a value without mutation |
| `^` | U+005E | Priority | Next value is a priority level |
| `&` | U+0026 | Async | Marks an operation as asynchronous/background |
| `%` | U+0025 | Modulo | Arithmetic modulo or proportional value |
| `<` | U+003C | Input | Reads input from a source |
| `>` | U+003E | Output | Directs output to a target |
| `=` | U+003D | Assert | Asserts equality or defines a condition |
| `;` | U+003B | Sequence | Separates sequential operations in a single name |
| `_` | U+005F | Void | Placeholder / wildcard (matches anything) |

## Extended Logic Doors (Unicode — Processes & Control Flow)

These use visually distinct Unicode characters for advanced OS operations.

| Char | Unicode | Name | Semantics |
|------|---------|------|-----------|
| `§` | U+00A7 | Permission | Defines an access control rule (read/write/execute/own/deny) |
| `¶` | U+00B6 | Process | Marks a process definition or reference |
| `∂` | U+2202 | Delta | Marks a change/diff operation |
| `λ` | U+03BB | Lambda | Defines an anonymous function/transformation |
| `∴` | U+2234 | Then | Conditional "then" branch |
| `∵` | U+2235 | Because | Conditional guard/reason |
| `∞` | U+221E | Loop | Marks a repeating/looping construct |
| `▶` | U+25B6 | Start | Starts or resumes a process |
| `⏸` | U+23F8 | Pause | Suspends a process |
| `⏹` | U+23F9 | Stop | Stops/kills a process |
| `⌚` | U+231A | Timer | Schedules a timed operation |

## Relational Markers

These characters are used within data names (not as leading logic doors) to express relationships.

| Char | Unicode | Name | Semantics |
|------|---------|------|-----------|
| `∆` | U+2206 | Join | Relational cross-reference between two scopes |
| `→` | U+2192 | Arrow | Explicit directional pointer |

### Example: `∆psychology∆blue`
This filename at path `databases/colors/blue/psychology/` creates a bidirectional link: "colors/blue is related to psychology, and psychology is related to blue."

## Reading Logic Doors in Paths

A path is read left-to-right. Each `/`-separated segment is classified by its first character:

```
/src/hard/identities/001/-expected/type/identity
      │         │     │      │       │      │
      data     data  data  state   data   data
                            │
                            └─ "-" = logic door: next points to a state
```

Full reading: "In the hard system, in identities, slot 001, at state 'expected', of type 'identity'."

## Compiled String Examples

In a {€} compiled string, logic doors compose without `/` separators:

```
$[{*|232}]          → schema of: array of: object of: compiled value OR 232
[(this is a text)]  → array of: raw value "this is a text"
{€€,("\text)}       → object of: escaped euro, then: raw value "\text"
~42                 → number: 42
^~5                 → priority: number 5
!completed          → signal: completed
#main               → channel: main
§read               → permission: read
```
