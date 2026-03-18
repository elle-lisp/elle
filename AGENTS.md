# Elle

Elle is a Lisp. Source text becomes bytecode; bytecode runs on a register-based VM.

This is not a toy. The implementation targets correctness, performance, and
clarity — in that order. We compile through multiple IRs, we have proper
lexical scoping with closure capture analysis, and we have a signal system.

You are an LLM. You will make mistakes. The test suite will catch them. Run the
tests. Read the error messages. They are designed to be helpful.

## Contents

- [Architecture](#architecture)
- [Products](#products)
- [Directories](#directories)
- [Testing](#testing)
- [Invariants](#invariants)
- [Intentional oddities](#intentional-oddities)
- [Conventions](#conventions)
- [Maintaining documentation](#maintaining-documentation)
- [Where to start](#where-to-start)

## Architecture

```
Source → Reader → Syntax → Expander → Syntax → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM
```

This is the only compilation pipeline. Source locations flow through the entire
pipeline: Syntax spans → HIR spans → LIR `SpannedInstr` → `LocationMap` in
bytecode. Error messages include file:line:col information.

### Key modules

| Module | Responsibility |
|--------|----------------|
| `reader` | Lexing and parsing to `Syntax` |
| `syntax` | Syntax types, macro expansion |
| `hir` | Binding resolution, capture analysis, signal inference, linting, symbol extraction, docstring extraction |
| `lir` | SSA form with virtual registers, basic blocks, `SpannedInstr` for source tracking |
| `compiler` | Bytecode instruction definitions, debug formatting |
| `vm` | Bytecode execution, builtin documentation storage |
| `value` | Runtime value representation (NaN-boxed); trait table field on 19 user-facing heap variants |
| `signals` | Signal type (`Silent`, `Yields`, `Polymorphic`), signal registry for keyword-to-bit mapping; includes `SIG_EXEC` (bit 11) for subprocess operations and `SIG_FUEL` (bit 12) for instruction budget exhaustion |
| `io` | I/O request types, backends, timeout handling; includes `PortKind::Pipe` for subprocess stdio and `ProcessHandle` for subprocess lifecycle |
| `lint` | Diagnostic types and lint rules |
| `symbols` | Symbol index types for IDE features |
| `primitives` | Built-in functions; includes `port/path`, `string/size-of`, `with-traits`, `traits`, `sys/args` (returns args after the source file in argv, empty if none), `sys/env` (returns env as struct with string keys; optional single-var lookup), `subprocess/exec`, `subprocess/wait`, `subprocess/kill`, `subprocess/pid`, `syntax-pair?`, `syntax-list?`, `syntax-symbol?`, `syntax-keyword?`, `syntax-nil?`, `syntax->list`, `syntax-first`, `syntax-rest`, `syntax-e`, `fiber/set-fuel`, `fiber/fuel`, `fiber/clear-fuel`, `meta/origin` (returns source location of a closure as `{:file :line :col}`, or `nil`), `file/stat` (filesystem metadata struct, follows symlinks), `file/lstat` (filesystem metadata struct, does not follow symlinks) |
| `stdlib` | Standard library functions (loaded at startup); includes stream combinators (`port/lines`, `port/chunks`, `port/writer`, `stream/map`, `stream/filter`, `stream/take`, `stream/drop`, `stream/concat`, `stream/zip`, `stream/for-each`, `stream/fold`, `stream/collect`, `stream/into-array`, `stream/pipe`) and subprocess convenience (`subprocess/system`) |
| `ffi` | C interop via libloading/bindgen |
| `jit` | JIT compilation via Cranelift; `JitRejectionInfo` tracks why closures were rejected |
| `formatter` | Code formatting for Elle source |
| `plugin` | Dynamic plugin loading for Rust cdylib primitives; available plugins: `elle-oxigraph` (RDF/SPARQL), `elle-sqlite`, `elle-crypto`, `elle-regex`, `elle-glob`, `elle-random`, `elle-selkie` (HTTP) |
| `path` | UTF-8 path operations |
| `pipeline` | Compilation entry points (see [`src/pipeline/AGENTS.md`](src/pipeline/AGENTS.md)) |
| `error` | `LocationMap` for bytecode offset → source location mapping |

### The Value type

`Value` is the runtime representation using NaN-boxing. Create values via methods like `Value::int()`, `Value::cons()`, `Value::closure()` rather than enum variants. Notable types:
- `Closure` — bytecode + captured environment + arity + signal + `location_map` + `doc` + `syntax` + `traits`
- `LBox` / `LocalLBox` — mutable lboxes for captured variables
- `Fiber` — independent execution context with stack, frames, signal mask
- `Parameter` — dynamic binding with default value, looked up at runtime
- `External` — opaque plugin-provided Rust object (`Rc<dyn Any>` with type name)

All heap-allocated values use `Rc`. Mutable values use `RefCell`.

**Trait table field:** Every user-facing heap variant carries a `traits: Value` field (8 bytes). Initialized to `Value::NIL` (meaning "no traits"). Only an immutable `LStruct` may be stored here; the `with-traits` primitive validates this at call time. The field is invisible to structural equality, ordering, and hashing.

**Variants that carry `traits` (19 types):** `LArray`, `LArrayMut`, `LStruct`, `LStructMut`, `LString`, `LStringMut`, `LBytes`, `LBytesMut`, `LSet`, `LSetMut`, `Cons`, `Closure`, `LBox`, `Fiber`, `Syntax`, `ManagedPointer`, `External`, `Parameter`, `ThreadHandle`.

**Variants that do NOT carry `traits` (6 infrastructure types):** `Float`, `NativeFn`, `LibHandle`, `Binding`, `FFISignature`, `FFIType`. `with-traits` on these returns a `:type-error`.

## Products

| Product | Path | Purpose |
|---------|------|---------|
| elle | `src/` | Interpreter/compiler (includes `lint`, `lsp`, and `rewrite` subcommands) |
| docgen | `demos/docgen/` | Documentation site generator (written in Elle) |
| lib/http.lisp | `lib/` | Pure Elle HTTP/1.1 client and server |

## Directories

| Path | Contains |
|------|----------|
| `src/` | Core interpreter/compiler |
| `src/io/` | I/O request types and backends |
| `src/lsp/` | Language server protocol implementation |
| `lib/` | Reusable Elle modules (HTTP, etc.) |
| `examples/` | Executable semantics documentation |
| `tests/` | Unit, integration, property tests |
| `benches/` | Criterion and IAI benchmarks |
| `docs/` | Design documents and guides |
| `demos/` | Comparison implementations |
| `plugins/` | Dynamically-loaded plugin crates (cdylib) |
| `site/` | Generated documentation site |

## Testing

**⚠️ NEVER run `cargo test --workspace` without explicit user instruction.** It takes ~30 minutes. Use `make test` (~2min) for pre-commit verification.

| Command | Runtime | What it does |
|---------|---------|-------------|
| `make smoke` | ~15s | Elle examples only |
| `make test` | ~2min | build + examples + elle scripts + unit tests |
| `cargo test --workspace` | ~30min | full suite — **ask first** |

For test organization, helpers, and how to add tests: [`docs/testing.md`](docs/testing.md).
For CI structure and failure triage: [`docs/debugging.md`](docs/debugging.md).

## Invariants

These must remain true. Violating them breaks the system:

1. **Bindings are resolved at analysis time.** HIR contains `Binding` (NaN-boxed
   Value pointing to heap `BindingInner`), not symbols. If you see symbol
   lookup at runtime, something is wrong.

2. **Closures capture by value into their environment.** Immutable captured
   locals are captured directly. Mutable captured locals and mutated parameters
   use `LocalLBox` for indirection. The `lbox_params_mask` on `Closure` tracks
   which parameters need lbox wrapping.

3. **Signals are inferred, not declared — except when `silence` provides explicit bounds.** The `Signal` type (`Silent`, `Yields`, `Polymorphic`) propagates from leaves to root during analysis. `silence` constrains inference; it doesn't replace it. The inferred signal must be a subset of the declared bound. When a parameter has a `silence` bound, it is no longer polymorphic — its signal is known to be zero bits.

4. **The VM is stack-based for operands, register-addressed for locals.**
   Instructions reference registers (locals) by index. Results push to the
   operand stack.

5. **Errors propagate.** Functions return `LResult<T>`. Silent failure is
   forbidden. If you catch an error, you must either handle it meaningfully or
   propagate it.

## Intentional oddities

Things that look wrong but aren't. The 4 most critical (agents get these wrong):

- **Elle has no `-e` flag.** To run one-liners, use `echo '(expr)' | elle`.

- **Elle has no `-` flags at all.** See `elle --help`.

- **`nil` vs `()` are distinct.** `nil` is falsy (absence). `()` is truthy (empty list).
  Lists terminate with `EMPTY_LIST`. Use `empty?` (not `nil?`) for end-of-list.
  **Getting this wrong causes infinite recursion.**

- **`#` is comment, `;` is splice.** `#` starts a comment. `;expr` is the splice operator
  (array-spreading). `true`/`false` are booleans (not `#t`/`#f`).

- **`assign` not `set` for mutation.** `(assign var value)` mutates. `(set x val)` creates
  a set value. Agents reflexively write `(set x val)` — this is wrong.

- **`silence` is compile-time total suppression; `squelch` is a runtime closure transform.** `(silence param)` inside a lambda body constrains a parameter: it must be a silent closure. `(squelch f :yield)` is a *primitive function call*, not a declaration — it takes a closure and returns a **new** closure that catches `:yield` at runtime and converts it to `:error`. Usage: `(let ((safe-f (squelch f :yield))) (safe-f))`. `(squelch f)` with no keywords is an arity error (requires at least 2 arguments). `(squelch non-closure :yield)` is a type error.

- **Collection literals: bare = immutable, `@` = mutable.** `[...]` → array, `@[...]` → @array.
   `{...}` → struct, `@{...}` → @struct. `|...|` → set, `@|...|` → @set.
   `"..."` → string, `@"..."` → @string. `(bytes ...)` → bytes, `(@bytes ...)` → @bytes.

For the full list of oddities (18 items): [`docs/oddities.md`](docs/oddities.md).

## Conventions

- Files and directories: lowercase, single-word when possible.
- Target file size: 500 lines / 15KB. Dispatch tables up to 800 lines.
- Prefer formal types over hashes/maps for structured data.
- Validation at boundaries, not recovery at use sites.
- Tests reflect architecture: unit tests for modules, integration tests for
  pipelines, property tests for invariants.
- Do not add backward compatibility machinery. Breaking changes are fine.
- Do not silently swallow errors. Propagate or log with context.

## Maintaining documentation

AGENTS.md and README.md files exist throughout the codebase. Keep them current:

- **When you change a module's interface**, update its AGENTS.md. Changed
  exports, new invariants, altered data flow — these matter to the next agent.

- **When you add a new module**, create AGENTS.md (for agents) and README.md
  (for humans). Copy structure from a sibling module.

- **When you violate a documented invariant**, either fix your code or update
  the invariant. Stale invariants are worse than none.

- **When you discover undocumented behavior**, document it. If it's intentional,
  add to `docs/oddities.md`. If it's a bug, file an issue.

Documentation debt compounds. A few minutes now saves hours of confusion later.

## Where to start

1. Read `pipeline.rs` — it shows the full compilation flow in 50 lines.
2. Read an example in `examples/` to understand the surface syntax.
3. Read `value.rs` to understand runtime representation.
4. Read a failing test to understand what's expected.

When in doubt, run the tests.

5. Read [`docs/cookbook.md`](docs/cookbook.md) for step-by-step recipes for common cross-cutting changes.
6. Read [`tests/AGENTS.md`](tests/AGENTS.md) for test organization and how to add new tests.

## Standard Library Functions

### subprocess/system

**Location:** `stdlib.lisp`

**Signature:** `(subprocess/system program args [opts])`

**Purpose:** Run a command to completion, capturing stdout and stderr as text. Returns `{:exit int :stdout string :stderr string}`.

**Behavior:**
- Spawns a subprocess with the given program and arguments
- Captures stdout and stderr as binary pipes, then decodes to UTF-8 strings
- Waits for subprocess exit and returns the exit code
- Reads pipes before waiting to avoid deadlock when output exceeds OS pipe buffer (~64KB)
- Optional third argument: opts struct with keys `:env` (struct of env vars), `:cwd` (working directory string), `:stdin` (default `:null`)

**Examples:**
```lisp
(subprocess/system "echo" ["hello"])
#=> {:exit 0 :stdout "hello\n" :stderr ""}

(subprocess/system "false" [])
#=> {:exit 1 :stdout "" :stderr ""}

(subprocess/system "ls" ["-la"] {:cwd "/tmp"})
#=> {:exit 0 :stdout "..." :stderr ""}
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Program not found | `exec-error` | `"subprocess/exec: {program}: {error}"` |
| Invalid UTF-8 in output | `encoding-error` | `"invalid UTF-8 at byte {offset}"` |
| I/O error | `exec-error` | `"subprocess/wait: {error}"` |

**Invariants:**

1. **Deadlock prevention.** Pipes are read before `subprocess/wait` to ensure neither side blocks on buffer overflow.
2. **Text decoding.** Output is decoded as strict UTF-8; invalid UTF-8 propagates an error.
3. **Exit code preservation.** The returned `:exit` code matches the subprocess exit status (0 = success, nonzero = failure).
4. **Subprocess cleanup.** The process is reaped on exit; no zombies are left behind.

### merge

**Location:** `stdlib.lisp`

**Signature:** `(merge a b)`

**Purpose:** Merges struct `b` into struct `a`, returning the same type as `a`. Keys in `b` override keys in `a`.

**Behavior:**
- If `a` is an immutable struct `{...}`, returns an immutable struct
- If `a` is a mutable struct `@{...}`, returns a mutable struct
- All keys from `a` are preserved
- All keys from `b` are added or override existing keys in `a`
- The type of the result matches the type of `a`

**Examples:**
```lisp
(merge {:x 1 :y 2} {:y 3 :z 4})
#=> {:x 1 :y 3 :z 4}

(merge @{:x 1 :y 2} {:y 3 :z 4})
#=> @{:x 1 :y 3 :z 4}

(merge {:a 1} {})
#=> {:a 1}

(merge {} {:b 2})
#=> {:b 2}
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| `a` is not a struct | `type-error` | `"merge: first argument must be struct, got {type}"` |
| `b` is not a struct | `type-error` | `"merge: second argument must be struct, got {type}"` |
| Wrong arity | `arity-error` | `"merge: expected 2 arguments, got N"` |

**Invariants:**

1. **Type preservation.** The result type matches the type of the first argument.
2. **Non-destructive.** Neither `a` nor `b` is modified; a new struct is returned.
3. **Override semantics.** Keys in `b` take precedence over keys in `a`.
4. **Immutability respected.** If `a` is immutable, the result is immutable. If `a` is mutable, the result is mutable.
