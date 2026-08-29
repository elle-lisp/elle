# vm

Bytecode execution. Stack-based operand handling with register-addressed locals.

## Responsibility

Execute bytecode instructions. Manage:
- Operand stack
- Global bindings
- Call frames and stack traces
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
| `SignalBits` | Internal return type (see `signals/AGENTS.md`) |
| `CallFrame` | Function name, IP, frame base |

## Data flow

```
Bytecode + Constants (as Rc<Vec<u8>>, Rc<Vec<Value>>)
    │
    ▼
execute_bytecode()  ← public API, wraps slices in Rc once, returns Result<Value, String>
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

Internal VM methods return `SignalBits` (see `signals/AGENTS.md` for bit
definitions). The dispatch loop handles each signal:
- `SIG_OK`: Normal completion. Value in `fiber.signal`.
- `SIG_ERROR`: Error struct in `fiber.signal`.
- `SIG_YIELD`: Fiber yield. Suspended frames in `fiber.suspended`.
- `SIG_RESUME`: Fiber primitive requests VM-side context switch.
- `SIG_PROPAGATE`: `fiber/propagate` re-signals caught signal.
- `SIG_QUERY`: Primitive reads VM state (arena stats, introspection).
- `SIG_HALT`: Graceful VM termination. Non-resumable.

The public `execute_bytecode` method is the translation boundary — it converts
`SignalBits` to `Result<Value, String>` for external callers. On `SIG_ERROR`,
it extracts the error struct from `fiber.signal` and formats the error message.

Instruction handlers return `()`. VM bugs panic immediately. User errors set
`fiber.signal` to `(SIG_ERROR, error_val(kind, msg))` and push `Value::NIL` to
keep the stack consistent.

## Rc threading

Bytecode and constants are threaded through the dispatch loop as `&Rc<Vec<u8>>`
and `&Rc<Vec<Value>>`. Individual instruction handlers dereference to slices
(`&[u8]`, `&[Value]`). Only the dispatch loop and its direct callees
(`handle_yield`, `handle_call`) need the `Rc` — they clone it cheaply when
creating `SuspendedFrame`s or `TailCallInfo`.

- `execute_bytecode` wraps raw slices in `Rc` once at the public boundary
- `execute_bytecode_from_ip` / `execute_bytecode_saving_stack` take `&Rc`
- `TailCallInfo` carries the tail callee's `Code`, env `Rc`, the callee closure
  value (installed as `fiber.current_closure` on the frame replacement), its
  squelch mask, and an optional adopt region — tail calls clone the `Rc`s (cheap),
  not the `Vec`s (expensive)
- `closure_env` parameter is `&Rc<Vec<Value>>` (non-optional; empty Rc for no env)
- `execute_closure_bytecode` takes `&Rc` params directly (no `.to_vec()` copy);
  used by JIT trampolines where the closure already owns Rc'd data

## Primitive dispatch (NativeFn)

All primitives are `NativeFn`: `fn(&[Value]) -> (SignalBits, Value)`. The VM
dispatches the return signal in `handle_primitive_signal()` (`signal.rs`):
- `SIG_OK` → push value to stack
- `SIG_ERROR` → store `(SIG_ERROR, value)` in `fiber.signal`, push NIL
- `SIG_YIELD` → store in `fiber.signal`, return yield
- `SIG_RESUME` → dispatch to fiber handler
- `SIG_PROPAGATE` → propagate child fiber's signal, preserve child chain
- `SIG_CANCEL` → inject error into target fiber
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
   and returns; the outer loop executes it. Stack overflow = tail call bug.

5. **Yield uses `SuspendedFrame` chains.** On yield, a `SuspendedFrame`
   captures bytecode (`Rc`), constants (`Rc`), env (`Rc`), IP, and operand
   stack. When yield propagates through Call instructions, each caller's frame
   is appended to `fiber.suspended`. `resume_suspended` replays frames from
   innermost (index 0) to outermost (last index).

6. **VM bugs panic, user errors set `fiber.signal`.** Instruction handlers
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
| `error_loc` | `Option<SourceLoc>` | Where the error now propagating was raised. Written by `record_error_loc` (first-writer-wins, so the innermost frame keeps it), taken by `absorbs` when a mask catches (docs/impl/vm.md § "Where a reported error's location comes from") |
| `env_cache` | `Vec<Value>` | Reusable buffer for `build_closure_env` (avoids alloc per call) |
| `tail_call_env_cache` | `Vec<Value>` | Reusable buffer for `handle_tail_call` env building |
| `eval_expander` | `Option<Expander>` | Cached Expander for runtime `eval` (avoids re-loading prelude) |
| `user_args` | `Vec<String>` | User-provided arguments from `--` separator on the command line. Empty if no `--` was given. Read by `sys/args` primitive. |

### Key Fiber fields (on `vm.fiber`)

| Field | Type | Purpose |
|-------|------|---------|
| `stack` | `SmallVec<[Value; 256]>` | Operand stack |
| `call_stack` | `Vec<CallFrame>` | For stack traces |
| `call_depth` | `usize` | Stack overflow detection |
| `signal` | `Option<(SignalBits, Value)>` | Signal from execution (errors, yields) |
| `error_loc` | `Option<(Value, SourceLoc)>` | The parked `SIG_ERROR` payload and where it was raised. Parked by `absorbs`, read back by `fiber/propagate` so a re-raised error keeps its raising form |
| `suspended` | `Option<Vec<SuspendedFrame>>` | Suspended execution frames (for yield/signal resumption) |
| `resume_value_unfunded` | `bool` | Whether the innermost suspension is a PRIMITIVE call, so the next delivery owes its resume value one reference (docs/impl/region/owner.md § "A delivery into a replayed frame carries one owning reference") |
| `denial_payload` | `Option<Value>` | The capability-denial payload this fiber has parked. The runtime built it, so no continuation of the body releases it and the install that displaces the park owes that release; the bits cannot say so, a denial parking under the withheld capability's own bits (docs/impl/region/owner.md § "A payload the RUNTIME built is released by the install that displaces it") |
| `signal_mask` | `SignalBits` | Which signals this fiber catches |
| `param_frames` | `Vec<Vec<(Value, Value)>>` | Parameter binding frames (stack of frames, each frame is vec of (param, value) pairs) |
| `parent` | `Option<WeakFiberHandle>` | Weak back-pointer to parent fiber |
| `parent_value` | `Option<Value>` | Cached Value for parent (identity-preserving) |
| `child` | `Option<FiberHandle>` | Strong pointer to child fiber |
| `child_value` | `Option<Value>` | Cached Value for child (identity-preserving) |

## Re-entrancy

`execute_bytecode_saving_stack` makes the VM re-entrant. It saves the caller's
operand stack, runs inner bytecode from IP 0, then restores it on return. The
inner execution sees an empty stack and runs on the same fiber (same heap,
parameter frames).

### Callers

| Caller | File | Context |
|--------|------|---------|
| `eval` primitive | `eval.rs` | Compiles and runs Elle source from within running code |
| Non-yielding `fiber/resume` | `call.rs` | Runs a child fiber inline on the current thread |
| `arena/allocs` SIG_QUERY handler | `signal.rs` | Runs a thunk to measure its allocations |
| JIT trampolines | `call.rs` | Re-enters interpreter for uncompiled hot paths |
| Fiber resume | `call.rs` | Resumes a suspended fiber |

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

1. **Yield instruction** (`handle_yield`): captures innermost frame as a
   `SuspendedFrame` with bytecode (Rc clone), constants (Rc clone), env
   (Rc clone), IP (after yield), and operand stack. Stored in `fiber.suspended`.
2. **Call handler** (if yield propagates through a call): appends caller's
   frame to `fiber.suspended` vec.
3. **Signal suspension** (`emit`): single `SuspendedFrame` with empty
   stack, stored in `fiber.suspended` by the resume handler.
4. **Frame ordering**: innermost (yielder/signaler) at index 0, outermost
   (caller) at last index.
5. **Resume** (`resume_suspended`): iterates frames forward, calling
   `execute_bytecode_from_ip` for each. Handles re-yields and errors.

A park at a suspending PRIMITIVE call — a dynamic `emit`, a capability denial —
resumes into that call's continuation, which releases the call's result. The
primitive never returns, so nothing mints the reference that release consumes:
`handle_primitive_signal` and the denial path record the shape on the fiber
(`resume_value_unfunded`) and `do_fiber_resume_single` mints it as it delivers.

The denial owes one more, in the other direction. Its payload is the RUNTIME's,
so the body names it nowhere and no continuation releases it — the install that
displaces the park does, through `release_displaced_denial_payload` off the
`denial_payload` record: `fiber/resume`, the `fiber/abort` / `fiber/refuse`
injection, and the three `FiberResume` deliveries that reach an inner fiber
directly. `fiber/resume` asks the record before `release_parked_signal`'s io arm
and skips that arm when the record claims the park, a fiber denied `:io` parking
under the same `SIG_IO` bit an io request does. See docs/impl/region/owner.md
§ "A payload the RUNTIME built is released by the install that displaces it".

Key methods:
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
outlives the VM, so Values returned by `execute_bytecode` remain valid after the
VM drops. ALL fibers — including the root — share this one heap, reached the
same way (`vm.heap_ptr`) on every fiber; isolation is per-region, not per-fiber.

`FiberHeap` uses a bump arena (`BumpArena`) wrapped in `SlabPool` for all
allocations. Destructor tracking ensures `HeapObject` variants with inner heap
allocations (`Vec`, `Rc`, `BTreeMap`) have their `Drop` impls called on
`release()` and `clear()`. `release()` runs destructors, returns slab slots to
the free list, and rewinds the arena to the region-entry position. Memory is
reclaimed by region release (`DecrefRegion`), tail-call rotation, or fiber death.

`reset_fiber()` in `core.rs` does not clear the heap — objects accumulate across
resets, so Values returned across multiple invocations remain valid.

## Parameter resolution

When a parameter is called (invoked as a function with no arguments), the VM
searches the parameter frame stack from top (most recent `parameterize`) to
bottom. If a binding is found, its value is returned. Otherwise, the parameter's
default value is returned.

**Frame structure**: `param_frames: Vec<Vec<(Value, Value)>>` is a stack of frames.
Each frame is a vector of (parameter, value) pairs. `PushParamFrame` pushes a new
frame; `PopParamFrame` pops the current frame. When a parameter is called, the VM
iterates from the top frame downward, searching for a matching parameter.

**Inheritance**: Child fibers inherit parent parameter frames. When a child fiber
is created, it copies the parent's `param_frames` stack. This allows child code
to see parent-established parameter bindings.
## Truthiness

The VM evaluates truthiness via `Value::is_truthy()`:
- `Value::NIL` → falsy
- `Value::FALSE` → falsy  
- Everything else (including `Value::EMPTY_LIST`, `Value::int(0)`) → truthy

The `Instruction::Nil` pushes `Value::NIL` (falsy).
The `Instruction::EmptyList` pushes `Value::EMPTY_LIST` (truthy).
