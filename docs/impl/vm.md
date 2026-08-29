# VM

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

- **`VM`** — owns the current fiber, primitive table, compiler, and
  JIT compiler
- **`Fiber`** — execution context: operand stack, call frames, locals,
  signal state, arena
- **`CallFrame`** — return address, local variable base, function
  metadata
- **`BytecodeFrame`** — points into a `CompiledFunction`'s bytecode
  stream

## Dispatch loop

The main loop in `execute.rs`:

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
`DivInt` handlers skip even that test, but no emitter produces those
bytecodes — see [impl/bytecode.md](bytecode.md) § Arithmetic.

## Fiber integration

- **Emit** (`Instruction::Emit` with a u16 signal bits operand) — saves
  the current frame as a `SuspendedFrame`, returns control to the parent
  fiber or scheduler
- **Signal emission** — checks the fiber's signal mask to decide
  whether to propagate or catch
- **Fuel** — decrements a counter per instruction; when zero, emits
  `:fuel` signal

## Where a reported error's location comes from

An error that reaches the root is printed with one `at file:line:col` line
and a caret under the source. That location is `VM::error_loc`, and it names
**the innermost frame that was running when the error was raised** — the
raising form itself, not any call above it.

The dispatch loop records it. Every path that leaves
`execute_bytecode_inner_impl` carrying `SIG_ERROR` or `SIG_HALT` calls
`VM::record_error_loc`, which maps the current instruction offset through
the frame's `LocationMap`. Recording is first-writer-wins: the raising frame
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
The VM validates tail position at compile time. This guarantees constant
stack space for tail-recursive functions.

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

- **Nested call.** `execute_bytecode_saving_stack` saves the caller's register,
  installs the callee's, runs the body, and restores the caller's on return. The
  callee value crosses the entry through the one-shot `VM::pending_entry_closure`
  (the raw root entry `execute_bytecode` consumes the same one-shot). **Every
  entrant that runs a closure body sets it** immediately before entering: the
  interpreter call path, the JIT helpers' interpreter fallback and tail-call
  resolution, the forced-tier entries (`compile/run-on`), the fiber's first
  resume, the measured-thunk entry (`arena/allocs`), the macro-transformer call,
  the FFI callback trampoline, the WASM host's bytecode fallback, and the spawned
  worker's body. A `NIL` (untracked) entry is reserved for a body that is not a
  closure instance — the top-level program, a module body, an eval'd form — whose
  bytecode can contain no self-reference. `LoadSelf` debug-asserts the register
  is populated, so an unthreaded entrant fails loudly at the read instead of
  resolving a self-reference to `NIL`.
- **Tail call.** `trampoline_loop` reuses the frame in place but installs the
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
every closure entry and carries across suspend/resume (impl/wasm.md).

## JIT fallback

When a function is JIT-compiled, `Call` dispatches to the native code
pointer instead of interpreting bytecode. If the JIT rejects a function
(e.g., due to yields), the VM falls back to bytecode interpretation.

## Files

```text
src/vm/core.rs        VM struct and initialization
src/vm/execute.rs     Main dispatch loop
src/vm/dispatch.rs    Opcode handlers
src/vm/call.rs        Call/return mechanics
src/vm/fiber.rs       Fiber state management
```

---

## See also

- [impl/bytecode.md](bytecode.md) — instruction set
- [impl/jit.md](jit.md) — JIT compilation
- [impl/mlir.md](mlir.md) — MLIR/LLVM tier-2 path
- [impl/wasm.md](wasm.md) — WebAssembly backend
- [impl/gpu.md](gpu.md) — GPU compute pipeline
- [impl/values.md](values.md) — Value representation
