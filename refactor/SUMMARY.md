# Elle Refactoring Summary

> Last updated: February 2026

## Completed work

### Value representation (Feb 2025)
NaN-boxed 8-byte `Value` with tagged pointers. Immediate encoding for nil,
bool, int (i48), symbol, keyword, float. Heap allocation via `HeapObject`
for strings, cons cells, vectors, tables, closures, exceptions, coroutines,
cells, continuations. `Value` is `Copy`. Two cell types: `Cell`
(user-created via `box`) and `LocalCell` (compiler-created for mutable
captures) — distinguished by a bool flag on `HeapObject::Cell`.

### Compilation pipeline (Feb 2025)
```
Source → Reader → Syntax → Expander → Syntax → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM
```

- **Syntax**: S-expression AST with spans. Macro expansion operates here.
- **HIR**: Binding resolution (`BindingId`), capture analysis, effect
  inference, tail call marking. Linting and symbol extraction for IDE.
- **LIR**: SSA form with virtual registers and basic blocks. Yield is a
  terminator. All control flow uses proper basic blocks (no inline jumps).
- **Emitter**: Stack simulation translates registers to operand stack
  operations. Patches jump offsets. Carries state across block boundaries.

### Lexical scoping (Feb 2025)
`BindingId` throughout HIR. Loop variables are locals. Mutable captures use
`LocalCell`. `cell_params_mask` on `Closure` tracks which parameters need
cell wrapping.

### CPS rework — Phases 0-4 (Feb 2025 – Feb 2026)

| Phase | PR | What |
|-------|----|------|
| 0 | #275 | Property tests for coroutines, effect threading fixes |
| 1 | #276 | First-class continuations: `ContinuationFrame`, `ContinuationData`, `Value::Continuation` |
| 2 | #277 | Deleted CPS interpreter (~4,400 lines), simplified `Coroutine` (7→4 fields) |
| 3 | #278 | Exception handler state in continuations, O(1) frame append, edge case tests |
| 4 | #279 | Yield as LIR terminator, multi-block functions, `LoadResumeValue` pseudo-instruction |

### Phase B: Hammer time (PR #280, Feb 2026)

| Sub | What |
|-----|------|
| B.1 | Old JIT deletion: removed previous Cranelift code, old compiler, ~12,500 lines |
| B.2 | value_old migration: all types now in `value/` submodules |
| B.3 | LocationMap: source locations flow through entire pipeline |
| B.4 | Thread transfer tests: closures transfer with location data |

### Phase C: Macros and modules (PR #281, Feb 2026)

| Sub | What |
|-----|------|
| C.1 | Quasiquote templates: `eval_quasiquote_to_syntax` for direct Syntax tree construction |
| C.2 | Compile-time macro operations: `macro?` and `expand-macro` in Expander |
| C.3 | Module-qualified names: lexer recognizes `module:name`, Expander resolves to flat names |
| Tests | All 8 previously-ignored tests now pass; zero ignored tests remain |

### Phase D: Documentation cleanup (PR #282)
Stale docs removed. AGENTS.md files updated. Unused deps removed.

### Tail call optimization (PR #272, Feb 2025)
HIR tail-call marking pass (`hir/tailcall.rs`). Lowerer emits
`LirInstr::TailCall`. VM trampoline via `pending_tail_call`. Handles
50,000+ depth for self-recursion, accumulator patterns, and mutual recursion.

### elle-lint and elle-lsp migration (PR #273, Feb 2025)
Both products use the new pipeline exclusively. HIR-based linter and symbol
extraction. No dependency on old `Expr` type.

### Interprocedural effect tracking (PRs #283-#284, Feb 2026)
`effect_env` maps `BindingId` → `Effect` for locally-defined lambdas.
`primitive_effects` maps `SymbolId` → `Effect` for primitives.
Polymorphic effect inference for higher-order functions.
`set!` invalidates effect tracking. Sound defaults (Yields for unknown).

### File splitting and cleanup (PR #285)
Fixed Instruction enum gap. Split 5 oversized files. Renamed broken demo.

### JIT Phases 1-3 (PRs #286-#287)
Cranelift scaffold with 5-parameter calling convention. VM integration
with hotness tracking and lazy compilation. Full LirInstr coverage:
Call, TailCall, Cons, Car, Cdr, MakeVector, cells, globals.

### JIT Phase 4 (PR #288, open on `jit-phase-4`)

Feature gate removal (JIT always enabled). Self-tail-call optimization
compiles to native loops via `loop_header` block. Exception propagation
after JIT calls. Cross-form effect tracking with fixpoint iteration.

Performance optimizations:
- `Vec<Value>` globals (replacing HashMap, UNDEFINED sentinel)
- `SmallVec<[ExceptionHandler; 2]>` for handler stacks
- Pre-allocated closure environments via `env_capacity()`
- Eliminated inline jumps from LIR (proper basic blocks only)
- Locally-defined variable support in JIT
- Panic on JIT compile failure for pure functions

### Debugging toolkit and effect unification (PR #291, Feb 2026)
- `disbit`/`disjit` primitives for bytecode and Cranelift IR inspection
- Introspection predicates: `closure?`, `jit?`, `pure?`, `coro?`, `raises?`,
  `arity`, `captures`, `bytecode-size`
- Effect type reworked to struct `{ yield_behavior, may_raise }`
- `is_pure()` fixed — pure functions CAN raise
- Every primitive declares its effect at registration time
- Design docs: `docs/EFFECTS.md`, `docs/JANET.md`

## Hammer Time II analysis (Feb 2026)

Full codebase audit. 46,170 lines in `src/`, 2,063 in `elle-lsp/elle-lint`.
19 top-level modules, 3 workspace members.

### Codebase health

| Metric | Value |
|--------|-------|
| Total source lines (src/) | 46,170 |
| Files over 500 lines | ~35 |
| Files over 300 lines (old target) | 66 |
| Production `unwrap()` on user data | 7 |
| Duplicated test helpers | 20+ files |
| Dead re-exports | 4 |
| TODOs in production code | 0 |
| TODOs in test code | 1 (issue #78) |

### File size analysis

Previous target was 300 lines / 5-10KB. Revised to 500 lines with
category-specific exceptions after analysis showed 66/130 files violated
the old target — the convention was wrong, not the code.

**Largest files:**

| File | Lines | Bytes | Category |
|------|------:|------:|----------|
| `pipeline.rs` | 1,532 | 53K | 74% inline tests — extract to integration tests |
| `primitives/string.rs` | 1,219 | 35K | Independent functions — split |
| `primitives/file_io.rs` | 1,047 | 30K | Independent functions — split |
| `hir/analyze/binding.rs` | 923 | 36K | Cohesive binding analysis — split by form |
| `lir/emit.rs` | 903 | 36K | Dispatch table — keep, extract helpers within file |
| `primitives/fibers.rs` | 890 | 27K | Independent functions — split |
| `jit/translate.rs` | 780 | 34K | Dispatch table — keep |
| `primitives/table.rs` | 763 | 23K | Borderline — leave for now |
| `primitives/list.rs` | 743 | 22K | Borderline — leave for now |
| `reader/syntax_parser.rs` | 721 | 25K | Parser — keep |

### Structural issues found

**Production unwraps (7):** `primitives/bitwise.rs` has 6 `as_int().unwrap()`
calls in a validate-then-unwrap pattern. `primitives/string.rs` has 1
`chars().next().unwrap()` after a count check. All are safe today but fragile
— changing the validation logic would silently introduce panics.

**Test helper duplication:** `eval()` helper copy-pasted in 20+ test files
with 3 semantic variants: (1) standard with `set_symbol_table`, (2) without
FFI context, (3) JIT-specific using `pipeline::eval()`. `setup()` duplicated
in 7+ files.

**Dead re-exports:**
- `compiler/mod.rs` re-exports `symbols::{SymbolDef, SymbolIndex, SymbolKind}` — 0 uses
- `vm/core.rs` re-exports `CallFrame as FiberCallFrame` — 0 uses
- `error/sourceloc.rs` is a 6-line re-export shim — 1 use in `primitives/concurrency.rs`
- `value/heap.rs` re-exports `Arity` from `types` — 0 uses via `heap::Arity`

**`register_builtin_docs()` (333 lines):** Documents special forms and prelude
macros (`if`, `let`, `fn`, `defn`, `->`, `try`, etc.). NOT derivable from
`PRIMITIVES` tables. Must be preserved but can be extracted to its own file.

**Circular dependencies:** `pipeline <-> primitives` (module loading calls
`compile`) and `primitives <-> vm` (registration vs dispatch). Both are
logical cycles already broken at runtime. The `pipeline <-> primitives` cycle
is mediated by unsafe global state (`get_vm_context()`). Not worth breaking
— the cure (callbacks/trait objects) is worse than the disease.

**`elle-lint` in `elle-lsp/Cargo.toml`:** Declared but unused at the Rust
level. No `use elle_lint::` anywhere in elle-lsp.

### What the analysis ruled out

| Considered | Rejected because |
|------------|------------------|
| Split `lir/emit.rs` into multiple files | 459-line match is a flat dispatch table — locality matters more than file size |
| Split `jit/translate.rs` into files | Same reason |
| Break `pipeline <-> primitives` cycle | Already broken by raw pointer. Real fix = redesign unsafe global state |
| Shared import prelude for primitives | Saves ~65 lines across 13 files. Not worth the indirection. |
| File size limit on test files | Tests are append-only, rarely clobbered |
| 300-line file target | 66/130 files violated it. 500 lines is the right grain for a compiler. |

## Current state

1,768 tests passing. Zero ignored (except 2 doc-tests). Clean clippy,
fmt, rustdoc. nqueens N=12 produces 14,200 solutions (~18-19s release).

### Remaining bottleneck
JIT-compiled code calls `elle_jit_call` for non-self calls, which always
routes through the interpreter (`vm.execute_bytecode`). JIT code is only
0.36% of nqueens runtime; 59% is interpreter overhead from JIT->VM bounces.

### Next: Hammer Time II (Phases H.1-H.5)

See `PLAN.md` for the detailed refactoring plan. Execution order:
1. H.1 — Fix 7 production unwraps (correctness)
2. H.3 — Remove dead re-exports (cleanup)
3. H.4 — Split oversized primitive files (line count)
4. H.2 — Consolidate test infrastructure (maintainability)
5. H.5 — Split analyzer binding.rs (complexity)

### After Hammer Time II: Fiber/Signal System
See `docs/FIBERS.md` for the implementation plan and `docs/EFFECTS.md` for
the design rationale. Unifies exception handling, coroutines, and effects
into a single fiber/signal mechanism. Surface syntax: `try`/`catch` + `defer`.

## Not yet done

### Semantic gaps
- Module system: `import` emits nil (module-qualified names now supported)
- `higher_order.rs` map/filter/fold don't support closures (only native fns)

## What was planned but won't happen

| Original plan | Actual outcome |
|---------------|----------------|
| `Expr` as intermediate between Syntax and HIR | Skipped — Syntax -> HIR directly |
| CPS as canonical IR for all yielding code | Replaced by bytecode continuations |
| Unified `LError` error system | Two-channel system instead |
| `Closure`/`JitClosure` merge in Value | JIT removed entirely (Phase B) |
| Arena-based memory | Deferred indefinitely |
| Tiered JIT | Deferred to future Phase E |
| Bytecode format redesign (32-bit instructions) | Not planned |
| Inline jump instructions in LIR | Eliminated in favor of proper basic blocks |
| CL condition/restart system | Replaced by try/catch + defer over fibers |
| 300-line file target | Replaced by 500-line target with category exceptions |

## File inventory

See `AGENTS.md` at repository root for authoritative module descriptions.
