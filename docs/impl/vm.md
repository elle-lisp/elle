# VM

<!-- audited: 2026-09-22 -->

The VM is a stack-machine interpreter that executes bytecode.

## Architecture

```text
┌──────────────┐
│    Fiber     │ ← execution context
│  ┌────────┐  │
│  │ Stack  │  │ ← operand stack (Values)
│  │ Frames │  │ ← call frame stack
│  │ Locals │  │ ← register-allocated locals
│  └────────┘  │
└──────────────┘
```

## Key types

- **`VM`** — owns the current fiber, the JIT cache, and the runtime
  configuration. It points at the heap, the compile context, and the symbol
  table that its `RuntimeCore` owns.
- **`Fiber`** — execution context: operand stack, paused callers, trace
  frames, region-remap frames, signal state
- **`CallFrame`** — a stack-trace entry: the entered and calling code
  objects, the offset of the call, and the frame base
- **`PausedCaller`** — a caller activation that waits in the fiber while its
  callee runs (see "Non-tail calls" below)
- **`BytecodeFrame`** — a parked execution point in a suspended fiber's replay
  chain

## Dispatch loop

The main loop is `execute_bytecode_inner_impl` in
[interp.rs](../../src/vm/dispatch/interp.rs):

1. Read opcode byte
2. Decode operands
3. Pop operands from stack
4. Perform operation
5. Push result
6. Advance instruction pointer

The loop dispatches one instruction at a time; no fused sequences exist.
Specialization lives inside the handlers instead. The polymorphic
arithmetic ops test their two operands for integers and take a wrapping
integer path before falling back to the general one
(`src/vm/arithmetic.rs`). The integer-only `AddInt`/`SubInt`/`MulInt`/
`DivInt` handlers skip even that test, and the emitter produces them where
the compiler proved both operands are integers — see
[impl/bytecode.md](bytecode.md).

## Fiber integration

- **Emit** (`Instruction::Emit` with a signal bits operand — see
  [impl/bytecode.md](bytecode.md) § "Signal-bits operands") — saves
  the current frame as a `SuspendedFrame`, returns control to the parent
  fiber or scheduler
- **Signal emission** — checks the fiber's signal mask to decide
  whether to propagate or catch
- **Fuel** — decrements a counter at each backward jump and each call; when
  zero, emits the `:fuel` signal

## Where a reported error's location comes from

An error that reaches the root is printed with one `at file:line:col` line
and a caret under the source. That location is `VM::error_loc`, and it names
**the innermost frame that was running when the error was raised** — the
raising form itself, not any call above it.

The dispatch loop records it. Every path that leaves
`execute_bytecode_inner_impl` carrying `SIG_ERROR` or `SIG_HALT` calls
`VM::record_error_loc`, which maps the current instruction offset through
the frame's `LocationMap`. A paused caller that the error leaves records the
offset of its call instruction the same way. Recording is first-writer-wins: the raising frame
reaches its exit path first, and each frame the error then unwinds through
finds the slot already taken, so the innermost location is the one that
survives to the root. A frame whose `LocationMap` has no entry for the
instruction leaves the slot empty for an outer frame to fill.

First-writer-wins needs an end, because the record answers only for the error
currently propagating. A fiber mask that absorbs `SIG_ERROR` ends that
propagation — `try`, `protect`, and `defer` all catch that way — so
`VM::absorbs` takes the live record at the moment it reports the catch. Every
position that drives a child fiber asks `absorbs`, so a later error finds an
empty slot and records its own location.

The record is parked, not dropped. `absorbs` moves it onto the caught fiber
(`Fiber::error_loc`), paired with the payload it describes. An error that is
caught and then sent on again — what `defer` does, and what `ev/run`'s
scheduler does with a failed thunk — keeps its raising form that way.
`fiber/propagate` re-raises the fiber's parked signal and takes the parked
location back, but only while the pair still names the payload being
re-raised. Both stdlib sites that surface a failed fiber's error therefore
use `(fiber/propagate f)`; raising the payload afresh with `(error
(fiber/value f))` would report the stdlib line that re-raised it.

## Tail calls

`TailCall` reuses the current call frame rather than pushing a new one.
The compiler decides which calls are in tail position. A tail call therefore
runs in constant space, in the interpreter and in compiled code.

## Non-tail calls

A non-tail call to an interpreted closure does not grow the Rust stack. The
caller's activation waits in the fiber, and the same dispatch loop runs the
callee. Recursion depth is therefore bounded by memory and by the depth cap
below, not by the thread's stack.

### How a call enters its callee

`call_inner` checks the callee, builds its environment, and pushes the
stack-trace frame. It then hands the callee to the loop as `VM::pending_call`
and exits dispatch, the way a tail call exits with `pending_tail_call`.

`VM::run_dispatch` takes the pending call. It moves the caller's operand stack,
resume offset and executing-closure register into a `PausedCaller` on
`Fiber::callers`. Then it opens the callee's activation — a region-remap frame
and a dues slot — and dispatches the callee from offset 0. A tail call inside
the callee replaces the callee's activation in place, as it does anywhere else.

A spliced call (`CallArrayMut`) releases its argument array as soon as the
callee's environment holds every argument, before the callee runs. Nothing
reads the array after that point.

### How a callee returns

When the callee's activation ends, `run_dispatch` pops its `PausedCaller`,
restores the caller's stack and register, and completes the call:

- **Return** — the result goes on the caller's stack, and the caller continues
  at its resume offset.
- **Suspend** — the caller parks behind the callee, and the caller's own
  activation leaves by the same signal. Every paused caller parks in turn, so
  `Fiber::suspended` holds the innermost frame first, as `resume_suspended`
  expects.
- **Error or halt** — the caller leaves by the same signal from the call
  instruction. The abandoned-frame walk runs for each frame on the way out.

`execute_code` (the root) and `trampoline_loop` (every other entry) call
`run_dispatch` where they would call the dispatch loop itself. The callers that
one `run_dispatch` pauses sit above the ones it found on `Fiber::callers`, and
all of them are gone when it returns. A suspended fiber therefore holds no
paused callers, only its parked chain, and a parked chain has the same shape
whatever depth it was built at.

### What still uses the Rust stack

Two kinds of call still nest on the Rust stack:

- **Re-entry** — a primitive that calls a closure: a trait method, `eval`,
  `arena/allocs`, a macro transformer, an FFI callback. Each one enters through
  `execute_bytecode_saving_stack`.
- **Compiled code** — the interpreter calls a JIT, WASM or MLIR callee as a
  native function, and a compiled caller calls a compiled callee the same way.

[native_stack.rs](../../src/vm/native_stack.rs) measures what is left of the
thread's stack. While less than 512 KiB remains, a call does not enter
compiled code: the interpreter runs the callee on fiber frames instead, so a
deep recursion that started compiled continues interpreted
([jit.md](jit.md)). While less than 256 KiB remains,
`execute_bytecode_saving_stack` refuses the re-entry and halts with
`:stack-overflow`, rather than letting the thread overflow.

### The depth cap

`Fiber::call_depth` counts the non-tail closure calls in progress, on every
tier. A call past `(vm/config :max-depth)` — 10,000,000 by default — halts
with `:stack-overflow`. A halt passes every signal mask, so `protect` does not
catch it. The cap stops a runaway recursion before it takes the machine's
memory: each paused caller costs a few hundred bytes
([config.md](../config.md)).

## The executing-closure register

`Fiber::current_closure` names the closure whose body is currently executing.
It is an **uncounted borrow** — a pure runtime register, not a heap object — and
it is the identity a self-reference resolves to. An activation can outlive its
closure's heap value (the region solver frees the value at its last use while
the body's `code`/`env` live on as `Rc`s), so the register may hold a dead value
for a body that never reads it. It is guaranteed live exactly where it is read:
`LoadSelf` occurs only in a self-recursive body, whose closure region outlives
the recursion (the tail-call deferred release releases it on the recursion's completion —
[selfrec.md](selfrec.md)). No other site may dereference it.

It is per-activation and threaded across every control-flow boundary, mirroring
`activation_region_map` exactly:

- **Nested call.** An interpreted non-tail call parks the caller's register in
  its `PausedCaller`, installs the callee named by `VM::pending_call`, and
  restores the caller's register when the callee returns.
- **Re-entry.** `execute_bytecode_saving_stack` saves the caller's register,
  installs the callee's, runs the body, and restores the caller's on return. The
  callee value crosses the entry through the one-shot `VM::pending_entry_closure`
  (the raw root entry `execute_code` consumes the same one-shot). **Every
  entrant that runs a closure body sets it** immediately before entering: the
  JIT helpers' interpreter fallback and tail-call
  resolution, the forced-tier entries (`compile/run-on`), the fiber's first
  resume, the measured-thunk entry (`arena/allocs`), the macro-transformer call,
  the FFI callback trampoline, the WASM host's bytecode fallback, and the spawned
  worker's body. A `NIL` (untracked) entry is reserved for a body that is not a
  closure instance — the top-level program, a module body, an eval'd form — whose
  bytecode can contain no self-reference. `LoadSelf` debug-asserts the register
  is populated, so an unthreaded entrant fails loudly at the read instead of
  resolving a self-reference to `NIL`.
- **Tail call.** The trampoline reuses the frame in place but installs the
  tail callee as the register on each replacement (a self-recursive `loop`
  re-installs itself; a tail call to a sibling installs the sibling).
- **Suspend/resume.** A yield parks the register in the `BytecodeFrame`
  (alongside its `activation_region_map`); `resume_suspended` re-installs it
  before re-entering the body.
- **Fiber swap.** The register lives on the `Fiber`, so it rides a fiber swap
  with the fiber — never a VM-global slot read across a switch.

A `#[cfg(debug_assertions)]` invariant at each **body-entry install**
(`VM::debug_assert_entry_closure_matches`) checks that the closure being handed
in is the body being entered — its template bytecode is the very `Rc` the
entered `Code` carries. It runs only where the closure is live by construction
(the entrant just took `code` from it): the one-shot consumes and the tail-call
installs. It is deliberately NOT checked at dispatch entry or on a restored
parked frame — a parked register is a possibly-dead borrow, and dereferencing
it there is unsound.

### Self-references: value path and call re-dispatch

A reference to a lambda's own self-recursive binding lowers to `LoadSelf`, which
yields the executing-closure register — in **both** value and call position (the
lowerer routes them identically; `lir/lower/expr.rs`):

- **Value position** (`go` returned, stored, or passed to a higher-order call)
  materializes the closure and uses it as a value.
- **Call position** (`(go …)`) uses it as the callee, so the call re-enters the
  current `code`+`env` with new args — a self-call re-dispatch that names no
  forward cell.

The one op serves every tier: the interpreter reads `current_closure`; the JIT
reads the `self_tag_payload` compiled-body parameter, and its self-tail-call
optimization re-enters the same compiled body directly when the callee is itself;
the WASM backend reads a reserved linear-memory self slot the host installs at
every closure entry and carries across suspend/resume ([wasm.md](wasm.md)).

## JIT fallback

When a function is JIT-compiled, `Call` dispatches to the native code
pointer instead of interpreting bytecode. The VM interprets a function the
JIT rejected — a polymorphic one, or one that uses an instruction the
translator lacks — and any call made while the native stack is low.

## Files

```text
src/vm/core.rs              VM struct and accessors
src/vm/execute.rs           entry points and the tail-call trampoline
src/vm/execute/nested.rs    run_dispatch: paused callers and their returns
src/vm/native_stack.rs      what is left of the thread's stack
src/vm/dispatch/interp.rs   the dispatch loop
src/vm/call/inner.rs        Call-position dispatch by callee kind
src/vm/core/resume.rs       replaying a suspended frame chain
src/value/fiber.rs          the Fiber and its frame types
```

---

## See also

- [impl/bytecode.md](bytecode.md) — instruction set
- [impl/jit.md](jit.md) — JIT compilation
- [impl/mlir.md](mlir.md) — MLIR/LLVM tier-2 path
- [impl/wasm.md](wasm.md) — WebAssembly backend
- [impl/gpu.md](gpu.md) — GPU compute pipeline
- [impl/values.md](values.md) — Value representation
