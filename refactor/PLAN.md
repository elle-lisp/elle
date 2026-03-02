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
- `higher_order.rs` map/filter/fold don't support closures (only native fns) — see issue

## Current work: Hammer Time II

Global cleanup, simplification, and file size reduction. See analysis below.

### File size targets

| Category | Target | Rationale |
|----------|--------|-----------|
| Dispatch tables (match-heavy) | 800 lines | Splitting destroys locality |
| Pipeline/analyzer modules | 500 lines | Complex but cohesive logic |
| Primitive collections | 400 lines | Independent functions, split easily |
| Everything else | 500 lines | Comfortable LLM read window |
| Test files | No limit | Append-only, rarely clobbered |

Previous target was 300 lines. That was too aggressive for a compiler — 66
of 130 source files violated it. The new targets reflect the natural grain
of the code: dispatch tables need to stay together, primitives don't.

### Phase H.1: Fix production unwraps (standalone PR)

**Risk: low. Value: high. Scope: small.**

7 production `unwrap()` calls violate the invariant "primitives validate
arguments, never panic." These are all validate-then-unwrap patterns where
validation and use are separated — safe today but fragile if the validation
logic changes.

| File | Lines | Pattern |
|------|-------|---------|
| `primitives/bitwise.rs` | 33,35,65,67,97,99 | `as_int().unwrap()` after validation loop |
| `primitives/string.rs` | 194 | `chars().next().unwrap()` after count check |

Fix: combine validation and use. For bitwise ops, fold with
`as_int().ok_or(...)` or match directly. For string.rs, use `if let`.

### Phase H.2: Test infrastructure (standalone PR)

**Risk: low-medium. Value: high. Scope: medium.**

The same `eval()` helper is copy-pasted in 20+ test files with at least 3
semantic variants. The `setup()` helper is duplicated in 7+ files.

1. Create `tests/common/mod.rs` with canonical `eval()` and `setup()`
2. The canonical `eval()` calls `set_symbol_table` (harmless when not needed)
3. JIT test variants stay separate (different pipeline entry point)
4. Replace all 20+ duplicated helpers with `use` from common module

Also: extract `pipeline.rs` inline tests (~1,135 lines) to
`tests/integration/pipeline.rs`. All 88 tests use only `pub` API — verified
that neither `scan_define_lambda` nor `scan_const_binding` (private) are
referenced. This reduces `pipeline.rs` from 1,532 to ~400 lines.

**Caveat:** `tests/lib.rs` uses `include!()` macros. The common module must
work with this structure.

### Phase H.3: Re-export cleanup (standalone PR)

**Risk: low. Value: moderate. Scope: small.**

| Item | Action | Verified |
|------|--------|----------|
| `compiler/mod.rs` re-exports `symbols::{SymbolDef, SymbolIndex, SymbolKind}` | Remove — 0 uses found | workspace-wide grep |
| `vm/core.rs` re-exports `CallFrame as FiberCallFrame` | Remove — 0 uses found | workspace-wide grep |
| `error/sourceloc.rs` (6-line re-export shim) | Remove, update 1 user (`primitives/concurrency.rs` line 166) to import from `reader` | workspace-wide grep |
| `value/heap.rs` re-exports `Arity` from sibling `types` | Remove — 0 uses via `heap::Arity` | grep confirmed |
| `value/heap.rs` re-exports `Closure`, `NativeFn`, `TableKey` | Keep — used by `repr/constructors.rs`, `repr/accessors.rs`, `vm/signal.rs`, `value/send.rs` via `heap::` paths | Or update 4 files to use `value::` paths and remove all heap re-exports |
| `elle-lint` dependency in `elle-lsp/Cargo.toml` | Remove — unused at Rust level | verify with `cargo build -p elle-lsp` |

### Phase H.4: Primitive file splits (standalone PR)

**Risk: low. Value: high. Scope: medium.**

Split the three largest primitive files. Functions within each are
independent — no shared state, no ordering constraints.

| File | Lines | Split into |
|------|-------|------------|
| `primitives/string.rs` | 1,219 | `string.rs` (core: length, append, substring, char ops) + `string_convert.rs` (to-string, format, replace, split, join) |
| `primitives/fileio.rs` | 1,047 | `file_read.rs` (read-file, read-lines, file-exists?, stat) + `file_write.rs` (write-file, delete, rename, directory ops) |
| `primitives/fibers.rs` | 890 | `fibers.rs` (fiber/new, resume, signal, status, value) + `fiber_ops.rs` (mask, parent, child, propagate, cancel, bits) |

Also extract `register_builtin_docs()` (333 lines) from `registration.rs`
to `builtin_docs.rs`. This documents special forms and prelude macros — it
is NOT derivable from `PRIMITIVES` tables and must be preserved. Extraction
reduces `registration.rs` from 616 to ~280 lines.

### Phase H.5: Analyzer binding split (standalone PR)

**Risk: medium. Value: moderate. Scope: medium.**

`hir/analyze/binding.rs` (923 lines) handles all binding forms. Natural
split by form:

| New file | Content | ~Lines |
|----------|---------|--------|
| `binding.rs` | `let`, `let*`, `letrec` | ~300 |
| `define.rs` | `def`, `var`, `set!` | ~300 |
| `destructure.rs` | destructuring patterns in bindings | ~300 |

### NOT doing

| Candidate | Why not |
|-----------|---------|
| Split `lir/emit.rs` into files | 459-line match is a flat dispatch — splitting destroys locality. Extract helpers within the file instead. |
| Split `jit/translate.rs` into files | Same reason — dispatch table. |
| Break `pipeline <-> primitives` cycle | Logical cycle only, already broken by raw pointer indirection. The real problem is `get_vm_context()` unsafe global state — that's a bigger redesign. |
| `primitives/prelude.rs` for shared imports | 13 files share 5-6 import lines. Saves ~65 lines total. Not worth the indirection. |
| File size limit on test files | Tests are append-only and rarely clobbered by agents. No ROI in splitting them. |

### Execution order

1. **H.1** — unwrap fixes (smallest, safest, immediate correctness win)
2. **H.3** — re-export cleanup (quick, no logic changes)
3. **H.4** — primitive splits (mechanical, high line-count reduction)
4. **H.2** — test infrastructure (highest value but needs care with `include!()`)
5. **H.5** — analyzer binding split (most risk, do last)

Each phase is a standalone PR. Run full test suite between phases.

## Future work

### Fiber/Signal System (see docs/fibers.md)

The next major milestone unifies exception handling, coroutines, and effect
inference into a single fiber/signal mechanism. Surface syntax: `try`/`catch`/
`defer`. See `docs/fibers.md` for the implementation plan and
`docs/effects.md` for the design rationale.

### JIT Phase 5: Intra-JIT calling

Make `elle_jit_call` and `elle_jit_tail_call` check the JIT cache and call
JIT code directly instead of always bouncing to the interpreter. This is the
single highest-impact optimization remaining for nqueens-class benchmarks.

### JIT Phase 6: Optimization

- Inline type checks for arithmetic fast paths
- JIT-native exception handling
- Arena allocation for hot-path allocations

### Effect system: fiber/signal migration

- Replace `Effect { yield_behavior, may_raise }` with `SignalBits` bitfield
- 16 compiler-reserved bits + 16 user-defined bits
- Effect declarations as contracts (not hints)
- See docs/effects.md and docs/fibers.md

### Semantic gaps

- Module system: `import` emits nil
- `higher_order.rs` map/filter/fold don't support closures (only native fns) — see issue

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
| B.1 | Old JIT deletion: removed previous Cranelift code, old compiler, ~12,500 lines |
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
| Full first-class continuations | Composable and future-proof. |
| Yield as LIR terminator | Proper control flow; prerequisite for JIT. |
| Single execution path (bytecode) | One thing to optimize. |
| `try`/`catch` + `defer` over fibers | Familiar syntax, full power via signal/resume underneath. |
| Nil ≠ empty list | `nil` falsy, `()` truthy. Lists end with `()`. |
| New pipeline skips Expr | Syntax → HIR directly. No reason to generate Expr. |
| TCO via trampoline | `pending_tail_call` on VM. Works for mutual recursion. |
| JIT always enabled | Cranelift required. No feature gate. |
| Conservative raises tracking | `bool` not `BTreeSet`. Correct first. |
| Debugging from Elle, not Rust | No recompilation for instrumentation. |
| 500-line file target, not 300 | 300 too aggressive for a compiler. 66/130 files violated it. |

## Known defects

- JIT-compiled code bounces to interpreter for non-self calls
- `higher_order.rs` map/filter/fold don't support closures (only native fns) — see issue
- 7 production `unwrap()` calls in bitwise.rs and string.rs (Phase H.1)

## Dead code to remove

- `handler-bind` stub — will never be implemented (fiber/signal model replaces it)
- `InvokeRestart` opcode — no-op, replaced by signal/resume
