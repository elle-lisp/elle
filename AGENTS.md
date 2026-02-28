# Elle

Elle is a Lisp. Source text becomes bytecode; bytecode runs on a register-based VM.

This is not a toy. The implementation targets correctness, performance, and
clarity - in that order. We compile through multiple IRs, we have proper
lexical scoping with closure capture analysis, and we have an effect system.

You are an LLM. You will make mistakes. The test suite will catch them. Run the
tests. Read the error messages. They are designed to be helpful.

## Architecture

```
Source → Reader → Syntax → Expander → Syntax → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM
```

This is the only compilation pipeline. Source locations flow through the entire
pipeline: Syntax spans → HIR spans → LIR `SpannedInstr` → `LocationMap` in
bytecode. Error messages include file:line:col information.

### Data flow across boundaries

| Boundary | Output type | Key fields |
|----------|-------------|------------|
| Reader → Expander | `Syntax` | `kind: SyntaxKind`, `span: Span`, `scopes: Vec<ScopeId>`, `scope_exempt: bool` |
| Expander → Analyzer | `Syntax` (expanded) | Same shape; macros resolved, scopes stamped |
| Analyzer → Lowerer | `Hir` (via `AnalysisResult`) | `kind: HirKind`, `span: Span`, `effect: Effect` |
| Lowerer → Emitter | `LirFunction` | `blocks: Vec<BasicBlock>`, `constants: Vec<LirConst>`, `arity: Arity`, `effect: Effect`, `num_locals: u16`, `num_captures: u16`, `cell_params_mask: u64`, `cell_locals_mask: u64`, `entry: Label`, `num_regs: u32`, `name: Option<String>` |
| Emitter → VM | `Bytecode` | `instructions: Vec<u8>`, `constants: Vec<Value>`, `location_map: LocationMap`, `symbol_names: HashMap<u32, String>`, `inline_caches: HashMap<usize, CacheEntry>` |
| VM → caller | `Value` | NaN-boxed 8-byte runtime value |

**What is preserved across the full pipeline:**

| Data | Syntax | HIR | LIR | Bytecode | Runtime |
|------|--------|-----|-----|----------|---------|
| Source spans | `Span` on each node | `Span` on each `Hir` | `SpannedInstr` / `SpannedTerminator` | `LocationMap` (`HashMap<usize, SourceLoc>`) | `Closure.location_map` |
| Effects | — | `Effect` on each `Hir` | `LirFunction.effect` | — | `Closure.effect` |
| Arity | — | `Lambda.params.len()` | `LirFunction.arity` | — | `Closure.arity` |
| Cell mask (params) | — | `Binding.needs_cell()` | `LirFunction.cell_params_mask` | — | `Closure.cell_params_mask` |
| Cell mask (locals) | — | `Binding.needs_cell()` | `LirFunction.cell_locals_mask` | — | JIT only (not on `Closure`) |

**What is transformed at each boundary:**

| Boundary | Transformation |
|----------|----------------|
| Syntax → HIR | Symbol names (`String`) → `Binding(Value)` (NaN-boxed heap pointer to `BindingInner`). Scope sets used for resolution, then no longer needed. |
| HIR → LIR | `Binding` → `u16` slot index (via `binding_to_slot: HashMap<Binding, u16>`). `HirKind` control flow → `BasicBlock` + `Terminator` (explicit jumps). Captures → `LoadCapture`/`LoadCaptureRaw` instructions. |
| LIR → Bytecode | `Reg` (virtual registers) → stack positions (emitter simulates stack). `Label` → byte offsets (jump patching). `LirConst` → `Value` in constant pool. `LirFunction` (nested closures) → `Value::closure()` in constant pool. |
| Bytecode → Runtime | `Bytecode` struct → `Closure` fields: `bytecode: Rc<Vec<u8>>`, `constants: Rc<Vec<Value>>`, `location_map: Rc<LocationMap>`. Globals addressed by `SymbolId` index into `VM.globals: Vec<Value>`. |

**What is discarded:**

| Boundary | Discarded |
|----------|-----------|
| Syntax → HIR | Variable names (replaced by `Binding` identity), scope sets (`Vec<ScopeId>`), macro definitions |
| HIR → LIR | `Binding` objects (replaced by slot indices), `HirKind` structure (replaced by flat instructions) |
| LIR → Bytecode | Virtual register names (`Reg`), block labels (`Label`), `LirConst` enum (replaced by `Value`) |
| Bytecode → Runtime | `inline_caches` (not carried on `Closure`). Note: `LirFunction` survives into `Closure.lir_function` for deferred JIT compilation |

### Key modules

| Module | Responsibility |
|--------|----------------|
| `reader` | Lexing and parsing to `Syntax` |
| `syntax` | Syntax types, macro expansion |
| `hir` | Binding resolution, capture analysis, effect inference, linting, symbol extraction |
| `lir` | SSA form with virtual registers, basic blocks, `SpannedInstr` for source tracking |
| `compiler` | Bytecode instruction definitions (`bytecode.rs`), debug formatting (`bytecode_debug.rs`) |
| `vm` | Bytecode execution |
| `value` | Runtime value representation (NaN-boxed) |
| `effects` | Effect type (`Pure`, `Yields`, `Polymorphic`) |
| `lint` | Diagnostic types and lint rules (pipeline-agnostic) |
| `symbols` | Symbol index types for IDE features (pipeline-agnostic) |
| `primitives` | Built-in functions |
| `ffi` | C interop via libloading/bindgen |
| `jit` | JIT compilation via Cranelift for non-suspending functions |
| `formatter` | Code formatting for Elle source |
| `plugin` | Dynamic plugin loading for Rust cdylib primitives |
| `pipeline` | Compilation entry points (`compile`, `analyze`, `eval`). See [`docs/pipeline.md`](docs/pipeline.md) |
| `error` | `LocationMap` for bytecode offset → source location mapping |

### The Value type

`Value` is the runtime representation. It uses NaN-boxing for efficient
representation. Create values via methods like `Value::int()`, `Value::cons()`,
`Value::closure()` rather than enum variants. Notable types:
- `Closure` - bytecode + captured environment + arity + effect + `location_map: Rc<LocationMap>`
- `Cell` / `LocalCell` - mutable cells for captured variables
- `Fiber` - independent execution context with stack, frames, and signal mask
- `External` - opaque plugin-provided Rust object (`Rc<dyn Any>` with type name)

All heap-allocated values use `Rc`. Mutable values use `RefCell`. The
`SendValue` wrapper exists for thread-safety when needed.

## Products

| Product | Path | Purpose |
|---------|------|---------|
| elle | `src/` | Interpreter/compiler (includes `--lint` and `--lsp` modes) |
| elle-doc | `elle-doc/` | Documentation site generator (written in Elle) |

## Directories

| Path | Contains |
|------|----------|
| `src/` | Core interpreter/compiler |
| `src/lsp/` | Language server protocol implementation |
| `examples/` | Executable semantics documentation |
| `tests/` | Unit, integration, property tests |
| `benches/` | Criterion and IAI benchmarks |
| `docs/` | Design documents and guides |
| `demos/` | Comparison implementations |
| `plugins/` | Dynamically-loaded plugin crates (cdylib) |
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

CI runs on PRs: tests (stable/beta/nightly), fmt, clippy, examples,
benchmarks (with regression reporting), rustdoc, elle-doc site generation.
All must pass. Main-push runs coverage, benchmark publishing, docs generation,
and Pages deployment.

## Invariants

These must remain true. Violating them breaks the system:

1. **Bindings are resolved at analysis time.** HIR contains `Binding` (NaN-boxed
   Value pointing to heap `BindingInner`), not symbols. If you see symbol
   lookup at runtime, something is wrong.

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
- Coroutine primitives (`coro/resume`) are implemented as fiber wrappers.
  They return `(SIG_RESUME, fiber_value)` and the VM's SIG_RESUME handler in
  `vm/call.rs` performs the actual fiber execution. This avoids primitives
  needing VM access.
- The `Cons` type in `value/heap.rs` is the heap-allocated cons cell data.
  `Value::cons(car, cdr)` creates a NaN-boxed pointer to a heap Cons.
- `nil` and empty list `()` are distinct values with different truthiness:
  - `Value::NIL` is falsy (represents absence)
  - `Value::EMPTY_LIST` is truthy (it's a list, just empty)
- Lists are `EMPTY_LIST`-terminated, not `NIL`-terminated. `(rest (list 1))`
  returns `EMPTY_LIST`. Use `empty?` (not `nil?`) to check for end-of-list.
  `nil?` only matches `Value::NIL`. This distinction matters in recursive
  list functions and affects `elle-doc/` and `examples/`.
- Signal bits are partitioned: Bits 0-2 are user-facing (error, yield, debug),
  Bits 3-8 are VM-internal (resume, FFI, propagate, cancel, query, halt),
  Bits 9-15 are reserved, and Bits 16-31 are for user-defined signal types.
  See `src/value/fiber.rs` lines 138-165 for the constants and partitioning
  comment.
- Destructuring uses **silent nil semantics**: missing values become `nil`,
  wrong types produce `nil`, no runtime errors. This is separate from `match`
  pattern matching which is conditional. `CarOrNil`/`CdrOrNil`/`ArrayRefOrNil`/
  `ArraySliceFrom`/`TableGetOrNil` are dedicated bytecode instructions for
  this — they never signal errors. `ArrayRefOrNil` and `ArraySliceFrom` handle
  both arrays and tuples — bracket destructuring works on any indexed sequential
  type. In `match`, however, `[a b]` patterns only match arrays (the `IsArray`
  guard rejects tuples before element extraction). In `match`, compound patterns
  (`Cons`, `List`, `Array`, `Table`) emit type guards (`IsPair`, `IsArray`,
  `IsTable`) that branch to the fail label before extracting elements.
- `defn`, `let*`, `->`, `->>`, `when`, `unless`, `try`/`catch`, `protect`,
  `defer`, `with`, and `yield*` are prelude macros defined in
  [`prelude.lisp`](prelude.lisp) (project root), loaded by the Expander
  before user code expansion. The prelude is embedded via `include_str!`
  (in `src/syntax/expand/mod.rs`) and parsed/expanded on each Expander
  creation.
- Collection literals follow the mutable/immutable split (see `docs/types.md`):
  bare delimiters are immutable, `@`-prefixed are mutable. `{:key val ...}` →
  struct (immutable). `@{:key val}` → table (mutable). `[1 2 3]` → tuple
  (immutable). `@[1 2 3]` → array (mutable). `"hello"` → string (immutable).
  `@"hello"` → buffer (mutable). `SyntaxKind::Tuple` represents `[...]`,
  `SyntaxKind::Array` represents `@[...]`, `SyntaxKind::Struct` represents
  `{...}`, `SyntaxKind::Table` represents `@{...}`. The reader produces all
  four directly (no desugaring to List with prepended symbols). `@"..."` desugars
  to `(string->buffer "...")`. In `match`, `[...]` matches tuples (`IsTuple`),
  `@[...]` matches arrays (`IsArray`), `{...}` matches structs (`IsStruct`),
  `@{...}` matches tables (`IsTable`). In destructuring (`def`/`let`/`fn`),
  no type guards — `ArrayRefOrNil`/`TableGetOrNil` handle both mutable and
  immutable types.
- `;expr` is the splice reader macro (Janet-style). It marks a value for
  array-spreading at call sites and data constructors. `(splice expr)` is the
  long form. `;` is a delimiter, so `a;b` is three tokens. `,;` is
  unquote-splicing (inside quasiquote), not comma + splice. Splice only works
  on arrays and tuples (indexed types). Structs and tables reject splice at
  compile time (key-value semantics). When a call has spliced args, the lowerer
  builds an args array (`MakeArray` → `ArrayExtend`/`ArrayPush` → `CallArray`)
  instead of the normal `Call` instruction. Arity checking is disabled for
  spliced calls.
- `#` is the comment character (not `;`). `true`/`false` are the boolean
  literals (not `#t`/`#f`).
- `begin` and `block` are distinct forms. `begin` sequences expressions
  without creating a scope (bindings leak into the enclosing scope). `block`
  sequences expressions within a new lexical scope (bindings are contained).
  `block` supports an optional keyword name and `break` for early exit:
  `(block :name body...)` / `(break :name value)`. `break` is validated at
  compile time — it must be inside a block and cannot cross function boundaries.
- `ExternalObject` uses `Rc<dyn Any>` despite the general preference for typed values.
  This is intentional — plugins are dynamically loaded and the core compiler cannot
  know their types at compile time. The `type_name` field provides Elle-side identity,
  and `downcast_ref` is used only within the plugin that created the type.
- `import` is now an alias for the `import-file` primitive (was previously an
  Elle-level function using `eval`/`read-all`/`slurp`). It returns the last
  expression's value for `.lisp` files, and `true` for `.so` plugins. The
  `import-file` primitive handles both Elle source files and plugin `.so` files.

## Conventions

- Files and directories: lowercase, single-word when possible.
- Target file size: 500 lines / 15KB. Dispatch tables (match-heavy) up to
  800 lines. Primitive collections up to 400 lines. Refactor when exceeded.
- Prefer formal types over hashes/maps for structured data.
- Validation at boundaries, not recovery at use sites.
- Tests reflect architecture: unit tests for modules, integration tests for
  pipelines, property tests for invariants.
- Examples in `examples/` serve as both documentation and executable tests.

## Blast radius

Common extension patterns and every file that must be touched. Missing a
file means a broken build, a silent bug, or an incomplete feature.

### Adding a new heap type

Example: Buffer (mutable byte sequence), added alongside the existing
Array, Table, Tuple, Struct, Closure types.

- [ ] `src/value/heap.rs` — add variant to `HeapObject` enum, add
      discriminant to `HeapTag` enum, add arms to `HeapObject::tag()`,
      `HeapObject::type_name()`, and `Debug for HeapObject`
- [ ] `src/value/repr/constructors.rs` — add `Value::your_type()` constructor
- [ ] `src/value/repr/accessors.rs` — add `is_your_type()` predicate,
      `as_your_type()` extractor, and arm in `Value::type_name()`
- [ ] `src/value/repr/traits.rs` — add arm to `PartialEq for Value`
      (structural vs reference equality)
- [ ] `src/value/display.rs` — add arms to both `Display` and `Debug`
      for `Value`
- [ ] `src/value/send.rs` — add variant to `SendValue` enum, add arms to
      `SendValue::from_value()` and `SendValue::into_value()` (or return
      error if not sendable)
- [ ] `src/value/mod.rs` — re-export new types if needed
- [ ] `src/primitives/type_check.rs` — add `your_type?` predicate function
      and entry in `PRIMITIVES` array
- [ ] `src/primitives/registration.rs` — add your module to `ALL_TABLES`
      if you created a new primitives file
- [ ] `src/primitives/` — create operations module (e.g., `buffer.rs`)
      with `PRIMITIVES` array
- [ ] `src/primitives/list.rs` — update `prim_length` if the type has a
      meaningful length
- [ ] `src/primitives/table.rs` — update `prim_get` / `prim_put` if the
      type supports indexed or keyed access
- [ ] `src/primitives/json/serializer.rs` — add arm to `serialize_value`
      and `serialize_value_pretty` (both functions have exhaustive
      `HeapTag` matches)
- [ ] `src/formatter/core.rs` — add arm to `format_value` (exhaustive
      `HeapObject` match)
- [ ] `src/syntax/convert.rs` — update `Syntax::from_value()` if the type
      can appear in macro results (Value → Syntax conversion)
- [ ] `src/value/AGENTS.md` — document the new type

If the type needs new bytecode instructions (construction, access):
- [ ] `src/compiler/bytecode.rs` — add variant(s) to `Instruction` enum
- [ ] `src/compiler/bytecode_debug.rs` — add arm(s) to `disassemble_lines`
- [ ] `src/lir/types.rs` — add variant(s) to `LirInstr` enum
- [ ] `src/lir/emit.rs` — add arm(s) to `emit_instr`
- [ ] `src/vm/dispatch.rs` — add arm(s) to the dispatch `match`
- [ ] `src/vm/data.rs` (or new handler file) — implement handler function(s)

### Adding a new bytecode instruction

- [ ] `src/compiler/bytecode.rs` — add variant to `Instruction` enum
      (append at end; byte values are positional via `repr(u8)`)
- [ ] `src/compiler/bytecode_debug.rs` — add arm to `disassemble_lines`
      with operand decoding
- [ ] `src/lir/types.rs` — add variant to `LirInstr` enum (and/or
      `Terminator` if it's a control flow instruction)
- [ ] `src/lir/emit.rs` — add arm to `emit_instr` (or `emit_terminator`)
      that emits the `Instruction` byte and operands
- [ ] `src/lir/lower/` — add lowering logic in the appropriate file:
      `expr.rs` (expressions), `control.rs` (control flow, calls),
      `binding.rs` (binding forms), `pattern.rs` (pattern matching),
      `lambda.rs` (closures)
- [ ] `src/vm/dispatch.rs` — add arm to the dispatch `match` in
      `execute_bytecode_inner_impl`, delegating to a handler
- [ ] `src/vm/` — implement handler in the appropriate file:
      `data.rs` (data ops), `arithmetic.rs`, `comparison.rs`,
      `types.rs` (type checks), `stack.rs`, `variables.rs`,
      `control.rs` (jumps), `closure.rs`, `scope/` (scope ops)
- [ ] `src/compiler/AGENTS.md` — document the new instruction

If JIT support is needed, also update:
- [ ] `src/jit/dispatch.rs` — add arm to the JIT dispatch
- [ ] `src/jit/compiler.rs` — add compilation logic
- [ ] `src/jit/translate.rs` — add instruction translation

### Adding a new special form

- [ ] `src/hir/analyze/forms.rs` — add `match` arm in `analyze_expr` for
      the new form name, and implement `analyze_your_form` method
- [ ] `src/hir/expr.rs` — add variant to `HirKind` enum
- [ ] `src/lir/lower/expr.rs` — add arm to `lower_expr` dispatch
- [ ] `src/lir/lower/` — implement `lower_your_form` in the appropriate
      file (`control.rs` for control flow, `binding.rs` for binding forms)
- [ ] `src/hir/tailcall.rs` — add arm to the tail-call marking pass if
      the form has sub-expressions that could be in tail position
- [ ] `src/hir/lint.rs` — add arm to the HIR linter walk if the form
      has sub-expressions to lint
- [ ] `src/hir/symbols.rs` — add arm to symbol extraction if the form
      introduces or references symbols

If the form needs new syntax:
- [ ] `src/syntax/mod.rs` — add variant to `SyntaxKind`, update
      `kind_label()`, `set_scopes_recursive()`
- [ ] `src/syntax/display.rs` — add display arm
- [ ] `src/reader/syntax_parser.rs` — add parsing logic
- [ ] `src/reader/lexer.rs` — add token type if new delimiter needed

If the form needs new bytecode instructions, follow the bytecode
checklist above.

### Adding a new collection literal

This is the most invasive change. It touches every layer of the pipeline.

Reader (source → tokens → syntax):
- [ ] `src/reader/lexer.rs` — add delimiter handling (e.g., `@[` for
      arrays, `@{` for tables)
- [ ] `src/reader/token.rs` — add token variant if needed
- [ ] `src/reader/syntax_parser.rs` — add parsing to `SyntaxKind`
- [ ] `src/reader/parser.rs` — add parsing to `Value` (legacy reader)

Syntax (expansion):
- [ ] `src/syntax/mod.rs` — add `SyntaxKind` variant, update
      `kind_label()`, `set_scopes_recursive()`
- [ ] `src/syntax/display.rs` — add display arm
- [ ] `src/syntax/convert.rs` — add arms to `to_value()` and
      `from_value()` (Syntax ↔ Value conversion)
- [ ] `src/syntax/expand/` — handle in expansion if the literal's
      elements need expanding

HIR (analysis):
- [ ] `src/hir/analyze/forms.rs` — add `SyntaxKind::YourType` arm in
      `analyze_expr` (typically desugars to a call to a constructor
      primitive)
- [ ] `src/hir/analyze/destructure.rs` — add destructuring support if
      the collection can appear in binding patterns
- [ ] `src/hir/analyze/special.rs` — add `match` pattern analysis for
      the collection type
- [ ] `src/hir/pattern.rs` — add `HirPattern` variant for pattern
      matching

Value (runtime representation):
- [ ] Follow the "Adding a new heap type" checklist above

LIR (lowering):
- [ ] `src/lir/lower/pattern.rs` — add pattern matching lowering (type
      guard + element extraction)
- [ ] `src/lir/lower/control.rs` — update destructuring lowering if
      needed

If the collection needs dedicated bytecode (type guards, element access):
- [ ] Follow the "Adding a new bytecode instruction" checklist above
      (e.g., `IsYourType`, `YourTypeGetOrNil` for pattern matching
      and destructuring)

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

## Failure triage

When CI fails or tests break, use this to find the cause fast.

| Failure | Symptom | Likely cause | Fix |
|---------|---------|--------------|-----|
| **elle-doc generation** | `docs` job fails on `./target/release/elle elle-doc/generate.lisp` | Using `nil?` to check end-of-list. Lists terminate with `EMPTY_LIST`, not `NIL`. `nil?` only matches `Value::NIL`. | Use `empty?` for list termination checks. Check `elle-doc/generate.lisp` and `elle-doc/lib/` for recursive list functions. Also check: string operations, `get` on tables returning nil for missing keys (line 603 pattern: `(if (nil? existing) (list) existing)`). |
| **Clippy** | `clippy` job fails | Any Rust warning. CI runs `cargo clippy --workspace --all-targets --all-features -- -D warnings`. | Run `cargo clippy --workspace --all-targets -- -D warnings` locally. Common hits: unused imports, unused variables, redundant clones, missing `#[allow(...)]` on intentionally dead code. |
| **Property tests** | `test` job fails with `proptest` output showing a shrunk counterexample | The shrunk output shows the *minimal* failing input. Read the `Minimal failing input:` line — it gives concrete values for each `in` clause in the `proptest!` block. | Reproduce with the exact shrunk values as a unit test. Check `proptest-regressions/` files — proptest persists failing seeds there. The strategies live in `tests/property/strategies.rs`. |
| **Integration tests** | `test` job fails in `tests/integration/` | Tests use `eval_source()` from `tests/common/mod.rs`, which runs the full pipeline: VM + primitives + stdlib + symbol table. Failures mean the pipeline produces wrong results or panics. | Read the assertion: `eval_source("(expr)")` returns `Result<Value, String>`. Check whether the test expects `.unwrap()` (success) or `.is_err()` (compile/runtime error). Common trap: `Value::EMPTY_LIST` is truthy, `Value::NIL` is falsy. |
| **Formatting** | `fmt` job fails | Unformatted Rust code. | Run `cargo fmt`. No arguments needed. CI runs `cargo fmt -- --check`. |
| **Rustdoc** | `docs` job fails on `cargo doc` step | Broken intra-doc links, malformed doc comments, or doc warnings. CI sets `RUSTDOCFLAGS="-D warnings"` with `--document-private-items`. | Run `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items` locally. Common: referencing a renamed/removed type in a `///` comment. |
| **Examples** | `examples` job fails | Each `.lisp` file in `examples/` runs with `timeout 10s`. Failures are either: (1) runtime error (assertion failure via `assert-eq` from `examples/assertions.lisp`, exits with code 1), (2) timeout (infinite loop or unbounded recursion), (3) semantic change broke expected output. | Run `cargo run -- examples/failing.lisp` locally. Examples use `assert-eq`, `assert-true`, etc. from `assertions.lisp` — check the assertion message for which invariant broke. Timeouts usually mean a recursive function hit the `nil?` vs `empty?` trap and never terminates. |
| **Benchmarks** | `benchmarks` job fails | Compilation error in bench code, or benchmark binary panics. Regressions are reported but don't fail CI (`fail-on-alert: false`). | Run `cargo bench --bench benchmarks` locally. Bench failures are usually compilation errors from changed APIs, not performance regressions. |
| **Tests on nightly** | `test` job fails only on nightly, passes on stable/beta | Nightly Rust introduced a new warning or changed behavior. | Check the nightly-specific error. If it's a new clippy lint or warning, add a targeted `#[allow(...)]` or fix the code. File an issue if it's a Rust regression. |

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

5. Read [`docs/cookbook.md`](docs/cookbook.md) for step-by-step recipes for common cross-cutting changes (new primitives, heap types, bytecode instructions, special forms, lint rules, prelude macros).
6. Read [`tests/AGENTS.md`](tests/AGENTS.md) for test organization, helpers, and how to add new tests.

## Agent specs

Agent spec files live in `.opencode/agents/`. Some runtimes rewrite this path
in the system prompt (e.g. to `.Claude/agents/`). If a rewritten path doesn't
resolve, always check `.opencode/agents/` — that is the canonical location.
