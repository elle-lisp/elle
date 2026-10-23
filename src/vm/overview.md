# VM

<!-- audited: 2026-10-22 -->

The VM executes bytecode on a fiber's operand stack, with each local in a stack slot above the frame base.

[The VM design](../../docs/impl/vm.md) holds the dispatch loop, non-tail
calls, and where a reported error's location comes from. This document holds
the module's internals: how a signal leaves the loop, how a code object is
threaded through it, and who re-enters it.

The VM manages the operand stack, the call frames, the closure environments
and the fiber state. It does not compile and it defines no primitive. A `VM`
holds per-instance state and the running fiber, `vm.fiber`, which a resume
swaps. Each field states its own purpose in [core.rs](core.rs), and each
`Fiber` field in [fiber.rs](../value/fiber.rs).

## Data flow

```text
A compiled unit (CodeUnit)
    │
    ▼
execute()        ← public API: copies a unit compiled on another heap,
    │              then runs its entry header
    ▼
execute_code()   ← tail-call and SIG_SWITCH trampolines, returns Result<Value, String>
    │
    ▼
run_dispatch()   ← pauses a caller on fiber.callers and runs its non-tail callee
    │
    ▼
execute_bytecode_inner_impl() → (SignalBits, usize)
    │
    ├─► fetch instruction
    ├─► dispatch by opcode
    ├─► modify stack/locals
    └─► loop until Return, Emit, a call or tail-call hand-off,
        an error or halt, or fuel runs out
```

## Signal-based returns

Internal VM methods return `SignalBits`. The dispatch loop exits only on a
signal, and the drivers above it act on each one:

- `SIG_OK`: normal completion. The value is in `fiber.signal`.
- `SIG_ERROR`: the error struct is in `fiber.signal`.
- `SIG_YIELD`: a fiber yield. The suspended frames are in `fiber.suspended`.
- `SIG_RESUME`: a fiber primitive requests a VM-side context switch.
- `SIG_SWITCH`: a fiber resumed inside a fiber; the trampoline runs the child.
- `SIG_PROPAGATE`: `fiber/propagate` re-signals a caught signal.
- `SIG_ABORT`: inject an error at the target fiber's suspension point.
- `SIG_QUERY`: a primitive reads VM state (arena stats, introspection).
- `SIG_FUEL`: the instruction budget ran out. A silent function spends fuel
  but never pauses ([inference.md](../../docs/signals/inference.md)). A spent
  expansion budget halts instead ([runtime.md](../../docs/runtime.md)).
- `SIG_HALT`: graceful VM termination, not resumable.

`execute_code`, which `execute` calls, is the translation boundary. It
converts `SignalBits` to `Result<Value, String>` for external callers. On
`SIG_ERROR`, it extracts the error struct from `fiber.signal` and formats the
error message.

Most instruction handlers return `()`. The call, tail-call and emit handlers
return the signal that ends the dispatch loop. A VM bug panics. A user error
in call position calls `VM::set_error(kind, msg)` and pushes `Value::NIL` to
keep the stack consistent (invariant 7 below); one in tail position returns
`Some(SIG_ERROR)`.

## Threading the code object

Bytecode, constants and the location table are threaded through the dispatch
loop as one `Code`: the code object itself, one payload slice
([template.md](../../docs/impl/region/template.md)). Individual instruction
handlers take slices (`&[u8]`, `&[Value]`) read off it. The dispatch loop, the
call, emit and closure handlers, and the signal handlers they call take the
`Code`. They copy it (one word) when they build a `SuspendedFrame`,
`TailCallInfo` or `PendingCall`.

- `execute` takes the unit's entry header at the public boundary.
- `execute_bytecode_from_ip` and `execute_bytecode_saving_stack` take a
  `&Code`.
- `TailCallInfo` carries the tail callee's `Code`, its env `Rc`, the callee
  closure value (installed as `fiber.current_closure` on the frame
  replacement), and its boundary, its squelch mask and whether its function
  is silent. A tail call clones the `Rc`s, not the `Vec`s. The releases the
  call strands are not carried here: `tail_call_inner` records them on the
  activation's own `ActivationDues`, which outlives whoever consumes the
  pending call ([owner.md](../../docs/impl/region/owner.md)).
- `PendingCall` carries a non-tail callee's `Code`, env `Rc` and closure value
  from `call_inner` to `run_dispatch`, which pauses the caller and runs the
  callee on the same loop ([vm.md](../../docs/impl/vm.md)).
- `execute_bytecode_from_ip`, `execute_bytecode_saving_stack` and
  `run_dispatch` take `&Rc<Vec<Value>>`, an empty `Rc` for no environment;
  `execute_code` takes an `Option`.

## Primitive dispatch

A primitive is a `PrimitiveDef` whose `func` is a `PrimFn`:
`fn(&mut NativeCtx, &[Value]) -> (SignalBits, Value)`. The VM dispatches the
return signal in `handle_primitive_signal()` ([signal.rs](signal.rs)):

- `SIG_OK`: push the value to the stack.
- `SIG_ERROR` (any bits containing it): store `(bits, value)` in
  `fiber.signal`, and push NIL.
- `SIG_HALT`: store it in `fiber.signal`, and leave the loop.
- Any other suspending bits: retain the payload, record the park in the
  delivery ledger, park the frame in `fiber.suspended`, and leave the loop.
- `SIG_RESUME`: dispatch to the fiber handler.
- `SIG_PROPAGATE`: propagate the child fiber's signal, and keep the child
  chain.
- `SIG_ABORT`: inject an error at the target fiber's suspension point.
- `SIG_QUERY`: `dispatch_query()` ([query.rs](signal/query.rs), which owns
  the list of operations), and push the result.

`fiber/resume` returns `(SIG_RESUME, fiber_value)`. At the root the VM swaps
the child into `vm.fiber` with `FiberHandle::take()` and `put()`, runs it, and
swaps back. Inside a fiber it parks the caller and hands the child to the
`SIG_SWITCH` trampoline.

On resume, the VM wires up the parent and child chain (Janet semantics):

- It sets `parent.child = child_handle` before it executes the child.
- When the signal is caught (`SIG_OK` or a mask match), it clears
  `parent.child`.
- When the signal is not caught and propagates, `parent.child` stays set, so
  the trace chain survives.

## Invariants

1. **Stack underflow is a VM bug.** Every pop must have a preceding push.
   "Stack underflow" means the bytecode or the emitter is broken. Every
   handler panics on stack underflow.

2. **Closure environments are immutable `Rc<Vec>`.** The vec is created at
   closure call time; a mutation goes through a cell, never through the env.

3. **`CaptureCell` auto-unwraps on `LoadUpvalue`.** `LBox`, the user's `box`,
   does not.

4. **A tail call does not grow `call_depth`.** `TailCall` stores the pending
   call info and returns; the outer loop executes it.

5. **An interpreted non-tail call does not grow the Rust stack.**
   `call_inner` stores a `PendingCall` and returns; `run_dispatch` pauses the
   caller in `fiber.callers` and runs the callee on the same loop
   ([vm.md](../../docs/impl/vm.md)).

6. **A yield and an error use `SuspendedFrame` chains.** On either, a
   `SuspendedFrame` captures the code object, the env (`Rc`), the IP and the
   operand stack. When the signal leaves a callee, `run_dispatch` parks the
   callee where it stopped and appends each paused caller's frame to
   `fiber.suspended`. `resume_suspended` replays frames from innermost (index
   0) to outermost (last index).

7. **A VM bug panics, and a user error sets `fiber.signal`.** A VM bug (stack
   underflow, bad bytecode) panics at once. A primitive returns
   `(SIG_ERROR, ctx.error(kind, msg))`, which `handle_primitive_signal` stores
   in `fiber.signal`; VM code raises through `VM::set_error` in
   [mod.rs](mod.rs), and a fiber through `Fiber::set_error_in`. An intrinsic
   bytecode op (`Add`, `Sub`, `Mul`, `Div`, `Lt` and the rest) trusts its
   operands: a wrong type produces garbage, not a crash or a signal. This
   matches WASM and SPIR-V semantics. It is sound because a call-position
   `%`-op compiles only when its operand contract is proven
   ([contract.rs](../hir/typeinfer/contract.rs)), and that proof makes the
   op's compile-time silence truthful. A `%`-op called as a value routes
   through its registered native function, which checks its arguments at run
   time. [The VM design](../../docs/impl/vm.md) states what a raising handler
   leaves on the stack.

## Re-entrancy

`execute_bytecode_saving_stack` makes the VM re-entrant. It saves the caller's
operand stack, runs inner bytecode from IP 0, then restores it on return. The
inner execution sees an empty stack and runs on the same fiber (same heap,
parameter frames). Each re-entry nests on the Rust stack, so it halts with
`:stack-overflow` when `native_stack` reports less than its reserve left.

| Caller | File | Context |
|--------|------|---------|
| `Eval` instruction | [eval.rs](eval.rs) | Compiles and runs Elle source from within running code |
| `import/load-file`, `import/load-syntax` | [modules.rs](../primitives/modules.rs) | Run a module's body |
| The test-setup loader | [modules.rs](signal/modules.rs) | Runs a test file's setup module |
| A fiber's first resume | [resume.rs](fiber/resume.rs) | Runs a new fiber's body |
| `arena/allocs` SIG_QUERY handler | [allocs.rs](signal/allocs.rs) | Runs a thunk to measure its allocations |
| `compile/run-on :bytecode` | [bytecode.rs](run_on/bytecode.rs) | Runs a closure on the bytecode tier |
| The tail-call sentinel | [jit.rs](run_on/jit.rs), [jit_entry.rs](jit_entry.rs) | Finishes a tail call a compiled callee handed back |
| `call_transformer`, `run_at_expansion` | [expansion.rs](expansion.rs) | Macro transformers and `begin-for-syntax` definitions, under the expansion fuel budget, with the unit they expand lent to `gensym` |
| `call_closure` | [call.rs](call.rs) | Trait methods, and the `import/resolve` an `include` calls |
| JIT helpers | [callops.rs](../jit/calls/callops.rs) | Run an uncompiled callee, or any callee once the native stack is low |
| The WASM host | [linker.rs](../wasm/linker.rs), [linker.rs](../wasm/lazy/linker.rs) | Falls back to bytecode for a callee the module does not hold |
| FFI callback | [callback.rs](../ffi/callback.rs) | Runs a closure a C function calls back |

A host that runs code on the current fiber cannot hold a suspension of that
code. `eval`, `import/load-file`, `import/load-syntax`, the `compile/*-module`
setup runs, `compile/run-on :jit`, a macro expansion, `call_closure`, an FFI
callback and the root driver each refuse one. `refuse_hosted_park` ends the
park through the discard chokepoint, and the host raises at its own call. An
error the hosted code raises parks its chain as any error does, and every host
ends that park through `discard_hosted_error` before it raises or reports.
`arena/allocs` and `compile/run-on :bytecode` hand the suspension on as their
own call's park (`abandon_hosted_park`). The module
doc of [execute.rs](execute.rs) holds the rules on what is preserved, what is
overwritten, and how to add a caller.

## Suspension mechanism

When a fiber suspends, or stops on an error:

1. **Emit instruction** (`handle_emit`): a suspending emit captures the
   innermost frame as a `SuspendedFrame` in `fiber.suspended`. The frame
   holds the code object, the env (an `Rc` clone), the IP after the emit, and
   the operand stack. An emit of a bit the fiber withholds raises nothing: it
   parks the same frame on a capability denial (`VM::emit_blocked`).
2. **Suspending primitive** (`handle_primitive_signal`, call position): parks
   its own frame the same way. A tail-position suspend, a fuel pause and an
   error park no frame of their own: the code they return to parks it
   (`complete_call` for a callee, `do_fiber_first_resume`, the re-suspend in
   `resume_suspended`).
3. **Return into a paused caller** (when a suspend or an error leaves a
   callee): `run_dispatch` appends the caller's frame to `fiber.suspended`.
4. **Frame ordering**: the innermost frame (the one that yielded or
   signaled) sits at index 0, and the outermost caller at the last index.
5. **Resume** (`resume_suspended`): iterates the frames forward, and calls
   `execute_bytecode_from_ip` for each. A frame that stops again re-parks,
   and every signal but a halt keeps the outer frames behind it.

A park at a suspending primitive call records how its delivery is funded in
the delivery ledger (`Fiber::delivery`), and the install that displaces the
park releases a payload the runtime built.
[park.md](../../docs/impl/region/park.md) owns both rules.

Key methods:

- `execute_scheduled` and `handle_sig_switch` ([scheduled.rs](scheduled.rs)):
  the entry points the scheduler calls.
- `run_thunk_to_completion`: `execute_bytecode_saving_stack` plus the
  `SIG_SWITCH` drain loop. It is the safe entry for a re-entrant caller that
  runs a thunk on the current fiber: `eval`, `import/load-file`,
  `import/load-syntax`, `arena/allocs` and the test-setup module loader.
- `resume_suspended`: replays a `Vec<SuspendedFrame>`, and handles re-yields
  and errors. A frame that raises parks through `park_raised`, which records
  the raise site in the delivery ledger.
- `with_child_fiber` ([child.rs](fiber/child.rs)): the swap protocol fiber
  resume and abort share. It swaps the child fiber into `vm.fiber`, wires the
  parent and child chain, runs the body, then swaps back. No heap swap is
  involved.

## The heap

The VM owns exactly one `FiberHeap`, reached through `vm.heap_ptr` and
`vm.heap()`. The instance's `RuntimeCore` owns it (a bare VM leaks a private
one), and it outlives the VM, so a `Value` that `execute` returns stays valid
after the VM drops. Every fiber, the root included, shares this one heap, and
reaches it the same way. Isolation is per region, not per fiber.

`FiberHeap` allocates every value into a region, and a region is freed when
its reference count reaches zero ([memory.md](../../docs/impl/memory.md)).

`reset_fiber()` in [lifecycle.rs](core/lifecycle.rs) does not clear the heap.
Objects accumulate across resets, so a `Value` returned across several
invocations stays valid.

## Parameter resolution

When a program calls a parameter with no arguments, the VM searches the
parameter frame stack from the top (the most recent `parameterize`) to the
bottom. It returns the first binding it finds, or else the parameter's
default value.

`param_frames: Vec<Vec<(u32, Value)>>` is a stack of frames, and each frame is
a vector of (parameter id, value) pairs. `PushParamFrame` pushes a new frame,
and `PopParamFrame` pops the current one.

A child fiber inherits its creator's bindings as one baseline frame: the
creator's stack flattened at `fiber/new`, innermost winning. The creator's
`parameterize` blocks unwind long before the scheduler resumes the child. The
baseline is a counted holder of every heap value in it
([park.md](../../docs/impl/region/park.md)).

Code that stops running pops no frame it pushed, so whatever abandons it
truncates the stack. An error parks the frames that pushed them, so a raise
truncates nothing, and a restart runs on with every binding in force. Each
host and boundary records the depth at its entry as a `ParamDepth`. The
discard chokepoint truncates to it where a `squelch` boundary, a broken proof,
a refusing host or a host's error ends a park (`discard_suspended_frames`).
