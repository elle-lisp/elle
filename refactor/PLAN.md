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

### Step 1: Introspection primitives (~2-3 hours)

Add primitives that inspect closure properties from Elle code.

**New file**: `src/primitives/introspect.rs`
- `closure?` — `value.as_closure().is_some()`
- `jit?` — closure with `jit_code.is_some()`
- `pure?` — closure with `effect == Effect::Pure`
- `coro?` — closure with `effect == Effect::Yields`
- `mutable?` — closure with `cell_params_mask != 0`
- `arity` — closure arity as int, pair, or nil
- `captures` — number of captures as int, or nil
- `bytecode-size` — bytecode length as int, or nil

**New file**: `src/primitives/jit_ops.rs` (VmAwareFn)
- `global?` — check if symbol is bound as global
- `jit` — trigger JIT compilation of a closure value, return it
- `jit!` — trigger JIT compilation of a global by symbol, mutate in place
- `call-count` — read VM's closure_call_counts for a closure

**Modify**: `src/primitives/registration.rs` — register all new primitives
**Modify**: `src/primitives/mod.rs` — add module declarations
**Modify**: `src/effects/primitives.rs` — add Pure annotations
**Modify**: `src/primitives/debug.rs` — remove placeholder `profile`

**Tests**: Unit tests in `tests/unittests/primitives.rs`. Integration tests
in new `tests/integration/debugging.rs`.

**Verification**: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`

### Step 2: Clock API (~2-3 hours)

POSIX clock primitives. All return `(seconds . nanoseconds)` cons pairs.

**New file**: `src/primitives/clock.rs`
- `clock-realtime` — `CLOCK_REALTIME`
- `clock-monotonic` — `CLOCK_MONOTONIC`
- `clock-process` — `CLOCK_PROCESS_CPUTIME_ID`
- `clock-thread` — `CLOCK_THREAD_CPUTIME_ID`
- `clock-resolution` — `clock_getres` with keyword argument
- `clock-nanosleep` — `clock_nanosleep` relative to MONOTONIC
- `timespec-diff` — subtract two timespecs with borrow
- `timespec->float` — convert to float seconds
- `timespec->ns` — convert to nanoseconds integer

**Dependency**: Add `libc` as direct dependency in `Cargo.toml` (already
transitive via cranelift, but make it explicit).

**Modify**: `src/primitives/registration.rs`, `mod.rs`, `effects/primitives.rs`

**Tests**: Unit tests for arithmetic, integration tests for clock reads.

**Verification**: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`

### Step 3: Raises effect tracking (~3-4 hours)

Extend the effect system with a boolean `may_raise` field.

**Modify**: `src/effects/mod.rs`
- Add `may_raise: bool` field to `Effect` (or as a parallel field on `Hir`)
- Update `combine` to OR the raises flags
- Update `Display`

**Modify**: `src/hir/analyze/special.rs`
- `analyze_throw`: set `may_raise = true` on the Hir node
- `analyze_handler_case`: clear `may_raise` only when handler catches
  `condition` (ID 1, the root type)

**Modify**: `src/hir/analyze/call.rs`
- Propagate callee's `may_raise` into call expression

**Modify**: `src/hir/analyze/binding.rs`
- Store `may_raise` in `effect_env` alongside yield effects

**Modify**: `src/value/closure.rs`
- Add `may_raise: bool` to `Closure`

**Modify**: `src/lir/emit.rs`
- Emit `may_raise` on closures

**Modify**: `src/pipeline.rs`
- Track `may_raise` during fixpoint iteration

**New primitive**: `raises?` in `src/primitives/introspect.rs`

**Tests**: Integration tests in `tests/integration/effect_enforcement.rs`
(extend existing) and `tests/integration/debugging.rs`.

**Verification**: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`

### Step 4: Benchmarking macros and example (~1-2 hours)

Elle-level benchmarking built on the clock primitives.

**New file**: `lib/bench.lisp`
- `bench` macro — time a single expression
- `bench-n` macro — time N iterations
- `bench-compare` macro — compare two expressions
- `assert-faster` macro — assertion that one is faster

**New file**: `examples/debugging.lisp` — replaces `debugging-profiling.lisp`
- Demonstrates all introspection primitives
- Demonstrates clock API
- Demonstrates benchmarking macros
- Uses assertions to verify results

**Tests**: `tests/integration/benchmarks.rs`

**Verification**: `cargo test --workspace`, example execution via
`cargo run -- examples/debugging.lisp`

### Step 5: Documentation and cleanup (~1 hour)

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
| All clocks return pairs | `(seconds . nanoseconds)`. Consistent, no overflow. |
| Debugging from Elle, not Rust | No recompilation for instrumentation. |

## Known defects

- `handler-bind` is a stub (parsed, codegen ignores handlers)
- `InvokeRestart` opcode allocated but VM handler is no-op
- `signal`/`warn`/`error` are constructors, not signaling primitives
- JIT-compiled code bounces to interpreter for non-self calls
- `higher_order.rs` map/filter/fold don't support closures (only native fns)
