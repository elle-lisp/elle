# Compilation Pipeline

Compilation entry points. Orchestrates Reader → Expander → Analyzer → Lowerer → Emitter.

Module: `src/pipeline/` (7 files, ~540 lines of implementation).

## Contents

- [Public API](#public-api)
- [VM ownership patterns](#vm-ownership-patterns)
- [Expander lifecycle](#expander-lifecycle)
- [The fixpoint loop](#the-fixpoint-loop)
- [Pre-scanning functions](#pre-scanning-functions-in-srcpipelinescanrs)
- [Compilation phases (single-form)](#compilation-phases-single-form)
- [Compilation cache](#compilation-cache-in-srcpipelinecachersrs)
- [Known issues](#known-issues)

| File | Lines | Purpose |
|------|-------|---------|
| `mod.rs` | 28 | Module declarations, re-exports, type definitions |
| `cache.rs` | 92 | Thread-local compilation cache (VM, Expander, PrimitiveMeta) |
| `scan.rs` | 97 | Pre-scanning for forward declarations (`prescan_forms`, `scan_define_lambda`, `scan_const_binding`) |
| `fixpoint.rs` | 81 | Shared fixpoint loop (`run_fixpoint`) parameterized by post-analyze closure |
| `compile.rs` | 115 | `compile()`, `compile_file()` |
| `analyze.rs` | 65 | `analyze()`, `analyze_file()` |
| `eval.rs` | 90 | `eval()`, `eval_all()`, `eval_syntax()` |

## Public API

### Types

```rust
pub struct CompileResult {
    pub bytecode: Bytecode,
}

pub struct AnalyzeResult {
    pub hir: Hir,
}
```

### Functions

| Function | Lines | VM for macros | Fixpoint? | Callers |
|----------|-------|---------------|-----------|---------|
| `compile` | 119–151 | Internal | No | Integration tests |
| `compile_file` | 162–261 | Internal | Yes | `main.rs:86` (file/stdin), `modules.rs:78` (`import-file`) |
| `eval` | 266–291 | Borrowed | No | `init_stdlib` (`module_init.rs` — loads `stdlib.lisp`), tests |
| `eval_all` | 298–309 | Internal (delegates to `compile_file`) | Yes | Tests |
| `eval_file` | (new) | Borrowed | Yes | File evaluation |
| `eval_syntax` | 91–113 | Borrowed | No | `macro_expand.rs:150` (macro body evaluation) |
| `analyze` | 313–326 | Borrowed | No | `hir/lint.rs`, `hir/symbols.rs` (tests only) |
| `analyze_file` | 330–413 | Borrowed | Yes | LSP (`lsp/state.rs:90`), linter (`lint/cli.rs:53`), property tests |

### Signatures

```rust
pub fn compile(source: &str, symbols: &mut SymbolTable, source_name: &str) -> Result<CompileResult, String>
pub fn compile_file(source: &str, symbols: &mut SymbolTable, source_name: &str) -> Result<CompileResult, String>
pub fn eval(source: &str, symbols: &mut SymbolTable, vm: &mut VM, source_name: &str) -> Result<Value, String>
pub fn eval_all(source: &str, symbols: &mut SymbolTable, vm: &mut VM, source_name: &str) -> Result<Value, String>
pub fn eval_file(source: &str, symbols: &mut SymbolTable, vm: &mut VM, source_name: &str) -> Result<Value, String>
pub fn eval_syntax(syntax: Syntax, expander: &mut Expander, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Value, String>
pub fn analyze(source: &str, symbols: &mut SymbolTable, vm: &mut VM, source_name: &str) -> Result<AnalyzeResult, String>
pub fn analyze_file(source: &str, symbols: &mut SymbolTable, vm: &mut VM, source_name: &str) -> Result<AnalyzeResult, String>
```

## VM ownership patterns

Two distinct patterns exist, and confusing them causes bugs:

**Internal VM** (`compile`, `compile_file`): These functions create a fresh
`VM::new()` and a fresh `Expander::new()`. The internal VM is used only for
macro expansion during compilation. Macro side effects don't persist. Primitives
are registered into this VM via `register_primitives`. This is the correct
pattern for batch compilation where the caller doesn't need a running VM.

**Borrowed VM** (`eval`, `eval_syntax`, `analyze`, `analyze_file`):
These borrow the caller's `&mut VM`. The same VM is used for both macro
expansion and (for `eval`) execution. This means macro side effects persist
in the caller's VM. This is the correct pattern for stdlib initialization
and macro body evaluation where state must accumulate.

**Hybrid** (`eval_all`): Delegates compilation to `compile_file` (which creates
an internal VM for macro expansion), then executes each compiled form on the
caller's borrowed VM. Macro side effects do NOT persist in the caller's VM.

**`eval_syntax` is special**: It also borrows the caller's `Expander`, not just
the VM. This is because it's called from inside `Expander::expand_macro_call_inner`
— the macro expansion engine needs to compile and run a macro body while
preserving the current expansion context (macro registry, scope state). Nested
macro calls work because the same Expander is threaded through.

**Primitive metadata divergence**: `compile`/`compile_file` call
`register_primitives()` which returns signal and arity metadata as a side
effect of registration. `eval`/`analyze` and friends call `build_primitive_meta()`
instead, which builds the same metadata without registering anything (the
caller's VM already has primitives registered). This is why `compile` can work
with just `&mut SymbolTable` while `eval` needs `&mut VM`.

## Expander lifecycle

Every public function except `eval_syntax` creates a fresh `Expander::new()`
and calls `expander.load_prelude(symbols, vm)` before expanding user code.
The prelude (`prelude.lisp`, embedded via `include_str!`) defines macros like
`defn`, `let*`, `when`, `unless`, `try`/`catch`, etc.

`eval_syntax` reuses the caller's Expander because it's invoked mid-expansion.
The prelude is already loaded in that Expander.

The prelude is parsed and expanded on every `Expander` creation. This means
every call to `compile`, `eval`, `analyze`, etc. re-parses the prelude. This
is intentional — Expanders are not cached or reused across top-level calls.

## The fixpoint loop

Used by `compile_file`, `eval_all` (via `compile_file`), and `analyze_file` to
correctly infer signals for mutually recursive top-level definitions.

### Problem

When compiling `(def f (fn (x) (g x)))` followed by `(def g (fn (x) (f x)))`,
the analyzer sees `f` before `g` exists. Without pre-scanning, `g` would be
treated as an unknown name with `Polymorphic` signal, making `f` also
`Polymorphic` — even if both are actually `Silent`.

### Algorithm (in `src/pipeline/fixpoint.rs`)

The algorithm has five phases (not to be confused with the max iteration
count of 10 within phase 4):

1. **Parse and expand** all forms upfront. Expansion is idempotent so this
   only happens once. (In `compile_file` and `analyze_file` before calling
   `run_fixpoint`.)

2. **Pre-scan for `(def name (fn ...))` patterns** via `prescan_forms()`.
    For each match, seed `def_signals` with `Signal::silent()` (optimistic —
    assume silent) and extract syntactic arity into `def_arities`.

3. **Pre-scan for `(def name ...)` patterns** via `prescan_forms()`. Track all
   `def` bindings as immutable for cross-form immutability checking.

4. **Fixpoint iteration** (in `run_fixpoint()`, max 10 iterations):
   - Clear `analysis_results`
   - For each form, create a fresh `Analyzer` seeded with:
     - `def_signals` from previous iteration (or pre-scan)
     - `def_arities` from pre-scan + previous forms
     - `immutable_defs` from pre-scan + previous forms
   - Analyze the form, collect actual inferred signals via
     `analyzer.take_defined_signals()`
   - After all forms: compare `new_def_signals` with `def_signals`
   - If equal → converged, break
   - If different → update `def_signals`, re-analyze all forms

5. **Post-analysis callback** (parameterized by `post_analyze` closure):
   - `compile_file` passes `|a| mark_tail_calls(&mut a.hir)` to mark tail calls
   - `analyze_file` passes `|_| {}` (no-op, returns HIR as-is)

6. **Lower and emit** (only in `compile_file`, after convergence).

### Convergence

The algorithm converges because signals form a lattice: `Silent` < `Yields` <
`Polymorphic`. Each iteration can only move signals upward (from the optimistic
`Silent` seed toward the true signal). Once no signal changes, the fixpoint is
reached. The max of 10 iterations is a safety bound — in practice, convergence
happens in 1–3 iterations.

### Deduplication

The fixpoint loop lives in `run_fixpoint()` (in `src/pipeline/fixpoint.rs`),
parameterized by a `post_analyze` closure. Both `compile_file` and
`analyze_file` call through it.

## Pre-scanning functions (in `src/pipeline/scan.rs`)

### `prescan_forms()`

```rust
pub(super) fn prescan_forms(
    forms: &[Syntax],
    symbols: &mut SymbolTable,
) -> (HashMap<SymbolId, Signal>, HashMap<SymbolId, Arity>, HashSet<SymbolId>)
```

Unified pre-scan that processes all forms in a single pass, calling both
`scan_define_lambda` and `scan_const_binding` for each form. Returns:
- `def_signals`: `(def name (fn ...))` patterns seeded with `Signal::silent()`
- `def_arities`: syntactic arities from lambda parameter lists
- `immutable_defs`: all `(def name ...)` patterns

### `scan_define_lambda()`

```rust
pub(super) fn scan_define_lambda(
    syntax: &Syntax,
    symbols: &mut SymbolTable,
) -> Option<(SymbolId, Option<Arity>)>
```

Matches expanded syntax of the form `(var/def name (fn ...))`. Returns the
interned `SymbolId` and the syntactic arity (number of parameters, if the
parameter list is a simple list). Used to seed the fixpoint loop with
optimistic `Signal::silent()` and known arities before analysis begins.

This operates on **expanded** syntax — `defn` has already been desugared to
`(def name (fn ...))` by the Expander.

### `scan_const_binding()`

```rust
pub(super) fn scan_const_binding(
    syntax: &Syntax,
    symbols: &mut SymbolTable,
) -> Option<SymbolId>
```

Matches `(def name ...)` patterns (not `var`). Returns the `SymbolId`. Used to
populate `immutable_defs` so the analyzer can reject `(assign name ...)` on
`def`-bound names across form boundaries.

## Compilation phases (single-form)

Every compilation path follows the same five phases:

1. **Read**: `read_syntax(source, source_name)` → `Syntax`
2. **Expand**: `expander.expand(syntax, symbols, vm)` → expanded `Syntax`
3. **Analyze**: `Analyzer::new_with_primitives(symbols, signals, arities)` →
   `analyzer.analyze(&expanded)` → `AnalysisResult { hir, .. }`
4. **Tail call marking**: `mark_tail_calls(&mut analysis.hir)` (mutates HIR in place)
5. **Lower + Emit**: `Lowerer::new().with_intrinsics(intrinsics).lower(&hir)` →
   `LirFunction` → `Emitter::new_with_symbols(snapshot).emit(&lir_func)` → `Bytecode`

`analyze` and `analyze_file` stop after phase 3 (no lowering or emission).

## Compilation cache (in `src/pipeline/cache.rs`)

The module uses a thread-local `CompilationCache` to avoid per-call costs of
VM creation, primitive registration, and prelude loading.

### `get_compilation_cache()`

Returns `(*mut VM, Expander, PrimitiveMeta)` from the thread-local cache.
The VM's fiber is always reset before use. The Expander is cloned so each
pipeline call gets independent expansion state. Used by `compile` and
`compile_file`.

### `get_cached_expander_and_meta()`

Returns `(Expander, PrimitiveMeta)` from the thread-local cache without
borrowing the cached VM. Used by `eval`, `analyze`, and `analyze_file` which
have their own VM.

### Invariants

- Prelude must be 100% defmacro (no runtime definitions)
- Primitives must be registered before any pipeline function call
- Pipeline functions are not re-entrant (no nested compile/compile_file)
- Primitive registration order is deterministic (ALL_TABLES)

## Known issues

Single-form functions
(`compile`, `eval`, `analyze`) don't benefit from cross-form signal inference —
a file compiled via `compile` instead of `compile_file` will treat all
cross-form calls as `Polymorphic`. The REPL compiles each form individually
via `compile_file` and registers def bindings in the compilation cache
(`register_repl_binding`) so they are visible to subsequent compilations.
However, cross-form signal inference within a single REPL input is limited
to what `compile_file` can infer for each form in isolation.
