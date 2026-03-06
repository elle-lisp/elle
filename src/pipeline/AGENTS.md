# pipeline

Compilation entry points: orchestrate the full pipeline from source to bytecode or value.

## Responsibility

Provide high-level functions that coordinate the compilation pipeline:
- **`analyze()`** — Source → HIR (no bytecode)
- **`analyze_all()`** — Multiple forms → HIR (with fixpoint effect inference)
- **`compile()`** — Source → Bytecode
- **`compile_all()`** — Multiple forms → Bytecode (with fixpoint effect inference)
- **`eval()`** — Source → Value (compile + execute)
- **`eval_all()`** — Multiple forms → Value (compile + execute)
- **`eval_syntax()`** — Syntax → Value (for macro body evaluation)

Does NOT:
- Parse source (that's `reader`)
- Expand macros (that's `syntax/expand`)
- Analyze bindings (that's `hir/analyze`)
- Lower to LIR (that's `lir/lower`)
- Emit bytecode (that's `lir/emit`)
- Execute bytecode (that's `vm`)

## Key types

| Type | Purpose |
|------|---------|
| `CompileResult` | Bytecode + warnings |
| `AnalyzeResult` | HIR (no bytecode) |

## Data flow

```
Source code
    │
    ├─► Reader → Syntax
    │
    ├─► Expander → Syntax (expanded)
    │
    ├─► Analyzer → HIR
    │
    ├─► (optional) Lowerer → LIR
    │
    ├─► (optional) Emitter → Bytecode
    │
    └─► (optional) VM → Value
```

## Fixpoint iteration for effect inference

`compile_all()` and `analyze_all()` use fixpoint iteration to correctly infer effects for mutually recursive top-level defines:

1. **Pre-scan** all forms for `(def name (fn ...))` patterns via `scan::prescan_forms()`
2. **Seed** `global_effects` with `Effect::none()` for all such defines (optimistic)
3. **Analyze** all forms, collecting actual inferred effects via `fixpoint::run_fixpoint()`
4. **If any effect changed**, re-analyze with corrected effects
5. **Repeat** until stable (max 10 iterations)

This ensures that mutually recursive functions have correct effect annotations before lowering.

## Macro expansion caching

The pipeline uses a thread-local cache for macro expansion:

- **`cache::get_cached_expander_and_meta()`** — Returns a cached `Expander` + `PrimitiveMeta` (effects + arities)
- **`cache::get_compilation_cache()`** — Returns a cached VM pointer + `Expander` + `PrimitiveMeta` for compilation

The cached VM is used only for macro body evaluation. Macro side effects don't persist beyond compilation.

## Symbol table context

The pipeline requires thread-local context pointers for macro expansion:

- **`set_vm_context(vm_ptr)`** — Set the current VM for macro body evaluation
- **`set_symbol_table(symbols_ptr)`** — Set the current SymbolTable for symbol interning during macros

These are set by the caller (e.g., `tests/common/mod.rs::eval_source()`) and must be cleared after use to avoid affecting other tests.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~30 | Re-exports and type definitions |
| `analyze.rs` | ~65 | `analyze()` and `analyze_all()` entry points |
| `compile.rs` | ~120 | `compile()` and `compile_all()` entry points |
| `eval.rs` | ~95 | `eval()`, `eval_all()`, `eval_syntax()` entry points |
| `cache.rs` | ~100 | Thread-local caching for Expander and VM |
| `scan.rs` | ~150 | Pre-scanning for global defines and arities |
| `fixpoint.rs` | ~200 | Fixpoint iteration for effect inference |

## Invariants

1. **Primitive names must be interned before compilation.** Call `intern_primitive_names(symbols)` at the start of `compile()` and `compile_all()` to ensure SymbolIds match the cached `PrimitiveMeta`.

2. **Symbol table context must be set for macro expansion.** Call `set_symbol_table()` before macro expansion and clear it afterward.

3. **VM context must be set for macro body evaluation.** Call `set_vm_context()` before macro expansion and clear it afterward.

4. **Fixpoint iteration is bounded.** Max 10 iterations to prevent infinite loops. If effects don't stabilize, compilation fails.

5. **Macro side effects don't persist.** The cached VM used for macro expansion is separate from the caller's VM. Macro side effects (e.g., `(print "hello")`) don't affect the caller's VM state.

6. **Tail calls are marked post-analysis.** After HIR analysis, `mark_tail_calls()` is called to identify tail positions. This must happen before lowering.

7. **Intrinsics are built per-compilation.** `build_intrinsics()` and `build_immediate_primitives()` are called during lowering to specialize operators based on the current symbol table.

## When to modify

- **Adding a new compilation phase**: Add a new function in the appropriate file (`analyze.rs`, `compile.rs`, `eval.rs`)
- **Changing fixpoint iteration**: Update `fixpoint.rs` and `scan.rs`
- **Changing caching strategy**: Update `cache.rs`
- **Changing symbol table handling**: Update all entry points to ensure context is set/cleared correctly

## Common pitfalls

- **Forgetting to intern primitive names**: If SymbolIds don't match `PrimitiveMeta`, effect and arity tracking fails
- **Not setting symbol table context**: Macros that use `gensym` will fail if context is not set
- **Not clearing context after use**: Leaving context pointers set can affect subsequent tests or compilations
- **Not calling `mark_tail_calls()`**: Tail call optimization is skipped if this is not called
- **Not building intrinsics**: Operator specialization is skipped if intrinsics are not built
