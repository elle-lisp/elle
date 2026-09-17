# subprocess

<!-- audited: 2026-09-16 -->

Spawning OS child processes, and the `subprocess` value every later call takes.

Up: [..](../AGENTS.md)

## Layout

| File | Contains |
|------|----------|
| `subprocess.rs` | The module root: `exit`, `halt`, `sys/pid`, `sys/args`, `sys/argv`, `sys/env`, and the primitive table for everything below |
| `subprocess/exec.rs` | `subprocess/exec` and the option parsing it runs first |
| `subprocess/handle.rs` | The `subprocess` value: its boundary check, its reads, and `wait`/`kill` |
| `subprocess/tests.rs` | `subprocess/kill` against a recorded exit status |

## The subprocess value

`subprocess/exec` answers one value — an external with type name `subprocess`,
carrying the child's pid, its three stdio port `Value`s, and the `ExitRecord`
that holds the exit status once anything reaps the child. It prints as
`#<subprocess 12345>`.

There is no wrapper struct and no `:process` key. The value *is* the process, so
a caller cannot hold the handle apart from the thing that owns it, and cannot
build something that resembles one.

**Capability bit:** `SIG_EXEC` (bit 11) gives subprocess operations their own
bit, so a fiber mask can allow or deny them independently of general I/O.
`subprocess/exec` and `subprocess/wait` emit `SIG_EXEC | SIG_IO`. `SIG_IO` is
what routes the request to the scheduler — `SIG_EXEC` selects no backend of its
own — but both bits route for a fiber mask: `|:exec|` catches a subprocess
request exactly as `|:io|` does (#895).

## One boundary, one message

`extract_subprocess` is the only way into a `ProcessHandle` from a primitive.
It checks the argument is a `subprocess` external and answers the handle, or
refuses with one message:

```
subprocess/wait: expected a subprocess, got struct
```

`wait`, `kill` and `pid` call it and get a handle they cannot doubt. None of
them repeats the check, because after the extractor there is nothing left to
check. A primitive added here calls the extractor rather than reaching for
`as_external` itself.

## Primitives

- `subprocess/exec program args [opts]` — Spawns a child. Answers a
  `subprocess`. Emits `SIG_EXEC | SIG_IO | SIG_YIELD`. Pipes are binary; text
  decoding is the caller's.
  - `program` (string): path to executable
  - `args` (list or array of strings): accepts empty list `()`, cons list,
    immutable array `[...]`, or mutable array `@[...]`
  - `opts` (optional struct): `:env` (struct, replaces the environment),
    `:cwd` (string), `:stdin`/`:stdout`/`:stderr` (`:pipe` default, `:inherit`,
    `:null`)
  - Error cases: non-sequence `args` → `type-error "subprocess/exec: args must
    be list, array, or @array, got {type}"`; non-string element → `type-error
    "subprocess/exec: args element must be string, got {type}"`; improper list
    → `type-error "subprocess/exec: improper list ending in {type}"`
  - `subprocess/system` (stdlib) gets sequence widening for free by passing
    `args` through unchanged.

- `subprocess/wait subprocess` — Waits for the child to exit. Answers the exit
  code as an integer (0 = success). Emits `SIG_EXEC | SIG_IO | SIG_YIELD`.
  Answers the same status however often it is called, because the reap records
  the status rather than spending it.

- `subprocess/kill subprocess [signal]` — Sends a signal, synchronously. Emits
  `SIG_ERROR` only. Default is `SIGTERM`. Answers `:signaled`, `:exited` or
  `:missing`. Each answer reports what the call observed and nothing beyond it;
  [subprocess](../../../docs/subprocess.md) owns the argument for why they are
  three answers and not one, and [io/](../../io/AGENTS.md) owns the record.

- `subprocess/pid subprocess` — The OS process ID, whether or not the child has
  been reaped. Emits `SIG_ERROR` only.

- `subprocess/exit subprocess` — The recorded exit status, or `nil` while the
  child runs. Reads the record; reaps nothing and never yields.

- `subprocess? value` — True for a `subprocess`, false for anything else. Never
  errors, matching `port?`.

## Reads go through the collection primitives

A subprocess answers `get`, `has?`, `keys` and `values` over a closed key set —
`:pid`, `:stdin`, `:stdout`, `:stderr`, `:exit` — in that order, which is fixed
rather than sorted. An unknown key reads as `nil`, or as the default `get` was
given.

Those four primitives live in `access/getop.rs` and `lstruct.rs`, not here, so
each dispatches to a reader this module owns rather than duplicating the key
set. `put`, `del` and `merge` refuse a subprocess: it reports what a child is
doing and is not a place to write fields.

`keys` answering a fixed order is a deliberate difference from a struct, whose
keys come back in `TableKey` hash order. The key set is declared here, so its
order is ours to choose, and a caller reading `(keys p)` gets the same sequence
on every run.

## Region effects

`subprocess/exec` declares `Opaque`: it copies every argument out into the
`SpawnRequest` (Rust `String`/`Vec`) and stores none, while the value it answers
is minted on the scheduler heap and delivered by a fiber resume. Pinned by
`subprocess_exec_declares_opaque_no_arg_clique`.

The reads that answer a port declare `PassThrough` — the port lives in the
subprocess's own region, never in the call's. `subprocess/pid`, `subprocess/exit`
and `subprocess?` answer immediates and declare `Immediate`. See
[region effects](../../../docs/impl/region/effects.md).
