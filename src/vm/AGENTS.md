# vm

<!-- audited: 2026-10-07 -->

The VM executes bytecode on a fiber's operand stack, with each local in a stack slot above the frame base.

[docs/impl/vm.md](../../docs/impl/vm.md) holds the design: the dispatch loop,
non-tail calls, and where a reported error's location comes from.

## Responsibility

Execute bytecode instructions. Manage:
- Operand stack
- Call frames: paused callers, stack traces, region-remap frames
- Closure environments
- Fiber state and signals

Does NOT:
- Compile code (that's `compiler/`, `hir/`, `lir/`)
- Parse source (that's `reader/`)
- Define primitives (that's `primitives/`)

## Interface

| Type | Purpose |
|------|---------|
| `VM` | Per-instance state and the running fiber (`vm.fiber`, swapped on each resume) |
| `SignalBits` | Internal return type (see [signals](../signals/AGENTS.md)) |
| `CallFrame` | The entered and calling code objects, IP, frame base |
| `PausedCaller` | A caller activation waiting in `Fiber::callers` for its callee |

## Data flow

```
A compiled unit (CodeUnit)
    │
    ▼
execute()        ← public API: copies a unit compiled on another heap,
    │              then runs its entry header
    ▼
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

Internal VM methods return `SignalBits` (see [signals](../signals/AGENTS.md) for
bit definitions). The dispatch loop only exits on a signal; the drivers above
it act on each one:
- `SIG_OK`: Normal completion. Value in `fiber.signal`.
- `SIG_ERROR`: Error struct in `fiber.signal`.
- `SIG_YIELD`: Fiber yield. Suspended frames in `fiber.suspended`.
- `SIG_RESUME`: Fiber primitive requests VM-side context switch.
- `SIG_SWITCH`: A fiber resumed inside a fiber; the trampoline runs the child.
- `SIG_PROPAGATE`: `fiber/propagate` re-signals caught signal.
- `SIG_ABORT`: Inject an error at the target fiber's suspension point.
- `SIG_QUERY`: Primitive reads VM state (arena stats, introspection).
- `SIG_FUEL`: The instruction budget ran out.
- `SIG_HALT`: Graceful VM termination. Non-resumable.

`execute_code`, which `execute` calls, is the translation boundary. It
converts `SignalBits` to `Result<Value, String>` for external callers. On
`SIG_ERROR`, it extracts the error struct from `fiber.signal` and formats the
error message.

Most instruction handlers return `()`. The call, tail-call and emit handlers
return the signal that ends the dispatch loop. VM bugs panic. A user error in
call position calls `VM::set_error(kind, msg)` and pushes `Value::NIL` to keep
the stack consistent (invariant 7 below); one in tail position returns
`Some(SIG_ERROR)`.

## Threading the code object

Bytecode, constants and the location table are threaded through the dispatch
loop as one `Code` — the code object itself, one payload slice
([template.md](../../docs/impl/region/template.md)). Individual instruction
handlers take slices (`&[u8]`, `&[Value]`) read off it. The dispatch loop, the
call, emit and closure handlers, and the signal handlers they call take the
`Code`. They copy it (one word) when they build a `SuspendedFrame`,
`TailCallInfo` or `PendingCall`.

- `execute` takes the unit's entry header at the public boundary
- `execute_bytecode_from_ip` / `execute_bytecode_saving_stack` take a `&Code`
- `TailCallInfo` carries the tail callee's `Code`, env `Rc`, the callee closure
  value (installed as `fiber.current_closure` on the frame replacement), and its
  squelch mask — tail calls clone the `Rc`s (cheap), not the `Vec`s (expensive).
  The releases the call strands are NOT carried here: `tail_call_inner` records
  them on the activation's own `ActivationDues`, which outlives whoever consumes
  the pending call ([owner.md](../../docs/impl/region/owner.md))
- `PendingCall` carries a non-tail callee's `Code`, env `Rc` and closure value
  from `call_inner` to `run_dispatch`, which pauses the caller and runs the
  callee on the same loop ([vm.md](../../docs/impl/vm.md))
- `execute_bytecode_from_ip`, `execute_bytecode_saving_stack` and
  `run_dispatch` take `&Rc<Vec<Value>>`, an empty `Rc` for no environment;
  `execute_code` takes an `Option`

## Primitive dispatch (NativeFn)

A primitive is a `PrimitiveDef` whose `func` is a `PrimFn`:
`fn(&mut NativeCtx, &[Value]) -> (SignalBits, Value)`. The VM
dispatches the return signal in `handle_primitive_signal()`
([signal.rs](signal.rs)):
- `SIG_OK` → push value to stack
- `SIG_ERROR` (any bits containing it) → store `(bits, value)` in
  `fiber.signal`, push NIL
- `SIG_HALT` → store in `fiber.signal`, leave the loop
- any other suspending bits → retain the payload, record the park in the
  delivery ledger, park the frame in `fiber.suspended`, leave the loop
- `SIG_RESUME` → dispatch to fiber handler
- `SIG_PROPAGATE` → propagate child fiber's signal, preserve child chain
- `SIG_ABORT` → inject an error at the target fiber's suspension point
- `SIG_QUERY` → `dispatch_query()` ([query.rs](signal/query.rs), which owns the
  list of operations), push the result

`fiber/resume` returns `(SIG_RESUME, fiber_value)`. At the root the VM swaps
the child into `vm.fiber` with `FiberHandle::take()`/`put()`, runs it, and
swaps back. Inside a fiber it parks the caller and hands the child to the
`SIG_SWITCH` trampoline.

On resume, the VM wires up the parent/child chain (Janet semantics):
- `parent.child = child_handle` before executing child
- On signal caught (SIG_OK or mask match): clear `parent.child = None`
- On signal NOT caught (propagates): leave `parent.child` set (trace chain)

## Dependents

- `primitives/` - NativeFn primitives; SIG_RESUME signals trigger VM-side execution
- `jit/`, `wasm/` - compiled code calls back into the VM for uncompiled callees
- [ffi/callback.rs](../ffi/callback.rs) - runs a closure a C function calls back
- `runtime/` - owns the VM and its heap
- [repl.rs](../repl.rs) - REPL session: form-by-form compilation with def persistence across inputs
- [program.rs](../program.rs) - file, stdin and `-e` execution for `elle` and the rig

## Invariants

1. **Stack underflow is a VM bug.** Every pop must have a preceding push.
   If you see "Stack underflow," the bytecode or emitter is broken. Every
   handler panics on stack underflow.

2. **Closure environments are immutable Rc<Vec>.** The vec is created at
   closure call time; mutations go through cells, not env modification.

3. **`CaptureCell` auto-unwraps on `LoadUpvalue`.** `LBox` (user's `box`) does
   NOT auto-unwrap.

4. **Tail calls don't grow call_depth.** `TailCall` stores pending call info
   and returns; the outer loop executes it.

5. **An interpreted non-tail call doesn't grow the Rust stack.** `call_inner`
   stores a `PendingCall` and returns; `run_dispatch` pauses the caller in
   `fiber.callers` and runs the callee on the same loop (see
   [vm.md](../../docs/impl/vm.md)).

6. **Yield uses `SuspendedFrame` chains.** On yield, a `SuspendedFrame`
   captures the code object, env (`Rc`), IP, and operand stack. When the yield
   leaves a callee, `run_dispatch` appends each paused caller's frame to
   `fiber.suspended`. `resume_suspended` replays frames from innermost (index
   0) to outermost (last index).

7. **VM bugs panic, user errors set `fiber.signal`.** VM bugs (stack
   underflow, bad bytecode) panic immediately. A primitive returns
   `(SIG_ERROR, ctx.error(kind, msg))`, which `handle_primitive_signal` stores
   in `fiber.signal`; VM code raises through `VM::set_error`. Intrinsic
   bytecode ops (Add, Sub, Mul, Div, Lt, etc.) trust their operands — wrong
   types produce garbage, not crashes or signals. This matches WASM/SPIR-V
   semantics, and it is sound because a call-position `%`-op only compiles
   when its operand contract is proven (prove-or-reject,
   [contract.rs](../hir/typeinfer/contract.rs)) — which is what makes the
   ops' compile-time `Silent` signal truthful. A `%`-op called dynamically as
   a value routes through its registered NativeFn, which validates arguments
   at runtime. See `VM::set_error` in [mod.rs](mod.rs) and
   `Fiber::set_error_in` in [fiber.rs](../value/fiber.rs).
   A handler that raises pushes one `Value::NIL` in place of its result, and
   the post-handler error exit pops it, so an error park holds nothing in the
   raising call's result position and a restart's resume value lands there.
   An instruction with no result (`CheckSignalBound`, `PushParamFrame`) pushes
   no placeholder, and its park takes no resume value
   ([vm.md](../../docs/impl/vm.md)).

## Key VM fields

| Field | Type | Purpose |
|-------|-------|---------|
| `fiber` | `Fiber` | Current fiber: stack, call frames, signal state |
| `heap_ptr` | `*mut FiberHeap` | This instance's single heap, owned by `RuntimeCore` (or privately leaked for a bare VM). All fibers share it; reach it via `heap()` |
| `current_fiber_handle` | `Option<FiberHandle>` | Handle for current fiber (`None` for root) |
| `current_fiber_value` | `Option<Value>` | Cached Value for current fiber (`None` for root) |
| `jit_cache` | `FxHashMap<*const u8, JitCacheEntry>` | JIT code cache; each entry's `CodePin` holds the region its key's payload lives in ([jit.md](../../docs/impl/jit.md)). Write via `install_jit_code`, read via `jit_code_for` |
| `spirv_cache` | `FxHashMap<*const u8, SpirvEntry>` | The SPIR-V `git` and `mlir/compile-spirv` compiled: one entry per code object, pinned like `jit_cache`, holding a kernel per workgroup size ([spirv.md](../../docs/impl/spirv.md)) |
| `jit_rejections` | `FxHashMap<*const u8, JitRejectionInfo>` | JIT rejection log: first rejection per closure template |
| `closure_call_counts` | `CallCounts` | Hotness profiling by bytecode address; each count carries its code region's generation, so a freed function's count reads as zero ([jit.md](../../docs/impl/jit.md)) |
| `pending_tail_call` | `Option<TailCallInfo>` | Rc-based tail call info (transient) |
| `pending_call` | `Option<PendingCall>` | The non-tail callee `call_inner` hands to `run_dispatch` (transient) |
| `root_exit_depth` | `usize` | The operand depth the last root body left at its exit, recorded before `execute_code` restores the stack it took ([vm.md](../../docs/impl/vm.md)) |
| `error_loc` | `Option<SourceLoc>` | Where the error now propagating was raised. Written by `record_error_loc` (first-writer-wins, so the innermost frame keeps it), taken by `absorbs` when a mask catches ([vm.md](../../docs/impl/vm.md)) |
| `env_cache` | `Vec<Value>` | Reusable buffer for `build_closure_env` (avoids alloc per call) |
| `tail_call_env_cache` | `Vec<Value>` | Reusable buffer for the tail-call env built by `tail_call_inner` and `build_tail_call_env` |
| `eval_expander` | `Option<Expander>` | Cached Expander for runtime `eval` (avoids re-loading prelude) |
| `user_args` | `Vec<String>` | The arguments after the source file, after `-`, or after `--`; empty when none follow. Read by `sys/args` |

### Key Fiber fields (on `vm.fiber`)

| Field | Type | Purpose |
|-------|------|---------|
| `stack` | `SmallVec<[Value; 256]>` | Operand stack |
| `callers` | `Vec<PausedCaller>` | Caller activations waiting for an interpreted callee |
| `call_stack` | `Vec<CallFrame>` | For stack traces |
| `call_depth` | `usize` | Non-tail closure calls in progress, checked against `(vm/config :max-depth)` |
| `signal` | `Option<(SignalBits, Value)>` | Signal from execution (errors, yields) |
| `error_loc` | `Option<(Value, SourceLoc)>` | The parked `SIG_ERROR` payload and where it was raised. Parked by `absorbs`, read back by `fiber/propagate` so a re-raised error keeps its raising form |
| `suspended` | `Option<Vec<SuspendedFrame>>` | Suspended execution frames (for yield/signal resumption) |
| `delivery` | `Delivery` | The delivery ledger: how the current park's delivery references are funded — the raise-minted payload, the bodyless payload (a denial's struct, an io op's request) whose release the displacing install owes, the park whose delivery retain no reader has consumed, and whether the resume value owes a mint. Method-only surface ([park.md](../../docs/impl/region/park.md)) |
| `mask` | `SignalBits` | Which of this fiber's signals its parent catches |
| `param_frames` | `Vec<Vec<(u32, Value)>>` | Parameter binding frames (stack of frames, each frame a vec of (param id, value) pairs) |
| `parent` | `Option<WeakFiberHandle>` | Weak back-pointer to parent fiber |
| `parent_value` | `Option<Value>` | Cached Value for parent (identity-preserving) |
| `child` | `Option<FiberHandle>` | Strong pointer to child fiber |
| `child_value` | `Option<Value>` | Cached Value for child (identity-preserving) |

## Re-entrancy

`execute_bytecode_saving_stack` makes the VM re-entrant. It saves the caller's
operand stack, runs inner bytecode from IP 0, then restores it on return. The
inner execution sees an empty stack and runs on the same fiber (same heap,
parameter frames). Each re-entry nests on the Rust stack, so it halts with
`:stack-overflow` when `native_stack` reports less than its reserve left.

### Callers

| Caller | File | Context |
|--------|------|---------|
| `Eval` instruction | [eval.rs](eval.rs) | Compiles and runs Elle source from within running code |
| `import` | [modules.rs](../primitives/modules.rs) | Runs a module's body |
| The test-setup loader | [modules.rs](signal/modules.rs) | Runs a test file's setup module |
| A fiber's first resume | [resume.rs](fiber/resume.rs) | Runs a new fiber's body |
| `arena/allocs` SIG_QUERY handler | [config.rs](signal/config.rs) | Runs a thunk to measure its allocations |
| `compile/run-on :bytecode` | [bytecode.rs](run_on/bytecode.rs) | Runs a closure on the bytecode tier |
| The tail-call sentinel | [jit.rs](run_on/jit.rs), [jit_entry.rs](jit_entry.rs) | Finishes a tail call a compiled callee handed back |
| `call_closure` | [call.rs](call.rs) | Macro transformers and trait methods |
| JIT helpers | [callops.rs](../jit/calls/callops.rs) | Run an uncompiled callee, or any callee once the native stack is low |
| The WASM host | [linker.rs](../wasm/linker.rs), [linker.rs](../wasm/lazy/linker.rs) | Falls back to bytecode for a callee the module does not hold |
| FFI callback | [callback.rs](../ffi/callback.rs) | Runs a closure a C function calls back |

A host that runs code on the current fiber cannot hold a suspension of that
code. `eval`, `import`, the `compile/*-module` setup runs, `compile/run-on :jit`
and the root driver refuse one: `refuse_hosted_park` ends the park through the
discard chokepoint, and the host raises at its own call. `arena/allocs` and
`compile/run-on :bytecode` hand the suspension on as their own call's park
(`abandon_hosted_park`). The module doc of [execute.rs](execute.rs) holds the
rules on what is preserved, what is overwritten, and how to add a caller.

## Suspension mechanism

When a fiber suspends, or stops on an error:

1. **Emit instruction** (`handle_emit`): captures innermost frame as a
   `SuspendedFrame` with the code object, env (Rc clone), IP (after the emit),
   and operand stack. Stored in `fiber.suspended`.
2. **Suspending primitive** (`handle_primitive_signal`, call position): parks
   its own frame the same way. A tail-position suspend, a fuel pause and an
   error park no frame of their own: the driver they return to parks it
   (`do_fiber_first_resume`, the re-suspend in `resume_suspended`).
3. **Return into a paused caller** (if a suspend leaves a callee):
   `run_dispatch` appends the caller's frame to the `fiber.suspended` vec.
4. **Frame ordering**: innermost (yielder/signaler) at index 0, outermost
   (caller) at last index.
5. **Resume** (`resume_suspended`): iterates frames forward, calling
   `execute_bytecode_from_ip` for each. A frame that stops again re-parks,
   and every signal but a halt keeps the outer frames behind it.

A park at a suspending primitive call records how its delivery is funded in
the delivery ledger (`Fiber::delivery`), and the install that displaces the
park releases a payload the runtime built.
[park.md](../../docs/impl/region/park.md) owns both rules.

Key methods:
- `run_dispatch`: Runs one activation's dispatch loop, with every interpreted
  non-tail callee it calls paused and resumed on `fiber.callers`
- `execute_scheduled` and `handle_sig_switch` ([scheduled.rs](scheduled.rs)):
  scheduler-facing entry points
- `execute_bytecode_from_ip`: Executes from a given IP with the code object (`&Code`)
- `execute_bytecode_saving_stack`: Saves/restores caller's stack, handles tail calls
- `run_thunk_to_completion`: `execute_bytecode_saving_stack` + the `SIG_SWITCH` drain loop — the safe entry for re-entrant callers running a thunk on the current fiber (`eval`, `import`, `arena/allocs`, test-setup module loader)
- `replay_suspended`: Replays `Vec<SuspendedFrame>`, handles re-yields and
  errors, and answers a `Replay` naming the error park it built; a fiber
  boundary records that park in the delivery ledger. `resume_suspended` is the
  same replay for a driver that records no park (`handle_sig_switch`)
- `with_child_fiber` ([child.rs](fiber/child.rs)): Shared swap protocol for
  fiber resume and abort. Swaps the child fiber into `vm.fiber`, wires the
  parent/child chain, runs the body, then swaps back. No heap swap is
  involved: all fibers (including root) share the VM's single heap, reached
  via `vm.heap_ptr`.

## The heap

The VM owns exactly one `FiberHeap`, reached via `vm.heap_ptr` / `vm.heap()`. It
is owned by the instance's `RuntimeCore` (or privately leaked for a bare VM) and
outlives the VM, so Values returned by `execute` remain valid after the
VM drops. ALL fibers — including the root — share this one heap, reached the
same way (`vm.heap_ptr`) on every fiber; isolation is per-region, not per-fiber.

`FiberHeap` allocates every value into a region, and a region is freed when its
reference count reaches zero ([memory.md](../../docs/impl/memory.md)).

`reset_fiber()` in [lifecycle.rs](core/lifecycle.rs) does not clear the heap —
objects accumulate across resets, so Values returned across multiple
invocations remain valid.

## Parameter resolution

When a parameter is called (invoked as a function with no arguments), the VM
searches the parameter frame stack from top (most recent `parameterize`) to
bottom. If a binding is found, its value is returned. Otherwise, the parameter's
default value is returned.

**Frame structure**: `param_frames: Vec<Vec<(u32, Value)>>` is a stack of frames.
Each frame is a vector of (parameter id, value) pairs. `PushParamFrame` pushes a
new frame; `PopParamFrame` pops the current frame. When a parameter is called, the
VM iterates from the top frame downward, searching for a matching parameter.

**Inheritance**: a child fiber inherits its creator's bindings as ONE baseline
frame — the creator's stack flattened at `fiber/new`, innermost winning — because
the creator's `parameterize` blocks unwind long before the scheduler resumes the
child. The baseline is a counted holder of every heap value in it
([park.md](../../docs/impl/region/park.md)).

**Abandonment**: code that stops running pops no frame it pushed, so whatever
abandons it truncates the stack. Each call, host and boundary records the depth
at its entry as a `ParamDepth`. A caller whose callee raised truncates to it,
and so does the discard chokepoint where a `squelch` boundary or a refusing
host ends a park (`discard_suspended_frames`).

## Truthiness

The VM evaluates truthiness via `Value::is_truthy()`:
- `Value::NIL` → falsy
- `Value::FALSE` → falsy
- Everything else (including `Value::EMPTY_LIST`, `Value::int(0)`) → truthy

The `Instruction::Nil` pushes `Value::NIL` (falsy).
The `Instruction::EmptyList` pushes `Value::EMPTY_LIST` (truthy).
