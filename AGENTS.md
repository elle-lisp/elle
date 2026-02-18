# Elle

Elle is a Lisp. Source text becomes bytecode; bytecode runs on a register-based VM.

This is not a toy. The implementation targets correctness, performance, and
clarity - in that order. We compile through multiple IRs, we JIT-compile hot
paths via Cranelift, we have proper lexical scoping with closure capture
analysis, and we have an effect system.

You are an LLM. You will make mistakes. The test suite will catch them. Run the
tests. Read the error messages. They are designed to be helpful.

## Architecture

```
Source → Reader → Syntax → Expander → Syntax → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM
```

The new pipeline (Syntax → HIR → LIR → Bytecode) is the primary pipeline.
elle-lint and elle-lsp use it exclusively. The old Value-based pipeline
remains only for CPS and Cranelift JIT, which still depend on the `Expr` type.

### Key modules

| Module | Responsibility |
|--------|----------------|
| `reader` | Lexing and parsing to `Syntax` |
| `syntax` | Syntax types, macro expansion |
| `hir` | Binding resolution, capture analysis, effect inference, linting, symbol extraction |
| `lir` | SSA form with virtual registers, basic blocks |
| `compiler` | Bytecode emission, JIT coordination, CPS (legacy AST still used by JIT/CPS) |
| `vm` | Bytecode execution |
| `value` | Runtime value representation (NaN-boxed) |
| `effects` | Effect type (`Pure`, `Yields`, `Polymorphic`) |
| `lint` | Diagnostic types and lint rules (pipeline-agnostic) |
| `symbols` | Symbol index types for IDE features (pipeline-agnostic) |
| `primitives` | Built-in functions |
| `ffi` | C interop via libloading/bindgen |
| `pipeline` | Compilation entry points (`compile_new`, `analyze_new`, `eval_new`) |

### The Value type

`Value` is the runtime representation. It uses NaN-boxing for efficient
representation. Create values via methods like `Value::int()`, `Value::cons()`,
`Value::closure()` rather than enum variants. Notable types:
- `Closure` - bytecode + captured environment + arity + effect
- `JitClosure` - native code pointer + environment
- `Cell` / `LocalCell` - mutable cells for captured variables
- `Coroutine` - suspendable computation with saved context

All heap-allocated values use `Rc`. Mutable values use `RefCell`. The
`SendValue` wrapper exists for thread-safety when needed.

## Products

| Product | Path | Purpose |
|---------|------|---------|
| elle | `src/` | Interpreter/compiler |
| elle-lsp | `elle-lsp/` | Language server |
| elle-lint | `elle-lint/` | Static analysis |
| elle-doc | `elle-doc/` | Documentation site generator (written in Elle) |

## Directories

| Path | Contains |
|------|----------|
| `src/` | Core interpreter/compiler |
| `examples/` | Executable semantics documentation |
| `tests/` | Unit, integration, property tests |
| `benches/` | Criterion and IAI benchmarks |
| `docs/` | Design documents and guides |
| `demos/` | Comparison implementations |
| `site/` | Generated documentation site |

## Verification

```bash
# Full test suite (do this before committing)
cargo test --workspace

# Just the main crate
cargo test

# Specific test
cargo test test_name

# Run all examples (they are tests)
cargo test --test '*'

# Check formatting
cargo fmt -- --check

# Lint (warnings will turn into errors in the CI and fail the build)
cargo clippy --workspace --all-targets -- -D warnings

# Run a single example
cargo run -- examples/closures.lisp

# Generate documentation site (this runs Elle code — catches runtime bugs)
cargo build --release && ./target/release/elle elle-doc/generate.lisp

# Rust API docs with warnings as errors
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

CI runs: tests (stable/beta/nightly), fmt, clippy, examples, coverage,
benchmarks, rustdoc, elle-doc site generation. All must pass.

## Invariants

These must remain true. Violating them breaks the system:

1. **Bindings are resolved at analysis time.** HIR contains `BindingId`, not
   symbols. If you see symbol lookup at runtime, something is wrong.

2. **Closures capture by value into their environment.** Mutable captures use
   `LocalCell`. The `cell_params_mask` on `Closure` tracks which parameters need
   cell wrapping.

3. **Effects are inferred, not declared.** The `Effect` enum (`Pure`, `Yields`,
   `Polymorphic`) propagates from leaves to root during analysis.

4. **The VM is stack-based for operands, register-addressed for locals.**
   Instructions reference registers (locals) by index. Results push to the
   operand stack.

5. **Errors propagate.** Functions return `LResult<T>`. Silent failure is
   forbidden. If you catch an error, you must either handle it meaningfully or
   re-raise it.

## Intentional oddities

Things that look wrong but aren't:

- Two cell types exist: `Cell` (user-created via `box`, explicit) and
  `LocalCell` (compiler-created for mutable captures, auto-unwrapped).
- `VmAwareFn` exists because some primitives (like `coroutine-resume`) need
  to execute bytecode, so they need VM access.
- The `Cons` type in `value/heap.rs` is the heap-allocated cons cell data.
  `Value::cons(car, cdr)` creates a NaN-boxed pointer to a heap Cons.
- `nil` and empty list `()` are distinct values with different truthiness:
  - `Value::NIL` is falsy (represents absence)
  - `Value::EMPTY_LIST` is truthy (it's a list, just empty)
- Lists are `EMPTY_LIST`-terminated, not `NIL`-terminated. `(rest (list 1))`
  returns `EMPTY_LIST`. Use `empty?` (not `nil?`) to check for end-of-list.
  `nil?` only matches `Value::NIL`. This distinction matters in recursive
  list functions and affects `elle-doc/` and `examples/`.

## Conventions

- Files and directories: lowercase, single-word when possible.
- Target file size: 300 lines / 5-10KB. Refactor when exceeded.
- Prefer formal types over hashes/maps for structured data.
- Validation at boundaries, not recovery at use sites.
- Tests reflect architecture: unit tests for modules, integration tests for
  pipelines, property tests for invariants.
- Examples in `examples/` serve as both documentation and executable tests.

## Maintaining documentation

AGENTS.md and README.md files exist throughout the codebase. Keep them current:

- **When you change a module's interface**, update its AGENTS.md. Changed
  exports, new invariants, altered data flow - these matter to the next agent.

- **When you add a new module**, create AGENTS.md (for agents) and README.md
  (for humans). Copy structure from a sibling module.

- **When you violate a documented invariant**, either fix your code or update
  the invariant. Stale invariants are worse than none.

- **When you discover undocumented behavior**, document it. If it's intentional,
  add to "Intentional oddities." If it's a bug, file an issue.

Documentation debt compounds. A few minutes now saves hours of confusion later.

## elle-doc: the documentation site generator

`elle-doc/generate.lisp` is an Elle program that generates the documentation
site. CI builds it with `./target/release/elle elle-doc/generate.lisp` as part
of the docs job. Because it's written in Elle, it exercises the runtime — any
change to the language semantics (value representation, list operations,
string handling) can break it.

When the docs CI job fails, check `elle-doc/generate.lisp` and its library
files in `elle-doc/lib/`. Common failure: using `nil?` instead of `empty?`
for list termination.

## What not to do

- Do not add backward compatibility machinery. Breaking changes are fine;
  we'll write a migration tool.
- Do not optimize prematurely. Correctness first. Profile before optimizing.
- Do not add features "for the future." Build what's needed now.
- Do not silently swallow errors. Propagate or log with context.
- Do not bypass the type system with excessive use of `Any` or downcasting.

## Where to start

1. Read `pipeline.rs` - it shows the full compilation flow in 50 lines.
2. Read an example in `examples/` to understand the surface syntax.
3. Read `value.rs` to understand runtime representation.
4. Read a failing test to understand what's expected.

When in doubt, run the tests.
