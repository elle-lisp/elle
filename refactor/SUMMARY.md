# Elle Refactoring Summary

> Last updated: February 2026

## Completed work

### Value representation (Feb 2025)
NaN-boxed 8-byte `Value` with tagged pointers. Immediate encoding for nil,
bool, int (i48), symbol, keyword, float. Heap allocation via `HeapObject`
for strings, cons cells, vectors, tables, closures, conditions, coroutines,
cells, continuations. `Value` is `Copy`. Two cell types remain: `Cell`
(user-created via `box`, explicit) and `LocalCell` (compiler-created for
mutable captures, auto-unwrapped) — distinguished by a bool flag on
`HeapObject::Cell`.

### Compilation pipeline (Feb 2025)
```
Source → Reader → Syntax → Expander → Syntax → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM
```

- **Syntax**: S-expression AST with spans. Macro expansion operates here.
- **HIR**: Binding resolution (`BindingId`), capture analysis, effect
  inference, tail call marking. Linting and symbol extraction for IDE.
- **LIR**: SSA form with virtual registers and basic blocks. Yield is a
  terminator. Inline jumps for if/while/for/and/or/cond/match.
- **Emitter**: Stack simulation translates registers to operand stack
  operations. Patches jump offsets. Carries state across yield boundaries.

### Lexical scoping (Feb 2025)
`BindingId` throughout HIR. No `VarRef` in new pipeline. Loop variables
are locals. Mutable captures use `LocalCell`. `cell_params_mask` on
`Closure` tracks which parameters need cell wrapping.

### CPS rework — Phases 0-4 (Feb 2025 – Feb 2026)

| Phase | PR | What |
|-------|----|------|
| 0 | #275 | Property tests for coroutines, effect threading fixes |
| 1 | #276 | First-class continuations: `ContinuationFrame`, `ContinuationData`, `Value::Continuation` |
| 2 | #277 | Deleted CPS interpreter (~4,400 lines), simplified `Coroutine` (7→4 fields) |
| 3 | #278 | Exception handler state in continuations, O(1) frame append, edge case tests |
| 4 | #279 | Yield as LIR terminator, multi-block functions, `LoadResumeValue` pseudo-instruction |

### Phase B: Hammer time (Feb 2026)

| PR | What |
|----|------|
| B.1 | JIT deletion: removed Cranelift, old compiler, ~12,500 lines |
| B.2 | value_old migration: all types now in `value/` submodules |
| B.3 | LocationMap: source locations flow through entire pipeline |
| B.4 | Thread transfer tests: closures transfer with location data |

### Phase C: Macros and modules (Feb 2026)

| Component | What |
|-----------|------|
| C.1 | Quasiquote templates: `eval_quasiquote_to_syntax` for direct Syntax tree construction |
| C.2 | Compile-time macro operations: `macro?` and `expand-macro` in Expander |
| C.3 | Module-qualified names: lexer recognizes `module:name`, Expander resolves to flat names |
| C.4 | yield-from delegation: `delegate` field on Coroutine, proper suspension semantics |
| Tests | All 8 previously-ignored tests now pass; zero ignored tests remain |

### Tail call optimization (Feb 2025, PR #272)
HIR tail-call marking pass (`hir/tailcall.rs`). Lowerer emits
`LirInstr::TailCall`. VM trampoline via `pending_tail_call`. Handles
50,000+ depth for self-recursion, accumulator patterns, and mutual recursion.

### elle-lint and elle-lsp migration (Feb 2025, PR #273)
Both products use the new pipeline exclusively. HIR-based linter and symbol
extraction. No dependency on old `Expr` type.

## Not yet done

### Semantic gaps
- `handler-bind` (non-unwinding handlers): stub
- Signal/restart system: `InvokeRestart` opcode is a no-op
- Effect enforcement at compile time: not started
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

## File inventory

See `AGENTS.md` at repository root for authoritative module descriptions.
