# vm

<!-- audited: 2026-09-22 -->

Bytecode execution. Stack-based operand handling with register-addressed locals.

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
| `VM` | Global state + root Fiber. Per-execution state lives on `vm.fiber` |
| `SignalBits` | Internal return type (see [signals](../signals/AGENTS.md)) |
| `CallFrame` | The entered and calling code objects, IP, frame base |
| `PausedCaller` | A caller activation waiting in `Fiber::callers` for its callee |

## Data flow

```
A code-object blueprint (TemplateProto)
    │
    ▼
execute_proto()  ← public API, materializes the code object, returns Result<Value, String>
    │
    ├─► run_dispatch() — runs non-tail callees on fiber frames
    │
    ├─► execute_bytecode_inner_impl() → (SignalBits, usize)
    │       │
    │       ├─► fetch instruction
    │       ├─► dispatch by opcode
    │       ├─► modify stack/locals
    │       ├─► check for errors
    │       └─► loop until Return/Yield/Error
    │       │
    │       ▼
    │   (SignalBits, ip) — signal + IP at exit
    │
    ▼
Result<Value, String>  ← translation boundary
```

## Signal-based returns

Internal VM methods return `SignalBits` (see [signals](../signals/AGENTS.md) for
bit definitions). The dispatch loop handles each signal:
- `SIG_OK`: Normal completion. Value in `fiber.signal`.
- `SIG_ERROR`: Error struct in `fiber.signal`.
- `SIG_YIELD`: Fiber yield. Suspended frames in `fiber.suspended`.
- `SIG_RESUME`: Fiber primitive requests VM-side context switch.
- `SIG_PROPAGATE`: `fiber/propagate` re-signals caught signal.
- `SIG_QUERY`: Primitive reads VM state (arena stats, introspection).
- `SIG_HALT`: Graceful VM termination. Non-resumable.

The public `execute_proto` method (through `execute_code`) is the translation
boundary — it converts `SignalBits` to `Result<Value, String>` for external
callers. On `SIG_ERROR`, it extracts the error struct from `fiber.signal` and
formats the error message.

Instruction handlers return `()`. VM bugs panic immediately. User errors set
`fiber.signal` to `(SIG_ERROR, error_val(kind, msg))` and push `Value::NIL` to
keep the stack consistent.

## Threading the code object

Bytecode, constants and the location table are threaded through the dispatch
loop as one `Code` — the code object itself, a payload slice plus a blueprint
pointer (docs/impl/region/template.md). Individual instruction handlers take
slices (`&[u8]`, `&[Value]`) read off it. Only the dispatch loop and its direct
callees (`handle_emit`, `handle_call`) need the `Code` — they clone it cheaply
(two words and one refcount) when creating `SuspendedFrame`s, `TailCallInfo`, or
`PendingCall`.

- `execute_proto` materializes the code object once at the public boundary
- `execute_bytecode_from_ip` / `execute_bytecode_saving_stack` take a `&Code`
- `TailCallInfo` carries the tail callee's `Code`, env `Rc`, the callee closure
  value (installed as `fiber.current_closure` on the frame replacement), and its
  squelch mask — tail calls clone the `Rc`s (cheap), not the `Vec`s (expensive).
  The releases the call strands are NOT carried here: `tail_call_inner` records
  them on the activation's own `ActivationDues`, which outlives whoever consumes
  the pending call (docs/impl/region/owner.md § "A deferred tail-call release has
  the node's life")
- `PendingCall` carries a non-tail callee's `Code`, env `Rc` and closure value
  from `call_inner` to `run_dispatch`, which pauses the caller and runs the
  callee on the same loop (docs/impl/vm.md § "Non-tail calls")
- `closure_env` parameter is `&Rc<Vec<Value>>` (non-optional; empty Rc for no env)

## Primitive dispatch (NativeFn)

A primitive is a `PrimitiveDef` whose `func` is a `PrimFn`:
`fn(&mut NativeCtx, &[Value]) -> (SignalBits, Value)`. The VM
dispatches the return signal in `handle_primitive_signal()` (`signal.rs`):
- `SIG_OK` → push value to stack
- `SIG_ERROR` → store `(SIG_ERROR, value)` in `fiber.signal`, push NIL
- `SIG_YIELD` → store in `fiber.signal`, return yield
- `SIG_RESUME` → dispatch to fiber handler
- `SIG_PROPAGATE` → propagate child fiber's signal, preserve child chain
- `SIG_ABORT` → inject an error at the target fiber's suspension point
- `SIG_QUERY` → dispatch to `dispatch_query()`, push result to stack. Operations: `arena/allocs` (re-entrant, handled before dispatch), `arena/stats` (0-arg: current fiber; 1-arg: suspended fiber; includes scope-enter/dtor counts), `call-count`, `doc`, `global?`, `fiber/self`, `jit/rejections`, `list-primitives`, `primitive-meta`

All SIG_RESUME primitives (including fiber wrappers) return
`(SIG_RESUME, fiber_value)`. The VM uses `FiberHandle::take()`/`put()` to swap
the child fiber into `vm.fiber`, executes the child, then swaps back.

On resume, the VM wires up the parent/child chain (Janet semantics):
- `parent.child = child_handle` before executing child
- On signal caught (SIG_OK or mask match): clear `parent.child = None`
- On signal NOT caught (propagates): leave `parent.child` set (trace chain)

## Dependents

- `primitives/` - NativeFn primitives; SIG_RESUME signals trigger VM-side execution
- `repl.rs` - REPL session: form-by-form compilation with def persistence across inputs
- `main.rs` - file execution

## Invariants

1. **Stack underflow is a VM bug.** Every pop must have a preceding push.
   If you see "Stack underflow," the bytecode or emitter is broken. Handlers
   panic on stack underflow.

2. **Closure environments are immutable Rc<Vec>.** The vec is created at
   closure call time; mutations go through cells, not env modification.

3. **`CaptureCell` auto-unwraps on `LoadUpvalue`.** `LBox` (user's `box`) does
   NOT auto-unwrap. This distinction matters.

4. **Tail calls don't grow call_depth.** `TailCall` stores pending call info
   and returns; the outer loop executes it.

5. **An interpreted non-tail call doesn't grow the Rust stack.** `call_inner`
   stores a `PendingCall` and returns; `run_dispatch` pauses the caller in
   `fiber.callers` and runs the callee on the same loop. Only re-entry (a
   primitive calling a closure) and compiled code nest on the Rust stack, and
   both check `native_stack` first (docs/impl/vm.md § "Non-tail calls").

6. **Yield uses `SuspendedFrame` chains.** On yield, a `SuspendedFrame`
   captures the code object, env (`Rc`), IP, and operand stack. When the yield
   leaves a callee, `run_dispatch` appends each paused caller's frame to
   `fiber.suspended`. `resume_suspended` replays frames from innermost (index
   0) to outermost (last index).

7. **VM bugs panic, user errors set `fiber.signal`.** Instruction handlers
   return `()` (not `Result`). VM bugs (stack underflow, bad bytecode) panic
   immediately. Primitives and stdlib wrappers produce catchable errors via
   `fiber.signal = (SIG_ERROR, error_val(kind, msg))`. Intrinsic bytecode
   ops (Add, Sub, Mul, Div, Lt, etc.) trust their operands — wrong types
   produce garbage, not crashes or signals. This matches WASM/SPIR-V
   semantics, and it is sound because a call-position `%`-op only compiles
   when its operand contract is proven (prove-or-reject,
   `hir/typeinfer/contract.rs`) — which is what makes the ops'
   compile-time `Silent` signal truthful. A `%`-op called dynamically as a
   value routes through its registered NativeFn, which validates arguments
   at runtime.
   See `set_error()` in `call.rs` and `fiber.rs` for the signal-based helper.

## Key VM fields

| Field | Type | Purpose |
|-------|-------|---------|
| `fiber` | `Fiber` | Current fiber: stack, call frames, signal state |
| `heap_ptr` | `*mut FiberHeap` | This instance's single heap, owned by `RuntimeCore` (or privately leaked for a bare VM). All fibers share it; reach it via `heap()` |
| `current_fiber_handle` | `Option<FiberHandle>` | Handle for current fiber (`None` for root) |
| `current_fiber_value` | `Option<Value>` | Cached Value for current fiber (`None` for root) |
| `jit_cache` | `FxHashMap<*const u8, JitCacheEntry>` | JIT code cache; each entry pins the bytecode allocation its key names (docs/impl/jit.md § "Cache identity"). Write via `install_jit_code`, read via `jit_code_for` |
| `jit_rejections` | `FxHashMap<*const u8, JitRejectionInfo>` | JIT rejection log: first rejection per closure template |
| `closure_call_counts` | `FxHashMap<*const u8, usize>` | JIT hotness profiling (FxHash for pointer keys) |
| `pending_tail_call` | `Option<TailCallInfo>` | Rc-based tail call info (transient) |
| `pending_call` | `Option<PendingCall>` | The non-tail callee `call_inner` hands to `run_dispatch` (transient) |
| `error_loc` | `Option<SourceLoc>` | Where the error now propagating was raised. Written by `record_error_loc` (first-writer-wins, so the innermost frame keeps it), taken by `absorbs` when a mask catches (docs/impl/vm.md § "Where a reported error's location comes from") |
| `env_cache` | `Vec<Value>` | Reusable buffer for `build_closure_env` (avoids alloc per call) |
| `tail_call_env_cache` | `Vec<Value>` | Reusable buffer for `handle_tail_call` env building |
| `eval_expander` | `Option<Expander>` | Cached Expander for runtime `eval` (avoids re-loading prelude) |
| `user_args` | `Vec<String>` | User-provided arguments from `--` separator on the command line. Empty if no `--` was given. Read by `sys/args` primitive. |

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
| `delivery` | `Delivery` | The delivery ledger: how the current park's delivery references are funded — the raise-minted payload, the bodyless (denial) payload whose release the displacing install owes, and whether the resume value owes a mint. Method-only surface (docs/impl/region/park.md § "A park names its funding in the delivery ledger") |
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
| `eval` primitive | `eval.rs` | Compiles and runs Elle source from within running code |
| A fiber's first resume | `fiber/resume.rs` | Runs a new fiber's body |
| `arena/allocs` SIG_QUERY handler | `signal/config.rs` | Runs a thunk to measure its allocations |
| `call_closure` | `call.rs` | Macro transformers and trait methods |
| JIT helpers | `jit/calls/callops.rs` | Run an uncompiled callee, or any callee once the native stack is low |
| FFI callback | `ffi/callback.rs` | Runs a closure a C function calls back |

### Yield hazard

If the inner closure yields (`SIG_YIELD`), the saved outer stack is restored but
the fiber is suspended mid-inner-execution. Callers that invoke user-provided
closures (`eval`, `arena/allocs`) do not handle yield — they propagate the signal
upward. Closures passed to these must be non-yielding (silent signal). This is not
currently enforced at the call site.

See `execute.rs` module doc for the full rules on what is preserved, what is
overwritten, and how to add new callers.

## Suspension mechanism

When a fiber suspends (via yield instruction or `emit`):

1. **Emit instruction** (`handle_emit`): captures innermost frame as a
   `SuspendedFrame` with the code object, env (Rc clone), IP (after the emit),
   and operand stack. Stored in `fiber.suspended`.
2. **Return into a paused caller** (if the yield leaves a callee):
   `run_dispatch` appends the caller's frame to the `fiber.suspended` vec.
3. **Signal suspension** (`emit`): single `SuspendedFrame` with empty
   stack, stored in `fiber.suspended` by the resume handler.
4. **Frame ordering**: innermost (yielder/signaler) at index 0, outermost
   (caller) at last index.
5. **Resume** (`resume_suspended`): iterates frames forward, calling
   `execute_bytecode_from_ip` for each. Handles re-yields and errors.

A park at a suspending PRIMITIVE call — a dynamic `emit`, a capability denial —
resumes into that call's continuation, which releases the call's result. The
primitive never returns, so nothing mints the reference that release consumes:
`handle_primitive_signal` and the denial path record the shape in the delivery
ledger (`Fiber::delivery` — `park_primitive` / `park_denial`) and
`do_fiber_resume_single` takes it (`take_resume_funding`) as it delivers.

A runtime-built payload owes one more, in the other direction. The body names it
nowhere and no continuation releases it — the install that displaces the park
does. Two parks are that shape, and each has its own reading. A DENIAL's payload
is named by the ledger's record (`park_denial` writes it,
`release_displaced_denial_payload` takes it through `take_bodyless`); an io
park's payload is named by BEING an `IoRequest` under `SIG_IO`
(`release_displaced_io_request`). The readings name disjoint payloads — a
denial's struct is never an `IoRequest` — so both run and neither asks what the
other did, which is what a fiber denied `:io` needs: it parks under the same
`SIG_IO` bit, and the install may reach a fiber that only relays the park and
holds no record to defer to. The installs are `fiber/resume`, the `fiber/abort` /
`fiber/refuse` injection, and the three `FiberResume` deliveries that reach an
inner fiber directly, and each owes the release — a `Fresh` op whose completion
buffer lives in the request's own region is a second value there, not a second
consumer of the retain. See docs/impl/region/park.md § "A payload the RUNTIME
built is released by the install that displaces it".

Key methods:
- `run_dispatch`: Runs one activation's dispatch loop, with every interpreted
  non-tail callee it calls paused and resumed on `fiber.callers`
- `execute_bytecode_from_ip`: Executes from a given IP with Rc bytecode/constants
- `execute_bytecode_saving_stack`: Saves/restores caller's stack, handles tail calls
- `run_thunk_to_completion`: `execute_bytecode_saving_stack` + the `SIG_SWITCH` drain loop — the safe entry for re-entrant callers running a thunk on the current fiber (`eval`, `arena/allocs`, test-setup module loader)
- `resume_suspended`: Replays `Vec<SuspendedFrame>`, handles re-yields and errors
- `with_child_fiber` (`fiber/child.rs`): Shared swap protocol for fiber
  resume/cancel. Swaps the child fiber into `vm.fiber`, wires the parent/child
  chain, runs the body, then swaps back. No heap swap is involved: all fibers
  (including root) share the VM's single heap, reached via `vm.heap_ptr`.

## The heap

The VM owns exactly one `FiberHeap`, reached via `vm.heap_ptr` / `vm.heap()`. It
is owned by the instance's `RuntimeCore` (or privately leaked for a bare VM) and
outlives the VM, so Values returned by `execute_proto` remain valid after the
VM drops. ALL fibers — including the root — share this one heap, reached the
same way (`vm.heap_ptr`) on every fiber; isolation is per-region, not per-fiber.

`FiberHeap` allocates every value into a region, and a region is freed when its
reference count reaches zero ([memory.md](../../docs/impl/memory.md)).

`reset_fiber()` in `core.rs` does not clear the heap — objects accumulate across
resets, so Values returned across multiple invocations remain valid.

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
(docs/impl/region/park.md § "A child's inherited parameter baseline is a counted
holder").
## Truthiness

The VM evaluates truthiness via `Value::is_truthy()`:
- `Value::NIL` → falsy
- `Value::FALSE` → falsy  
- Everything else (including `Value::EMPTY_LIST`, `Value::int(0)`) → truthy

The `Instruction::Nil` pushes `Value::NIL` (falsy).
The `Instruction::EmptyList` pushes `Value::EMPTY_LIST` (truthy).
