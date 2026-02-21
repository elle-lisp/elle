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

- Full pipeline with property tests (~1,130+ tests, zero ignored)
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
- No debugging/introspection primitives from Elle code
- No timing/clock primitives
- `profile` primitive is a placeholder
- No `raises` tracking in the effect system
- `handler-bind` is a stub
- `signal`/`warn`/`error` are constructors, not signaling primitives

## Current work: Debugging Toolkit (PR TBD)

See `docs/DEBUGGING.md` for the full design. Implementation steps below.

### Step 1: Value types and debugging primitives (~4-5 hours)

Add `Instant` and `Duration` heap types, then implement all debugging
toolkit primitives in a single file.

**Modify**: `src/value/heap.rs`
- Add `HeapObject::Instant(std::time::Instant)` variant
- Add `HeapObject::Duration(std::time::Duration)` variant
- Update `HeapTag`, `tag()`, `type_name()`, `Debug`

**Modify**: `src/value/repr/constructors.rs`, `accessors.rs`, `traits.rs`
- Add `Value::instant()`, `Value::duration()` constructors
- Add `as_instant()`, `as_duration()`, `is_instant()`, `is_duration()`
- Add `PartialEq` arms for Instant and Duration

**Modify**: `src/value/display.rs`
- Add display formatting for `#<instant>` and `#<duration ...>`

**Modify**: `src/value/send.rs`
- Add `SendValue` handling for Instant and Duration

**New file**: `src/primitives/debugging.rs`
All debugging toolkit primitives in one file:
- Introspection: `closure?`, `jit?`, `pure?`, `coro?`, `mutates-params?`,
  `arity`, `captures`, `bytecode-size`, `raises?`
- Time: `now`, `elapsed`, `cpu-time`, `duration`, `duration->seconds`,
  `duration->nanoseconds`, `duration<`, `instant?`, `duration?`
- JIT control (VmAwareFn): `global?`, `jit`, `jit!`, `call-count`

**Modify**: `src/primitives/debug.rs` — remove placeholder `profile`
**Modify**: `src/primitives/concurrency.rs` — `sleep` accepts duration only
**Modify**: `src/primitives/mod.rs` — add `debugging` module

**Dependency**: Add `cpu-time` to `Cargo.toml`.

**Tests**: Unit tests in `tests/unittests/primitives.rs`. Integration tests
in new `tests/integration/debugging.rs`.

**Verification**: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`

### Step 2: Raises effect tracking and registration unification (~4-5 hours)

Extend the effect system with `may_raise`, then unify effect declaration
into primitive registration (eliminating the separate side-table).

**Modify**: `src/effects/mod.rs`
- Restructure `Effect` as struct: `YieldBehavior` enum + `may_raise: bool`
- Add convenience constructors: `pure()`, `pure_raises()`, `yields()`,
  `yields_raises()`, `polymorphic(n)`, `polymorphic_raises(n)`
- Update `combine` to OR `may_raise` alongside yield combination
- Update `Display`, `Hash`, `Eq`, `Default`

**Modify**: `src/primitives/registration.rs`
- Add `Effect` parameter to `register_fn` and `register_vm_aware_fn`
- Migrate all existing primitive registrations with correct effects
- Register all new debugging toolkit primitives with effects
- Default: `Effect::pure_raises()` (conservative), then tighten where
  appropriate (type predicates, boolean ops, constants → `Effect::pure()`)

**Delete**: `src/effects/primitives.rs`
- Side-table replaced by registration-time effect declarations
- `register_primitive_effects` and `get_primitive_effects` removed
- Analyzer reads effects from the registration-populated map

**Modify**: `src/hir/analyze/special.rs`
- `analyze_throw`: set `may_raise = true` on the Hir node
- `analyze_handler_case`: clear `may_raise` only when handler catches
  `condition` (ID 1, the root type)

**Modify**: `src/hir/analyze/call.rs`
- Propagate callee's `may_raise` into call expression
- For primitive calls, use the registered `Effect` (not a hardcoded default)

**Modify**: `src/hir/analyze/binding.rs`
- Store `may_raise` in `effect_env` alongside yield effects

**Modify**: `src/value/closure.rs`
- Add `may_raise: bool` to `Closure`

**Modify**: `src/lir/emit.rs`
- Emit `may_raise` on closures

**Modify**: `src/pipeline.rs`
- Track `may_raise` during fixpoint iteration
- Update primitive effects map construction (now from registration, not side-table)

**Tests**: Integration tests in `tests/integration/effect_enforcement.rs`
(extend existing) and `tests/integration/debugging.rs`.

**Verification**: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`

### Step 3: Benchmarking macros and example (~1-2 hours)

Elle-level benchmarking built on the time primitives.

**New file**: `lib/bench.lisp`
- `bench` macro — time a single expression
- `bench-n` macro — time N iterations
- `bench-compare` macro — compare two expressions

**New file**: `examples/debugging.lisp` — replaces `debugging-profiling.lisp`
- Demonstrates all introspection primitives
- Demonstrates time API
- Demonstrates benchmarking macros
- Uses assertions to verify results

**Tests**: `tests/integration/benchmarks.rs`

**Verification**: `cargo test --workspace`, example execution via
`cargo run -- examples/debugging.lisp`

### Step 4: Documentation and cleanup (~1 hour)

- Update `docs/BUILTINS.md` with new primitives
- Update `src/primitives/AGENTS.md` with new modules
- Update `src/effects/AGENTS.md` with raises tracking
- Update root `AGENTS.md` if needed
- Remove `examples/debugging-profiling.lisp` (replaced)
- Update `refactor/SUMMARY.md`

**Verification**: Full CI check: `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt -- --check`,
`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`

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

### Effect system extensions

- Specific exception type tracking (`BTreeSet<u32>`)
- Primitive raises annotations (`/` raises division-by-zero, etc.)
- `handler-case` subtype subtraction

### Semantic gaps

- `handler-bind` (non-unwinding handlers): stub
- Signal/restart system: `InvokeRestart` opcode is a no-op
- Module system: `import` emits nil

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
| `handler-case` not try/catch | Condition system is the mechanism. |
| Nil ≠ empty list | `nil` falsy, `()` truthy. Lists end with `()`. |
| New pipeline skips Expr | Syntax → HIR directly. No reason to generate Expr. |
| TCO via trampoline | `pending_tail_call` on VM. Works for mutual recursion. |
| JIT always enabled | Cranelift required. No feature gate. |
| Conservative raises tracking | `bool` not `BTreeSet`. Correct first. |
| First-class instant/duration | Mirrors Rust's `std::time`. Type-safe, no cons allocation. |
| Debugging from Elle, not Rust | No recompilation for instrumentation. |

## Known defects

- `handler-bind` is a stub (parsed, codegen ignores handlers)
- `InvokeRestart` opcode allocated but VM handler is no-op
- `signal`/`warn`/`error` are constructors, not signaling primitives
- JIT-compiled code bounces to interpreter for non-self calls
- `higher_order.rs` map/filter/fold don't support closures (only native fns)
