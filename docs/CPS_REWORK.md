# CPS Rework: Unified Continuation-Based Execution

This document records the completed work to unify Elle's execution model around
first-class continuations in bytecode/LIR, eliminating the separate CPS
interpreter path.

## Current State (Post-Phase 4)

Elle now has a **single execution path** for all coroutines:

1. **Bytecode VM only**: All code, yielding or not, executes via bytecode
2. **First-class continuations**: `Value::Continuation` holds a chain of
   `ContinuationFrame`s, each capturing bytecode, constants, environment,
   IP, stack, and exception handler state
3. **Frame chain mechanism**: When yield propagates through call boundaries,
   each caller's frame is appended to the continuation chain
4. **Exception handler preservation**: `handler-case` blocks active at yield
   time remain active after resume

The CPS interpreter has been deleted (~4,400 lines removed).

### Key implementation details

- `ContinuationFrame` stores: bytecode, constants, env, ip, stack,
  exception_handlers, handling_exception
- Frame ordering: innermost (yielder) first, outermost (caller) last
- `append_frame` is O(1) (was O(n) with `prepend_frame`)
- `resume_continuation` iterates frames forward, restoring handler state
- Exception check at start of instruction loop handles cross-frame propagation
- Tail calls handled in `execute_bytecode_from_ip_with_state`

## Migration Checklist (Completed)

### Phase 0: Prerequisites
- [x] 0.1: NaN-boxing Value merged
- [x] 0.2: Continuation usage audited
- [x] 0.3: Comprehensive coroutine tests added

### Phase 1: First-class Continuations
- [x] 1.1: `Value::Continuation` defined (`ContinuationData`, `ContinuationFrame`)
- [x] 1.2: Frame chain mechanism in VM (Yield captures frame, Call appends caller frame)
- [x] 1.3: `resume_continuation` replays frame chain
- [x] 1.4: `VmResult::Yielded` carries continuation value

Note: The original plan (1.2-1.5) was superseded by the frame-chain approach.
Instead of explicit `CaptureCont`/`ApplyCont` instructions, continuations are
built incrementally as yields propagate through call boundaries.

### Phase 2: Delete CPS Interpreter
- [x] 2.1: Removed `compiler/cps/` (~4,400 lines)
- [x] 2.2: Simplified `Coroutine` struct (7 fields -> 4)
- [x] 2.3: Single execution path (bytecode only)
- [x] 2.4: Migrated `yielded_value` to new Value type

### Phase 3: Harden Continuations
- [x] 3.1: Exception handler state saved in continuation frames
- [x] 3.2: `ContinuationData` frame ordering optimized (O(1) append)
- [x] 3.3: Edge case tests (handler-case+yield, deep call chains, tail calls)
- [x] 3.4: Documentation updated
- [x] 3.5: Exception check at start of instruction loop (for cross-frame propagation)
- [x] 3.6: Tail call handling in `execute_bytecode_from_ip_with_state`

### Phase 4: LIR Continuation Instructions
- [x] Yield as LIR terminator (`Terminator::Yield { value, resume_label }`)
- [x] `LoadResumeValue` pseudo-instruction for resume blocks
- [x] Emitter carries stack state across yield boundaries
- [x] Multi-block functions for yielding code

## Success Criteria (Met)

1. All coroutine tests pass
2. No CPS interpreter code remains
3. `Closure` has no `source_ast` field
4. Benchmark shows no regression for non-yielding code
5. Documentation is updated
