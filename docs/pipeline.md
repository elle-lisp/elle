# pipeline

Compilation entry points. Orchestrates Reader → Expander → Analyzer → Lowerer → Emitter.

Single file: `src/pipeline.rs` (~415 lines of implementation, ~1350 lines of tests).

## Public API

### Types

```rust
pub struct CompileResult {
    pub bytecode: Bytecode,
    pub warnings: Vec<String>,  // currently always empty
}

pub struct AnalyzeResult {
    pub hir: Hir,
}
```

### Functions

| Function | Lines | VM for macros | Fixpoint? | Callers |
|----------|-------|---------------|-----------|---------|
| `compile` | 119–151 | Internal | No | REPL (`main.rs:169,289`), integration tests |
| `compile_all` | 162–261 | Internal | Yes | `main.rs:86` (file/stdin), `modules.rs:78` (`import-file`) |
| `eval` | 266–291 | Borrowed | No | `init_stdlib` (`higher_order_def.rs`, `time_def.rs`, `module_init.rs`), tests |
| `eval_all` | 298–309 | Internal (delegates to `compile_all`) | Yes | Tests |
| `eval_syntax` | 91–113 | Borrowed | No | `macro_expand.rs:150` (macro body evaluation) |
| `analyze` | 313–326 | Borrowed | No | `hir/lint.rs`, `hir/symbols.rs` (tests only) |
| `analyze_all` | 330–413 | Borrowed | Yes | LSP (`lsp/state.rs:90`), linter (`lint/cli.rs:53`), property tests |

### Signatures

```rust
pub fn compile(source: &str, symbols: &mut SymbolTable) -> Result<CompileResult, String>
pub fn compile_all(source: &str, symbols: &mut SymbolTable) -> Result<Vec<CompileResult>, String>
pub fn eval(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Value, String>
pub fn eval_all(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Value, String>
pub fn eval_syntax(syntax: Syntax, expander: &mut Expander, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Value, String>
pub fn analyze(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<AnalyzeResult, String>
pub fn analyze_all(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Vec<AnalyzeResult>, String>
```

## VM ownership patterns

Two distinct patterns exist, and confusing them causes bugs:

**Internal VM** (`compile`, `compile_all`): These functions create a fresh
`VM::new()` and a fresh `Expander::new()`. The internal VM is used only for
macro expansion during compilation. Macro side effects don't persist. Primitives
are registered into this VM via `register_primitives`. This is the correct
pattern for batch compilation where the caller doesn't need a running VM.

**Borrowed VM** (`eval`, `eval_syntax`, `analyze`, `analyze_all`):
These borrow the caller's `&mut VM`. The same VM is used for both macro
expansion and (for `eval`) execution. This means macro side effects persist
in the caller's VM. This is the correct pattern for stdlib initialization
and macro body evaluation where state must accumulate.

**Hybrid** (`eval_all`): Delegates compilation to `compile_all` (which creates
an internal VM for macro expansion), then executes each compiled form on the
caller's borrowed VM. Macro side effects do NOT persist in the caller's VM.

**`eval_syntax` is special**: It also borrows the caller's `Expander`, not just
the VM. This is because it's called from inside `Expander::expand_macro_call_inner`
— the macro expansion engine needs to compile and run a macro body while
preserving the current expansion context (macro registry, scope state). Nested
macro calls work because the same Expander is threaded through.

**Primitive metadata divergence**: `compile`/`compile_all` call
`register_primitives()` which returns effect and arity metadata as a side
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

Used by `compile_all`, `eval_all` (via `compile_all`), and `analyze_all` to
correctly infer effects for mutually recursive top-level definitions.

### Problem

When compiling `(def f (fn (x) (g x)))` followed by `(def g (fn (x) (f x)))`,
the analyzer sees `f` before `g` exists. Without pre-scanning, `g` would be
treated as an unknown global with `Polymorphic` effect, making `f` also
`Polymorphic` — even if both are actually `Pure`.

### Algorithm (lines 162–261 in `compile_all`, 330–413 in `analyze_all`)

The algorithm has five phases (not to be confused with the max iteration
count of 10 within phase 4):

1. **Parse and expand** all forms upfront (lines 170–174). Expansion is
   idempotent so this only happens once.

2. **Pre-scan for `(def name (fn ...))` patterns** (lines 176–186). For each
   match, seed `global_effects` with `Effect::none()` (optimistic — assume
   pure) and extract syntactic arity into `global_arities`.

3. **Pre-scan for `(def name ...)` patterns** (lines 188–195). Track all
   `def` bindings as immutable globals for cross-form immutability checking.

4. **Fixpoint iteration** (lines 197–241, max 10 iterations):
   - Clear `analysis_results`
   - For each form, create a fresh `Analyzer` seeded with:
     - `global_effects` from previous iteration (or pre-scan)
     - `global_arities` from pre-scan + previous forms
     - `immutable_globals` from pre-scan + previous forms
   - Analyze the form, collect actual inferred effects via
     `analyzer.take_defined_global_effects()`
   - After all forms: compare `new_global_effects` with `global_effects`
   - If equal → converged, break
   - If different → update `global_effects`, re-analyze all forms

5. **Lower and emit** (lines 243–258). Only after convergence.

### Convergence

The algorithm converges because effects form a lattice: `Pure` < `Yields` <
`Polymorphic`. Each iteration can only move effects upward (from the optimistic
`Pure` seed toward the true effect). Once no effect changes, the fixpoint is
reached. The max of 10 iterations is a safety bound — in practice, convergence
happens in 1–3 iterations.

### Duplication

The fixpoint loop is duplicated between `compile_all` (lines 162–261) and
`analyze_all` (lines 330–413). The logic is nearly identical; the only
difference is that `compile_all` calls `mark_tail_calls` during iteration and
then lowers/emits, while `analyze_all` skips tail call marking and returns
`AnalyzeResult` directly.

## Pre-scanning functions

### `scan_define_lambda` (lines 35–66)

```rust
fn scan_define_lambda(syntax: &Syntax, symbols: &mut SymbolTable) -> Option<(SymbolId, Option<Arity>)>
```

Matches expanded syntax of the form `(var/def name (fn ...))`. Returns the
interned `SymbolId` and the syntactic arity (number of parameters, if the
parameter list is a simple list). Used to seed the fixpoint loop with
optimistic `Effect::none()` and known arities before analysis begins.

This operates on **expanded** syntax — `defn` has already been desugared to
`(def name (fn ...))` by the Expander.

### `scan_const_binding` (lines 70–83)

```rust
fn scan_const_binding(syntax: &Syntax, symbols: &mut SymbolTable) -> Option<SymbolId>
```

Matches `(def name ...)` patterns (not `var`). Returns the `SymbolId`. Used to
populate `immutable_globals` so the analyzer can reject `(set name ...)` on
`def`-bound globals across form boundaries.

## Compilation phases (single-form)

Every compilation path follows the same five phases:

1. **Read**: `read_syntax(source)` → `Syntax`
2. **Expand**: `expander.expand(syntax, symbols, vm)` → expanded `Syntax`
3. **Analyze**: `Analyzer::new_with_primitives(symbols, effects, arities)` →
   `analyzer.analyze(&expanded)` → `AnalysisResult { hir, .. }`
4. **Tail call marking**: `mark_tail_calls(&mut analysis.hir)` (mutates HIR in place)
5. **Lower + Emit**: `Lowerer::new().with_intrinsics(intrinsics).lower(&hir)` →
   `LirFunction` → `Emitter::new_with_symbols(snapshot).emit(&lir_func)` → `Bytecode`

`analyze` and `analyze_all` stop after phase 3 (no lowering or emission).

## Known issues

**GitHub issue #375** tracks the plan for pipeline decomposition. Current
structural problems:

- The fixpoint loop is duplicated between `compile_all` and `analyze_all`
- `CompileResult.warnings` is always empty (dead field)
- Single-form functions (`compile`, `eval`, `analyze`) don't benefit from
  cross-form effect inference — a file compiled via `compile` instead of
  `compile_all` will treat all global calls as `Polymorphic`
- The REPL uses `compile` (single-form), so REPL-defined functions don't
  get cross-form effect inference
