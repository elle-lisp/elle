# Elle Examples

Executable programs that demonstrate Elle's features. Each file is a
self-contained application — run it, read the output, read the source.

```bash
cargo run -- examples/hello.lisp         # start here
cargo run -- examples/basics.lisp        # then explore
cargo test --test '*'                    # run them all
```

## Files

Start with [`hello.lisp`](hello.lisp) and work down:

| File | What it is |
|------|------------|
| [`hello.lisp`](hello.lisp) | Smoke test — one line, proves the toolchain works |
| [`basics.lisp`](basics.lisp) | Type system tour: immediates, truthiness, arithmetic, the mutable/immutable split, bytes, boxes, equality |
| [`functions.lisp`](functions.lisp) | A gradebook built with `defn`/`fn`, closures, higher-order functions, composition, pipelines, variadic and mutual recursion |
| [`control.lisp`](control.lisp) | An expression evaluator grown section by section: `if`, `cond`, `case`, `when`/`unless`, `if-let`/`when-let`, `while`, `forever`, `block`/`break`, full `match` patterns, `->` / `->>` |
| [`collections.lisp`](collections.lisp) | A contact book app exercising literal syntax, `get`/`put`, destructuring, `each`, threading, splice, string ops, grapheme clusters |
| [`destructuring.lisp`](destructuring.lisp) | Unpacking data: silent nil semantics, wildcards, `& rest`, nested patterns, struct/table by-key, match dispatch on struct tags |
| [`errors.lisp`](errors.lisp) | Error handling: `error`, `try`/`catch`, `protect`, `defer`, `with`, propagation, safe wrappers, validation |
| [`coroutines.lisp`](coroutines.lisp) | Cooperative sequences: `coro/new`, `yield`, lifecycle, Fibonacci generator, interleaving, nesting, `yield*` delegation |
| [`meta.lisp`](meta.lisp) | Macros and hygiene: `defmacro`, quasiquote/unquote, `gensym`, `datum->syntax`, `syntax->datum` |
| [`concurrency.lisp`](concurrency.lisp) | Parallel threads: `spawn`, `join`, closure capture across threads, parallel computation |
| [`processes.lisp`](processes.lisp) | Erlang-style actors: fiber-based scheduler, message passing with `send`/`recv`, links, `trap-exit`, crash propagation |
| [`io.lisp`](io.lisp) | Files, JSON, modules: `slurp`/`spit`, paths, directories, `json-parse`/`json-serialize`, `import-file` |
| [`introspection.lisp`](introspection.lisp) | Looking inside: clock primitives, `time/elapsed`, closure introspection, `disbit`/`disjit`, benchmarking |
| [`ffi.lisp`](ffi.lisp) | C interop: `ffi/native`, `ffi/defbind`, structs, variadic calls, callbacks (`qsort`) |

[`assertions.lisp`](assertions.lisp) is a shared assertion library loaded by
all other files. It provides `assert-eq`, `assert-true`, `assert-false`,
`assert-list-eq`, `assert-not-nil`, and `assert-string-eq`.

Every file exits 0 on success, 1 on failure. CI runs each one with a
10-second timeout.
