# Elle

Elle is a Lisp. Source text becomes bytecode; bytecode runs on a register-based VM.

This is not a toy. The implementation targets correctness, performance, and
clarity — in that order. We compile through multiple IRs, we have proper
lexical scoping with closure capture analysis, and we have an effect system.

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
| `hir` | Binding resolution, capture analysis, effect inference, linting, symbol extraction, docstring extraction |
| `lir` | SSA form with virtual registers, basic blocks, `SpannedInstr` for source tracking |
| `compiler` | Bytecode instruction definitions, debug formatting |
| `vm` | Bytecode execution, builtin documentation storage |
| `value` | Runtime value representation (NaN-boxed) |
| `effects` | Effect type (`Inert`, `Yields`, `Polymorphic`) |
| `io` | I/O request types, backends, timeout handling |
| `lint` | Diagnostic types and lint rules |
| `symbols` | Symbol index types for IDE features |
| `primitives` | Built-in functions |
| `ffi` | C interop via libloading/bindgen |
| `jit` | JIT compilation via Cranelift |
| `formatter` | Code formatting for Elle source |
| `plugin` | Dynamic plugin loading for Rust cdylib primitives |
| `path` | UTF-8 path operations |
| `pipeline` | Compilation entry points (see [`src/pipeline/AGENTS.md`](src/pipeline/AGENTS.md)) |
| `error` | `LocationMap` for bytecode offset → source location mapping |

### The Value type

`Value` is the runtime representation using NaN-boxing. Create values via methods like `Value::int()`, `Value::cons()`, `Value::closure()` rather than enum variants. Notable types:
- `Closure` — bytecode + captured environment + arity + effect + `location_map` + `doc` + `syntax`
- `Cell` / `LocalCell` — mutable cells for captured variables
- `Fiber` — independent execution context with stack, frames, signal mask
- `Parameter` — dynamic binding with default value, looked up at runtime
- `External` — opaque plugin-provided Rust object (`Rc<dyn Any>` with type name)

All heap-allocated values use `Rc`. Mutable values use `RefCell`.

## Products

| Product | Path | Purpose |
|---------|------|---------|
| elle | `src/` | Interpreter/compiler (includes `lint`, `lsp`, and `rewrite` subcommands) |
| docgen | `demos/docgen/` | Documentation site generator (written in Elle) |

## Directories

| Path | Contains |
|------|----------|
| `src/` | Core interpreter/compiler |
| `src/io/` | I/O request types and backends |
| `src/lsp/` | Language server protocol implementation |
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
   use `LocalCell` for indirection. The `cell_params_mask` on `Closure` tracks
   which parameters need cell wrapping.

3. **Effects are inferred, not declared.** The `Effect` enum (`Inert`, `Yields`,
   `Polymorphic`) propagates from leaves to root during analysis.

4. **The VM is stack-based for operands, register-addressed for locals.**
   Instructions reference registers (locals) by index. Results push to the
   operand stack.

5. **Errors propagate.** Functions return `LResult<T>`. Silent failure is
   forbidden. If you catch an error, you must either handle it meaningfully or
   propagate it.

## Intentional oddities

Things that look wrong but aren't. The 4 most critical (agents get these wrong):

- **`nil` vs `()` are distinct.** `nil` is falsy (absence). `()` is truthy (empty list).
  Lists terminate with `EMPTY_LIST`. Use `empty?` (not `nil?`) for end-of-list.
  **Getting this wrong causes infinite recursion.**

- **`#` is comment, `;` is splice.** `#` starts a comment. `;expr` is the splice operator
  (array-spreading). `true`/`false` are booleans (not `#t`/`#f`).

- **`assign` not `set` for mutation.** `(assign var value)` mutates. `(set x val)` creates
  a set value. Agents reflexively write `(set x val)` — this is wrong.

- **Collection literals: bare = immutable, `@` = mutable.** `[...]` → array, `@[...]` → @array.
  `{...}` → struct, `@{...}` → @struct. `|...|` → set, `@|...|` → @set.
  `"..."` → string, `@"..."` → @string.

For the full list of oddities (17 items): [`docs/oddities.md`](docs/oddities.md).

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
