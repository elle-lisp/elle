# Elle Refactoring Summary

> Last updated: February 2026

## Completed work

### Value representation (Feb 2025)
NaN-boxed 8-byte `Value` with tagged pointers. Immediate encoding for nil,
bool, int (i48), symbol, keyword, float. Heap allocation via `HeapObject`
for strings, cons cells, vectors, tables, closures, conditions, coroutines,
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
| B.1 | JIT deletion: removed Cranelift, old compiler, ~12,500 lines |
| B.2 | value_old migration: all types now in `value/` submodules |
| B.3 | LocationMap: source locations flow through entire pipeline |
| B.4 | Thread transfer tests: closures transfer with location data |

### Phase C: Macros and modules (PR #281, Feb 2026)

| Sub | What |
|-----|------|
| C.1 | Quasiquote templates: `eval_quasiquote_to_syntax` for direct Syntax tree construction |
| C.2 | Compile-time macro operations: `macro?` and `expand-macro` in Expander |
| C.3 | Module-qualified names: lexer recognizes `module:name`, Expander resolves to flat names |
| C.4 | yield-from delegation: `delegate` field on Coroutine, proper suspension semantics |
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

## Current state

~1,130+ tests passing. Zero ignored (except 2 doc-tests). Clean clippy,
fmt, rustdoc. nqueens N=12 produces 14,200 solutions (~18-19s release).

### Remaining bottleneck
JIT-compiled code calls `elle_jit_call` for non-self calls, which always
routes through the interpreter (`vm.execute_bytecode`). JIT code is only
0.36% of nqueens runtime; 59% is interpreter overhead from JIT→VM bounces.

### Next: Debugging Toolkit
See `docs/DEBUGGING.md` for design, `refactor/PLAN.md` for implementation
steps. Introspection primitives, clock API, raises tracking, benchmarking
macros — all from Elle code, no recompilation needed.

## Not yet done

### Semantic gaps
- `handler-bind` (non-unwinding handlers): stub
- Signal/restart system: `InvokeRestart` opcode is a no-op
- Module system: `import` emits nil (module-qualified names now supported)

### Error system
The unified `LError` from the original plan was never implemented. Current
system uses two channels:
- `Err(String)` = VM bug (uncatchable)
- `vm.current_exception` = runtime error (catchable by `handler-case`)

Documented in `docs/EXCEPT.md`. Functional but not elegant.

## What was planned but won't happen

| Original plan | Actual outcome |
|---------------|----------------|
| `Expr` as intermediate between Syntax and HIR | Skipped — Syntax → HIR directly |
| CPS as canonical IR for all yielding code | Replaced by bytecode continuations |
| Unified `LError` error system | Two-channel system instead |
| `Closure`/`JitClosure` merge in Value | JIT removed entirely (Phase B) |
| Arena-based memory | Deferred indefinitely |
| Tiered JIT | Deferred to future Phase E |
| Bytecode format redesign (32-bit instructions) | Not planned |
| Inline jump instructions in LIR | Eliminated in favor of proper basic blocks |

## File inventory

See `AGENTS.md` at repository root for authoritative module descriptions.
