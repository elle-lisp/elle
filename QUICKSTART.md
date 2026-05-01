# Elle Quickstart

Elle is a Lisp with lexical scope, closures, and a signal system.

## Critical gotchas

Read these first — they cause the most bugs.

- **`nil` ≠ `()`** — `nil` is falsy; `()` is the empty list and is
  truthy. Use `empty?` to check end-of-list, not `nil?`.
  See [docs/empty-list.md](docs/empty-list.md) for why.
- **`#` is comment, `;` is splice** — `;[1 2 3]` spreads into the
  surrounding form.
- **`assign` mutates; `set` creates a set** — `(assign x 10)` changes
  `x`; `(set x 10)` creates the set `|x 10|`.
- **Only `nil` and `false` are falsy** — `0`, `""`, and `()` are truthy.
- **Bare = immutable; `@` = mutable** — `[1 2]` is immutable, `@[1 2]`
  is mutable.
- **`let` is sequential** (Clojure-style) — bindings are flat pairs
  `[a 1 b 2]`; each binding sees all previous ones. `let*` is an alias.

## Running

```bash
elle script.lisp          # run a file
elle script.md            # run literate markdown
echo '(+ 1 2)' | elle     # one-liner
elle                       # REPL
make smoke                 # run all tests (~30s)
```

## Language topics

| File | Content |
|------|---------|
| [syntax](docs/syntax.md) | Literals, comments, splice, quoting, collection literals |
| [types](docs/types.md) | Type predicates, conversions, truthiness, equality |
| [bindings](docs/bindings.md) | def, var, let, letrec, assign, scope rules |
| [destructuring](docs/destructuring.md) | List, array, struct patterns |
| [destructuring-advanced](docs/destructuring-advanced.md) | Rest, wildcard, nested, match integration |
| [functions](docs/functions.md) | fn, defn, closures, higher-order, sorting |
| [named-args](docs/named-args.md) | &named, &keys, &opt, default |
| [arrays](docs/arrays.md) | Array/\@array: get, put, push, pop, concat |
| [structs](docs/structs.md) | Struct/\@struct: get, put, merge, accessor syntax |
| [sets](docs/sets.md) | Set literals, union, intersection, difference |
| [strings](docs/strings.md) | String ops (string/ prefix), graphemes |
| [bytes](docs/bytes.md) | Binary data, hex encoding |
| [control](docs/control.md) | if, cond, case, when, unless, begin, block, break |
| [loops](docs/loops.md) | while, forever, each, repeat |
| [match](docs/match.md) | Pattern matching with guards |
| [errors](docs/errors.md) | error, try/catch, protect, defer, with |
| [signals](docs/signals/) | Signal system, silence, squelch |
| [fibers](docs/signals/fibers.md) | Fiber basics, signal masks, status |
| [concurrency](docs/concurrency.md) | ev/spawn, ev/join, ev/race, ev/scope, processes |
| [threads](docs/threads.md) | OS threads, channels |
| [coroutines](docs/coroutines.md) | coro/new, coro/resume, generators |
| [parameters](docs/parameters.md) | Dynamic parameters, parameterize |
| [macros](docs/macros.md) | defmacro, syntax-case, hygiene |
| [modules](docs/modules.md) | import, closure-as-module pattern |
| [traits](docs/traits.md) | with-traits, trait dispatch |
| [portrait](docs/analysis/portrait.md) | Semantic analysis and portraits |
| [io](docs/io.md) | Ports, file I/O, subprocesses |
| [ffi](docs/ffi.md) | C interop, libloading, callbacks |
| [lua](docs/lua.md) | Lua syntax mode |
| [epochs](docs/epochs.md) | Epoch migration system |

## Runtime and internals

| File | Content |
|------|---------|
| [runtime](docs/runtime.md) | Runtime signals, fuel budgets |
| [scheduler](docs/scheduler.md) | Async scheduler, io_uring |
| [embedding](docs/embedding.md) | Using Elle as a library |
| [memory](docs/memory.md) | Arenas, scope allocation |
| [processes](docs/processes.md) | Erlang-style processes, GenServer, supervisors |

## Implementation

| File | Content |
|------|---------|
| [reader](docs/impl/reader.md) | Lexer, parser, source locations |
| [hir](docs/impl/hir.md) | Binding resolution, signal inference |
| [lir](docs/impl/lir.md) | SSA form, virtual registers |
| [bytecode](docs/impl/bytecode.md) | Instruction set, encoding |
| [vm](docs/impl/vm.md) | Stack machine, dispatch |
| [jit](docs/impl/jit.md) | Cranelift JIT compilation |
| [wasm](docs/impl/wasm.md) | WebAssembly backend (Wasmtime) |
| [mlir](docs/impl/mlir.md) | MLIR/LLVM CPU tier-2 backend |
| [spirv](docs/impl/spirv.md) | SPIR-V emission for GPU compute |
| [gpu](docs/impl/gpu.md) | End-to-end GPU compute (Vulkan) |
| [values](docs/impl/values.md) | Value representation, tagged union |

## Reference

| File | Content |
|------|---------|
| [plugins](docs/plugins.md) | 29 shipped plugins |
| [stdlib](docs/stdlib.md) | Standard library and prelude |
| [testing](docs/analysis/testing.md) | Test patterns, make smoke/test |
| [debugging](docs/analysis/debugging.md) | Debugging and introspection |
| [cookbook](docs/cookbook/index.md) | Recipes for cross-cutting changes |
| [DEVLOG](DEVLOG.md) | Per-PR development log (generated from diffs) |
| [CHANGELOG](CHANGELOG.md) | Changelog by subsystem arc (agent-optimized) |
