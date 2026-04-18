# Elle Documentation

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
- **Demos and examples** — every code sample is real code that was executed
  the last time the tests ran; there are no stale snippets that "used to work".
- **Tests** — `make doctest` runs every `.md` file under `docs/` and fails
  loudly if anything stops working. When you change an interface, the doc
  for that interface either updates or breaks the build.

Write docs the same way you'd write a test: pick something to demonstrate,
show the code, assert the result. Anything you put in a ` ```lisp ` block
is code the build will execute. Anything outside a fenced block is prose
that the reader will skip. See [`impl/reader.md`](impl/reader.md) for the
full pipeline and [`../src/reader/mod.rs`](../src/reader/mod.rs)'s
`strip_markdown` for the exact extraction rules.

## Language Topics

Focused files covering one topic each, all runnable via `elle docs/<file>.md`.

`syntax` `types` `bindings` `destructuring` `destructuring-advanced`
`functions` `named-args` `arrays` `structs` `sets` `strings` `bytes`
`control` `loops` `match` `errors` `concurrency` `threads` `coroutines`
`parameters` `traits` `io` `lua` `epochs`

## Design Documents

| Directory | Content |
|-----------|---------|
| [processes.md](processes.md) | Erlang-style processes, GenServer, supervisors |
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
| [impl/](impl/) | Reader, HIR, LIR, bytecode, VM, JIT, WASM, MLIR, SPIR-V, GPU, values |

## Reference

| File | Content |
|------|---------|
| [plugins.md](plugins.md) | 29 shipped plugins |
| [stdlib.md](stdlib.md) | Standard library and prelude |
| [modules.md](modules.md) | Import system |
| [macros.md](macros.md) | Macro system |
| [ffi.md](ffi.md) | C interop |

## Quick Navigation

- **Starting out?** Read [QUICKSTART.md](../QUICKSTART.md)
- **Adding a feature?** Check [cookbook/](cookbook/)
- **Understanding signals?** Read [signals/](signals/)
- **Writing tests?** Read [analysis/](analysis/)
