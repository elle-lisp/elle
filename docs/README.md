# Elle Documentation

<!-- audited: 2026-09-23 -->

This directory contains language references, design documents, and contributor
guides. See [QUICKSTART.md](../QUICKSTART.md) for the full table of contents.

## These files are programs

Every `.md` file in this directory is simultaneously a piece of documentation
**and** a runnable Elle program. The reader recognizes `.md` as a first-class
source format: when you run

```sh
elle docs/control.md
```

the reader extracts every fenced code block tagged ` ```lisp ` or ` ```elle `,
replaces all other lines (prose, tables, other code fences) with blank lines
so source positions line up with the original markdown, and feeds the result
to the standard s-expression reader. Error messages point back to the exact
`.md` line and column.

This means these files serve three roles at once:

- **Documentation** — readable on GitHub, in your editor, or rendered on the
  generated site.
- **Demos and examples** — every code sample is real code that the last test
  run compiled, and ran unless it needs something the build does not have;
  there are no stale snippets that "used to work".
- **Tests** — `make doctest` runs [README.md](../README.md),
  [QUICKSTART.md](../QUICKSTART.md), [INSTALL.md](../INSTALL.md) and every
  `.md` file under `lib/` and `docs/`, and fails loudly if anything stops
  working. `make doctest-list` prints that set. When you change an interface,
  the doc for that interface either updates or breaks the build.

Write docs the same way you'd write a test: pick something to demonstrate,
show the code, assert the result. Anything you put in a ` ```lisp ` block
is code the build will execute. Anything outside a fenced block is prose
that the reader will skip. See [`impl/reader.md`](impl/reader.md) for the
full pipeline and [`../src/reader/mod.rs`](../src/reader/mod.rs)'s
`strip_markdown` for the exact extraction rules.

## Only a lisp fence holds Elle code

A fence tagged anything but ` ```lisp ` or ` ```elle ` is never run, so it
never holds Elle code. It holds output, a shell session, a table, a diagram,
or a language that is not Elle, and its tag names that language. A fence with
no tag is text.

[tests/integration/doctest.rs](../tests/integration/doctest.rs) enforces this
in every document `make doctest` runs. It fails on a fence with a line that
opens a form, such as `(defn` or `(fiber/new`, unless the fence's tag names a
language that is not Elle: `rust`, `sh`, `bash`, `sparql`, `json`, `toml`,
`sql`, `turtle` or `mermaid`. A value a program prints, such as `{:ok 1}`
or `(1 2 3)`, opens with no symbol and stays in a text fence.

A snippet that cannot run by itself still goes in a lisp fence, with the
scaffolding it needs:

- **A compile error.** Compile the source through the file front end and
  assert that it fails: `(protect (compile/whole-module src "<doc>"))`.
- **A plugin the build does not make, a server, or a device.** Define a
  function that the document never calls, so the build compiles it without
  running it. Take the plugin as a parameter: a literal `"plugin/NAME"`
  import must name a plugin `make doctest` builds.
- **A signature or a synopsis.** Write it as inline code in the prose or in a
  table, not in a fence.

## Language Topics

Focused files covering one topic each, all runnable via `elle docs/<file>.md`.

[syntax](syntax.md) [types](types.md) [bindings](bindings.md)
[destructuring](destructuring.md) [destructuring-advanced](destructuring-advanced.md)
[functions](functions.md) [named-args](named-args.md) [arrays](arrays.md)
[structs](structs.md) [sets](sets.md) [strings](strings.md) [bytes](bytes.md)
[control](control.md) [loops](loops.md) [match](match.md) [errors](errors.md)
[concurrency](concurrency.md) [threads](threads.md)
[parameters](parameters.md) [traits](traits.md) [io](io.md)
[subprocess](subprocess.md) [lua](lua.md)
[epochs](epochs.md) [intrinsics](intrinsics.md) [compile-time](compile-time.md)

## Design Documents

| Document | Content |
|-----------|---------|
| [processes.md](processes.md) | Erlang-style processes: mailboxes, links, monitors |
| [process-scheduler.md](process-scheduler.md) | Sub-fibers, forwarded I/O and nested schedulers inside processes |
| [behaviors.md](behaviors.md) | GenServer, Actor, Task, EventManager |
| [supervisor.md](supervisor.md) | Supervisors: child specs, restart strategies, supervised subprocesses |
| [signals/](signals/) | Signal system design, protocol, inference, JIT |
| [signals/fibers.md](signals/fibers.md) | Fiber architecture |

## Contributor Guides

| Directory | Content |
|-----------|---------|
| [cookbook/](cookbook/) | Recipes: primitives, heap types, bytecode, plugins |
| [analysis/](analysis/) | Testing strategy, debugging, portraits |
| [pipeline.md](pipeline.md) | Compilation pipeline |

## Implementation

| Directory | Content |
|-----------|---------|
| [impl/](impl/) | Reader, lexicon, syntax, HIR, LIR, bytecode, VM, JIT, WASM, MLIR, SPIR-V, GPU, values, symbols, stdlib cache |

## Reference

| File | Content |
|------|---------|
| [plugins.md](plugins.md) | Rust plugins and `std/` modules, and how to build one |
| [stdlib.md](stdlib.md) | Standard library and prelude |
| [modules.md](modules.md) | Import system |
| [macros.md](macros.md) | Macro system |
| [ffi.md](ffi.md) | C interop |
| [embedding.md](embedding.md) | Embedding Elle in Rust/C |

## Quick Navigation

- **Starting out?** Read [QUICKSTART.md](../QUICKSTART.md)
- **Adding a feature?** Check [cookbook/](cookbook/)
- **Understanding signals?** Read [signals/](signals/)
- **Writing tests?** Read [analysis/](analysis/)
