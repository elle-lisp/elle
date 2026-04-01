# Elle

Elle is a Lisp. Source text becomes bytecode; bytecode runs on a VM.

This is not a toy. The implementation targets correctness, performance, and
clarity — in that order. We compile through multiple IRs, we have proper
lexical scoping with closure capture analysis, and we have a signal system.

You are an LLM. You will make mistakes. The test suite will catch them. Run the
tests. Read the error messages. They are designed to be helpful.

## Before you touch any code

You will write Elle code — in tests, repros, and debugging. You will
misuse the language if you don't know it.

1. Read [`QUICKSTART.md`](QUICKSTART.md). It is the complete language
   reference. You need it.
2. Read [`docs/stdlib.md`](docs/stdlib.md) for the standard library.
3. The `docs/` directory contains literate Elle programs. Run them
   with `elle docs/foo.md` to see working examples.
4. Use `(doc name)` in the REPL to look up any primitive.
   Use `elle --help` to see all primitives grouped by category.
5. Read [`docs/cookbook.md`](docs/cookbook.md) for step-by-step recipes
   for common cross-cutting changes.
6. Read [`tests/AGENTS.md`](tests/AGENTS.md) for test organization.

## Contents

- [Before you touch any code](#before-you-touch-any-code)
- [Architecture](#architecture)
- [Products](#products)
- [Directories](#directories)
- [Testing](#testing)
- [Invariants](#invariants)
- [Intentional oddities](#intentional-oddities)
- [Conventions](#conventions)
- [Maintaining documentation](#maintaining-documentation)

## Architecture

```
Source → Reader → Syntax → Expander → Syntax → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM
```

This is the only compilation pipeline. Source locations flow through the entire
pipeline: Syntax spans → HIR spans → LIR `SpannedInstr` → `LocationMap` in
bytecode. Error messages include file:line:col information.

### Key modules

- **`reader`** — Lexing and parsing to `Syntax`
- **`syntax`** — Syntax types, macro expansion
- **`hir`** — Binding resolution, capture analysis, signal inference, linting,
  symbol extraction, docstring extraction
- **`lir`** — SSA form with virtual registers, basic blocks, `SpannedInstr`
  for source tracking
- **`compiler`** — Bytecode instruction definitions, debug formatting
- **`vm`** — Bytecode execution, builtin documentation storage
- **`value`** — Runtime value representation (tagged-union); trait table field on
  19 user-facing heap variants
- **`signals`** — Signal type (`Silent`, `Yields`, `Polymorphic`), signal
  registry for keyword-to-bit mapping;
  includes `SIG_EXEC` (bit 11) for subprocess operations and `SIG_FUEL`
  (bit 12) for instruction budget exhaustion
- **`io`** — I/O request types, backends, timeout handling;
  includes `PortKind::Pipe` for subprocess stdio and `ProcessHandle`
  for subprocess lifecycle
- **`lint`** — Diagnostic types and lint rules
- **`symbols`** — Symbol index types for IDE features
- **`primitives`** — Built-in functions. Run `(help)` in the REPL for a
  full list grouped by category. See [`docs/stdlib.md`](docs/stdlib.md).
- **`stdlib`** — Standard library functions (loaded at startup). See
  [`docs/stdlib.md`](docs/stdlib.md).
- **`ffi`** — C interop via libloading/bindgen
- **`jit`** — JIT compilation via Cranelift; compiles silent and yielding
  functions (rejects polymorphic); `JitRejectionInfo` tracks rejections
- **`wasm`** — WASM backend via Wasmtime; full-module compilation
  (`--wasm=full`) or per-closure tiered compilation (`--wasm=N`).
  See [`docs/impl/wasm.md`](docs/impl/wasm.md).
- **`formatter`** — Code formatting for Elle source
- **`plugin`** — Dynamic plugin loading for Rust cdylib primitives.
  See [`docs/plugins.md`](docs/plugins.md) for the full list.
- **`path`** — UTF-8 path operations
- **`pipeline`** — Compilation entry points
  (see [`src/pipeline/AGENTS.md`](src/pipeline/AGENTS.md))
- **`error`** — `LocationMap` for bytecode offset → source location mapping

### The Value type

`Value` is a 16-byte tagged union `(tag: u64, payload: u64)`. See
[`docs/impl/values.md`](docs/impl/values.md) for details. Key points:
- Create via `Value::int()`, `Value::cons()`, etc. — not enum variants
- Heap values use `Rc`; mutable values use `RefCell`
- 19 user-facing heap variants carry a `traits: Value` field
- 5 infrastructure types (`Float`, `NativeFn`, `LibHandle`, `FFISignature`,
  `FFIType`) do not carry traits

## Products

| Product | Path | Purpose |
|---------|------|---------|
| elle | `src/` | Interpreter/compiler (includes `lint`, `lsp`, and `rewrite` subcommands) |
| docgen | `demos/docgen/` | Documentation site generator (written in Elle) |
| lib/http.lisp | `lib/` | Pure Elle HTTP/1.1 client and server |
| lib/aws.lisp | `lib/` | Elle-native AWS client (SigV4, HTTPS) |

## Directories

| Path | Contains |
|------|----------|
| `src/` | Core interpreter/compiler |
| `src/io/` | I/O request types and backends |
| `src/lsp/` | Language server protocol implementation |
| `lib/` | Reusable Elle modules (HTTP, TLS, Redis, DNS, AWS, etc.) |
| `examples/` | Executable semantics documentation |
| `tests/` | Unit, integration, property tests |
| `benches/` | Criterion and IAI benchmarks |
| `docs/` | Design documents and guides |
| `demos/` | Comparison implementations |
| `plugins/` | Dynamically-loaded plugin crates (cdylib) |
| `tools/` | MCP server, graph extractor, codemod scripts |
| `site/` | Generated documentation site |

## Testing

**⚠️ NEVER run `cargo test --workspace` without explicit user instruction.**
It takes ~30 minutes. Use `make test` (~2min) for pre-commit verification.

| Command | Runtime | What it does |
|---------|---------|-------------|
| `make smoke` | ~15s | Elle examples only |
| `make test` | ~2min | build + examples + elle scripts + unit tests |
| `cargo test --workspace` | ~30min | full suite — **ask first** |

For test organization, helpers, and how to add tests:
[`docs/testing.md`](docs/testing.md).
For CI structure and failure triage: [`docs/debugging.md`](docs/debugging.md).

## Invariants

These must remain true. Violating them breaks the system:

1. **Bindings are resolved at analysis time.** HIR contains `Binding`
   (a `u32` index into a `BindingArena`), not symbols. Binding metadata lives
   in the arena owned by the compilation pipeline. If you see symbol lookup at
   runtime, something is wrong.

2. **Closures capture by value into their environment.** Immutable captured
   locals are captured directly. Mutable captured locals and mutated parameters
   use `LocalLBox` for indirection. The `lbox_params_mask` on `Closure` tracks
   which parameters need lbox wrapping.

3. **Signals are inferred, not declared — except when `silence` provides
   explicit bounds.** The `Signal` type (`Silent`, `Yields`, `Polymorphic`)
   propagates from leaves to root during analysis. `silence` constrains
   inference; it doesn't replace it. The inferred signal must be a subset of
   the declared bound. When a parameter has a `silence` bound, it is no longer
   polymorphic — its signal is known to be zero bits.

4. **The VM is stack-based for operands, register-addressed for locals.**
   Instructions reference registers (locals) by index. Results push to the
   operand stack.

5. **Errors propagate.** Functions return `LResult<T>`. Silent failure is
   forbidden. If you catch an error, you must either handle it meaningfully or
   propagate it.

## Intentional oddities

The 4 most critical (agents get these wrong):

- **`nil` vs `()` are distinct.** `nil` is falsy; `()` is truthy (empty
  list). Use `empty?` not `nil?` for end-of-list. **Getting this wrong
  causes infinite recursion.**
- **`#` is comment, `;` is splice.** Not the other way around.
- **`assign` not `set` for mutation.** `(set x val)` creates a set.
- **`squelch` takes exactly 2 arguments.** `(squelch closure :keyword)` or
  `(squelch closure |:kw1 :kw2|)` with a set.

Full list: [`docs/warts.md`](docs/warts.md).

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

AGENTS.md and README.md files exist throughout the codebase. Keep them
current:

- **When you change a module's interface**, update its AGENTS.md. Changed
  exports, new invariants, altered data flow — these matter to the next agent.

- **When you add a new module**, create AGENTS.md (for agents) and README.md
  (for humans). Copy structure from a sibling module.

- **When you violate a documented invariant**, either fix your code or update
  the invariant. Stale invariants are worse than none.

- **When you discover undocumented behavior**, document it. If it's
  intentional, add to `docs/warts.md`. If it's a bug, file an issue.

Documentation debt compounds. A few minutes now saves hours of confusion
later.

## Implementation entry points

1. Read `pipeline.rs` — it shows the full compilation flow in 50 lines.
2. Read an example in `examples/` to understand the surface syntax.
3. Read `value.rs` to understand runtime representation.
4. Read a failing test to understand what's expected.

When in doubt, run the tests.

## MCP Server

`tools/mcp-server.lisp` is an MCP (Model Context Protocol) server that
exposes an oxigraph RDF store over SPARQL via JSON-RPC 2.0 on stdio.

### Tools exposed

| Tool | Purpose |
|------|---------|
| `sparql_query` | Execute SPARQL SELECT / ASK / CONSTRUCT |
| `sparql_update` | Execute SPARQL UPDATE (INSERT DATA, DELETE, etc.) |
| `load_rdf` | Load RDF data from a string (turtle/ntriples/nquads/rdfxml) |
| `dump_rdf` | Serialize the store to a string |

### Store location

Resolution order:
1. CLI arg: `elle tools/mcp-server.lisp -- /path/to/store`
2. Env var: `ELLE_MCP_STORE=/path/to/store`
3. Default: `.elle-mcp/store/` in CWD (auto-created)

The store is always persistent (no in-memory fallback). `.elle-mcp/` is
gitignored.

### Related tools

| File | Purpose |
|------|---------|
| `tools/elle-graph.lisp` | Extract RDF triples from Elle source files via `read-all` |
| `tools/rust-graph.lisp` | Extract RDF triples from Rust source files via syn plugin |
| `tools/load-all.lisp` | Extract Elle + Rust graphs and load into oxigraph store |
| `tools/demo-queries.lisp` | Example SPARQL queries against the Elle knowledge graph |
| `tools/test-mcp.lisp` | Smoke test: spawns server, exercises all tools |
| `tools/test-oxigraph-load.lisp` | Verifies oxigraph plugin loads |
| `tools/bug-repro.lisp` | VM panic repro: glob + nested let+protect + push |

### Graph schema

`elle-graph.lisp` emits ntriples with `urn:elle:` namespace:

| Type | Predicates |
|------|-----------|
| `elle:Fn` | `elle:name`, `elle:file`, `elle:arity`, `elle:param`, `elle:doc` |
| `elle:Def` | `elle:name`, `elle:file` |
| `elle:Macro` | `elle:name`, `elle:file` |
| `elle:Import` | `elle:name`, `elle:path`, `elle:file` |

`rust-graph.lisp` and the `extract_rust` MCP tool emit ntriples with
`urn:rust:` namespace:

| Type | Predicates |
|------|-----------|
| `rust:Fn` | `rust:name`, `rust:file`, `rust:param`, `rust:param-type`, `rust:return-type`, `rust:async`, `rust:unsafe`, `rust:visibility`, `rust:attribute` |
| `rust:Struct` | `rust:name`, `rust:file`, `rust:kind`, `rust:field`, `rust:field-type`, `rust:visibility`, `rust:attribute` |
| `rust:Enum` | `rust:name`, `rust:file`, `rust:variant`, `rust:visibility`, `rust:attribute` |
| `rust:Trait` | `rust:name`, `rust:file`, `rust:visibility`, `rust:attribute` |
| `rust:Const` | `rust:name`, `rust:file`, `rust:visibility`, `rust:attribute` |
| `rust:Static` | `rust:name`, `rust:file`, `rust:visibility`, `rust:attribute` |
| `rust:Type` | `rust:name`, `rust:file`, `rust:visibility`, `rust:attribute` |
| `rust:Mod` | `rust:name`, `rust:file`, `rust:visibility`, `rust:attribute` |

## Standard Library

See [`docs/stdlib.md`](docs/stdlib.md) for the full standard library
reference. Use `(doc name)` in the REPL for any function's documentation.
