# Elle Roadmap

> Last updated: February 2026

## Where we are

Elle has a single compilation pipeline:

```
Source → Reader → Syntax → Expander → Syntax → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM
```

A Cranelift-based JIT compiles hot pure functions from LIR to native code.
The JIT is always enabled (not feature-gated). Self-tail-calls compile to
native loops. Cross-form effect tracking uses fixpoint iteration.

Source locations flow through the entire pipeline: Syntax spans → HIR spans →
LIR `SpannedInstr` → `LocationMap` in bytecode. Error messages include
file:line:col information.

Yield is a proper LIR terminator that splits functions into multiple basic
blocks. The emitter carries stack simulation state across yield boundaries.

### What works

- Full pipeline with property tests (1,768 tests, zero ignored)
- TCO via trampoline (50,000+ depth) and JIT native loops (self-tail-call)
- First-class continuations for coroutines across call boundaries
- Exception handlers preserved across yield/resume
- NaN-boxed 8-byte Value (Copy semantics, 48-bit signed integers)
- JIT compilation of pure functions (Cranelift backend)
- Cross-form effect tracking with fixpoint iteration
- elle-lint, elle-lsp, elle-doc all functional
- Clean clippy, fmt, rustdoc

### What needs work

- JIT intra-calling: JIT code bounces to interpreter for non-self calls
- `handler-bind` stub — replaced by fiber/signal model (see docs/FIBERS.md)
- `signal`/`error` are constructors — will become actual signal primitives in fiber model

## Current work: Fiber/Signal System (see docs/FIBERS.md)

The next major milestone unifies exception handling, coroutines, and effect
inference into a single fiber/signal mechanism. Surface syntax: `try`/`catch`/
`finally`. See `docs/FIBERS.md` for the implementation plan and
`docs/EFFECTS.md` for the design rationale.

### Remaining from Debugging Toolkit

- `Instant`/`Duration` heap types not yet implemented (clock/time primitives use floats for now)
- Benchmarking macros (`lib/bench.lisp`) not yet written

## Future work

### JIT Phase 5: Intra-JIT calling

Make `elle_jit_call` and `elle_jit_tail_call` check the JIT cache and call
JIT code directly instead of always bouncing to the interpreter. This is the
single highest-impact optimization remaining for nqueens-class benchmarks.

Deferred until the debugging toolkit is in place so we can measure the
improvement from Elle code.

### JIT Phase 6: Optimization

- Inline type checks for arithmetic fast paths
- JIT-native exception handling
- Arena allocation for hot-path allocations

### Effect system: fiber/signal migration

- Replace `Effect { yield_behavior, may_raise }` with `SignalBits` bitfield
- 16 compiler-reserved bits + 16 user-defined bits
- Effect declarations as contracts (not hints)
- See docs/EFFECTS.md and docs/FIBERS.md

### Semantic gaps

- Module system: `import` emits nil
- `higher_order.rs` map/filter/fold don't support closures (only native fns)

## Completed phases

### Phases 0-4: CPS rework (PRs #275-#279)

| Phase | What |
|-------|------|
| 0 | Property tests for coroutines, effect threading fixes |
| 1 | First-class continuations: `ContinuationFrame`, `ContinuationData`, `Value::Continuation` |
| 2 | Deleted CPS interpreter (~4,400 lines), simplified `Coroutine` (7→4 fields) |
| 3 | Exception handler state in continuations, O(1) frame append, edge case tests |
| 4 | Yield as LIR terminator, multi-block functions, `LoadResumeValue` pseudo-instruction |

### Phase B: Hammer time (PR #280)

| Sub | What |
|-----|------|
| B.1 | JIT deletion: removed Cranelift, old compiler, ~12,500 lines |
| B.2 | value_old migration: all types now in `value/` submodules |
| B.3 | LocationMap: source locations flow through entire pipeline |
| B.4 | Thread transfer tests: closures transfer with location data |

### Phase C: Macros and modules (PR #281)

| Sub | What |
|-----|------|
| C.1 | Quasiquote templates: `eval_quasiquote_to_syntax` for direct Syntax tree construction |
| C.2 | Compile-time macro operations: `macro?` and `expand-macro` in Expander |
| C.3 | Module-qualified names: lexer recognizes `module:name`, Expander resolves to flat names |
| C.4 | yield-from delegation: `delegate` field on Coroutine, proper suspension semantics |

### Phase D: Documentation cleanup (PR #282)
Stale docs removed. AGENTS.md files updated. Unused deps removed.

### Interprocedural effects (PRs #283-#284)
Cross-function effect tracking, sound defaults, polymorphic inference.

### File splitting and cleanup (PR #285)
Instruction enum gap fix, 5 critical files split, broken demo renamed.

### JIT Phases 1-3 (PRs #286-#287)
Cranelift scaffold, VM integration, full LirInstr coverage.

### JIT Phase 4 (PR #288, open on `jit-phase-4`)
Feature gate removal, self-tail-call TCO, exception propagation,
cross-form effect tracking, profiling optimizations (Vec globals,
SmallVec handlers, pre-allocated envs, inline jump elimination).

## Decisions

| Decision | Rationale |
|----------|-----------|
| Delete JIT code, not feature-flag it | Git preserves history. Dead code costs. |
| Full first-class continuations | Composable and future-proof. |
| Yield as LIR terminator | Proper control flow; prerequisite for JIT. |
| Single execution path (bytecode) | One thing to optimize. |
| `try`/`catch`/`finally` over fibers | Familiar syntax, full power via signal/resume underneath. |
| Nil ≠ empty list | `nil` falsy, `()` truthy. Lists end with `()`. |
| New pipeline skips Expr | Syntax → HIR directly. No reason to generate Expr. |
| TCO via trampoline | `pending_tail_call` on VM. Works for mutual recursion. |
| JIT always enabled | Cranelift required. No feature gate. |
| Conservative raises tracking | `bool` not `BTreeSet`. Correct first. |
| First-class instant/duration | Mirrors Rust's `std::time`. Type-safe, no cons allocation. |
| Debugging from Elle, not Rust | No recompilation for instrumentation. |

## Known defects

- `handler-bind` is a stub (replaced by fiber model — see docs/FIBERS.md)
- `InvokeRestart` opcode is a no-op (replaced by signal/resume)
- JIT-compiled code bounces to interpreter for non-self calls
- `higher_order.rs` map/filter/fold don't support closures (only native fns)
