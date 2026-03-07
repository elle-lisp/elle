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
| `SignalBits` | Internal return type: `SIG_OK`, `SIG_ERROR`, `SIG_YIELD`, `SIG_DEBUG`, `SIG_RESUME`, `SIG_FFI`, `SIG_PROPAGATE`, `SIG_CANCEL`, `SIG_HALT` |
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
    │       ├─► modify stack/locals/globals
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

Internal VM methods return `SignalBits` (or `(SignalBits, usize)` for the
inner dispatch loop):
- `SIG_OK` (0): Normal completion. Value in `fiber.signal`.
- `SIG_ERROR` (1): Error. Error tuple in `fiber.signal` as `[:keyword "message"]`.
- `SIG_YIELD` (2): Fiber yield. Value in `fiber.signal`, suspended frames in `fiber.suspended`.
- `SIG_RESUME` (8): VM-internal. Fiber primitive requests VM-side execution.
- `SIG_PROPAGATE` (32): VM-internal. `fiber/propagate` re-signals caught signal.
- `SIG_CANCEL` (64): VM-internal. `fiber/cancel` injects error into fiber.
- `SIG_QUERY` (128): VM-internal. Primitive reads VM state (e.g., call counts, global bindings, arena stats, environment).
- `SIG_HALT` (256): Graceful VM termination. Value in `fiber.signal`. Non-resumable; fiber is Dead.

The public `execute_bytecode` method is the translation boundary — it converts
`SignalBits` to `Result<Value, String>` for external callers. On `SIG_ERROR`,
it extracts the error tuple from `fiber.signal` and formats the error message.

Instruction handlers no longer return `Result<(), String>`. VM bugs panic
immediately. User errors set `fiber.signal` to `(SIG_ERROR, error_val(kind, msg))`
and push `Value::NIL` to keep the stack consistent.

## Rc threading

Bytecode and constants are threaded through the dispatch loop as `&Rc<Vec<u8>>`
and `&Rc<Vec<Value>>`. Individual instruction handlers dereference to slices
(`&[u8]`, `&[Value]`). Only the dispatch loop and its direct callees
(`handle_yield`, `handle_call`) need the `Rc` — they clone it cheaply when
creating `SuspendedFrame`s or `TailCallInfo`.

- `execute_bytecode` wraps raw slices in `Rc` once at the public boundary
- `execute_bytecode_from_ip` / `execute_bytecode_saving_stack` take `&Rc`
- `TailCallInfo` is `(Rc<Vec<u8>>, Rc<Vec<Value>>, Rc<Vec<Value>>)` — tail
  calls clone the Rc (cheap), not the Vec (expensive)
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
- `SIG_QUERY` → dispatch to `dispatch_query()`, push result to stack

All SIG_RESUME primitives (including coroutine wrappers) return
`(SIG_RESUME, fiber_value)`. The VM uses `FiberHandle::take()`/`put()` to swap
the child fiber into `vm.fiber`, executes the child, then swaps back.

On resume, the VM wires up the parent/child chain (Janet semantics):
- `parent.child = child_handle` before executing child
- On signal caught (SIG_OK or mask match): clear `parent.child = None`
- On signal NOT caught (propagates): leave `parent.child` set (trace chain)

## Dependents

- `primitives/` - NativeFn primitives; SIG_RESUME signals trigger VM-side execution
- `repl.rs` - runs compiled code
- `main.rs` - file execution

## Invariants

1. **Stack underflow is a VM bug.** Every pop must have a preceding push.
   If you see "Stack underflow," the bytecode or emitter is broken. Handlers
   panic on stack underflow.

2. **Closure environments are immutable Rc<Vec>.** The vec is created at
   closure call time; mutations go through cells, not env modification.

3. **`LocalCell` auto-unwraps on `LoadUpvalue`.** `Cell` (user's `box`) does
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
   immediately. User errors (type mismatch, division by zero) set
   `fiber.signal` to `(SIG_ERROR, error_val(kind, msg))`, push `Value::NIL`
   to keep the stack consistent, and return normally. The dispatch loop checks
   `fiber.signal` for `SIG_ERROR` after each instruction and returns
   immediately. See `set_error()` in `call.rs` and `fiber.rs` for the helper.

## Key VM fields

| Field | Type | Purpose |
|-------|-------|---------|
| `fiber` | `Fiber` | Root fiber: stack, call frames, signal state |
| `current_fiber_handle` | `Option<FiberHandle>` | Handle for current fiber (`None` for root) |
| `current_fiber_value` | `Option<Value>` | Cached NaN-boxed Value for current fiber (`None` for root) |
| `globals` | `Vec<Value>` | Global bindings by SymbolId |
| `defined_globals` | `Vec<bool>` | Tracks which global slots have been assigned (shadows `globals`) |
| `jit_cache` | `FxHashMap<*const u8, Rc<JitCode>>` | JIT code cache (FxHash for pointer keys) |
| `closure_call_counts` | `FxHashMap<*const u8, usize>` | JIT hotness profiling (FxHash for pointer keys) |
| `pending_tail_call` | `Option<TailCallInfo>` | Rc-based tail call info (transient) |
| `env_cache` | `Vec<Value>` | Reusable buffer for `build_closure_env` (avoids alloc per call) |
| `tail_call_env_cache` | `Vec<Value>` | Reusable buffer for `handle_tail_call` env building |
| `eval_expander` | `Option<Expander>` | Cached Expander for runtime `eval` (avoids re-loading prelude) |

### Key Fiber fields (on `vm.fiber`)

| Field | Type | Purpose |
|-------|------|---------|
| `stack` | `SmallVec<[Value; 256]>` | Operand stack |
| `call_stack` | `Vec<CallFrame>` | For stack traces |
| `call_depth` | `usize` | Stack overflow detection |
| `signal` | `Option<(SignalBits, Value)>` | Signal from execution (errors, yields) |
| `suspended` | `Option<Vec<SuspendedFrame>>` | Suspended execution frames (for yield/signal resumption) |
| `heap` | `Box<FiberHeap>` | Per-fiber arena for heap allocation (installed as thread-local during child execution) |
| `signal_mask` | `SignalBits` | Which signals this fiber catches |
| `param_frames` | `Vec<Vec<(Value, Value)>>` | Parameter binding frames (stack of frames, each frame is vec of (param, value) pairs) |
| `parent` | `Option<WeakFiberHandle>` | Weak back-pointer to parent fiber |
| `parent_value` | `Option<Value>` | Cached NaN-boxed Value for parent (identity-preserving) |
| `child` | `Option<FiberHandle>` | Strong pointer to child fiber |
| `child_value` | `Option<Value>` | Cached NaN-boxed Value for child (identity-preserving) |

## Suspension mechanism

When a fiber suspends (via yield instruction or `fiber/signal`):

1. **Yield instruction** (`handle_yield`): captures innermost frame as a
   `SuspendedFrame` with bytecode (Rc clone), constants (Rc clone), env
   (Rc clone), IP (after yield), and operand stack. Stored in `fiber.suspended`.
2. **Call handler** (if yield propagates through a call): appends caller's
   frame to `fiber.suspended` vec.
3. **Signal suspension** (`fiber/signal`): single `SuspendedFrame` with empty
   stack, stored in `fiber.suspended` by the resume handler.
4. **Frame ordering**: innermost (yielder/signaler) at index 0, outermost
   (caller) at last index.
5. **Resume** (`resume_suspended`): iterates frames forward, calling
   `execute_bytecode_from_ip` for each. Handles re-yields and errors.

Key methods:
- `execute_bytecode_from_ip`: Executes from a given IP with Rc bytecode/constants
- `execute_bytecode_saving_stack`: Saves/restores caller's stack and active_allocator, handles tail calls
- `resume_suspended`: Replays `Vec<SuspendedFrame>`, handles re-yields and errors
- `with_child_fiber`: Shared swap protocol for fiber resume/cancel. Also
  manages per-fiber heap routing: saves the current thread-local heap pointer,
  installs the child fiber's `FiberHeap`, executes, then restores the saved
  pointer on swap-back. Root fibers have no heap installed (allocate to the
  global `HEAP_ARENA`); only child fibers get per-fiber heap routing.
  For yielding fibers (`Effect::Yields`), also provisions a shared allocator
  via a three-way branch (step 3b): (a) parent has shared_alloc → propagate
  down, (b) root parent → child creates its own, (c) non-root parent →
  create on parent's heap. Cleared on swap-back (step 7a).

## Allocation region instructions

`RegionEnter` and `RegionExit` push/pop scope marks on the current FiberHeap
via `region_enter()`/`region_exit()`. No-op for the root fiber (no FiberHeap
installed). The lowerer gates emission on escape analysis — currently maximally
conservative, so no region instructions are emitted in normal code.

## Active allocator pointer

`FiberHeap` has an `active_allocator: *const bumpalo::Bump` pointer that currently
always points to the fiber's root bump. Supports future scope-level redirection.

**Initialization:** `init_active_allocator()` is called in `with_child_fiber` after
the child's heap is installed as thread-local (pointer stability requires the heap
to be in its final Box location).

**Save/restore on Call/Return:** `execute_bytecode_saving_stack` saves the active
allocator pointer before execution and restores it after, so callee scope changes
don't leak into the caller's context.

**Save/restore on Yield/Resume:** `SuspendedFrame` carries `active_allocator` so the
pointer can be restored when a fiber resumes. All construction sites write the current
value via `save_active_allocator()`.

**Fiber swap:** `with_child_fiber` handles heap swapping. Each fiber's `active_allocator`
lives on its own `FiberHeap`, so fiber transitions naturally switch allocator context.

**Root fiber:** Has no FiberHeap installed. `save_active_allocator()` returns null,
`restore_active_allocator()` is a no-op.

## Fiber heap routing

Child fibers each own a `Box<FiberHeap>` (on the `Fiber` struct). When the
VM swaps to a child fiber via `with_child_fiber`, it installs the child's
heap as the thread-local allocation target. All `Value::cons()`, `Value::closure()`,
etc. calls during child execution route to the child's `FiberHeap` instead of
the global `HEAP_ARENA`. On swap-back, the parent's heap (or null for root)
is restored.

`FiberHeap` uses bumpalo for bump allocation. Destructor tracking ensures
`HeapObject` variants with inner heap allocations (`Vec`, `Rc`, `BTreeMap`,
`Box<str>`) have their `Drop` impls called on `release()` and `clear()`.
The bump itself only fully resets on `clear()` (fiber death); partial
`release()` runs destructors but does not reclaim bump memory (bumpalo has
no partial reset).

The root fiber does NOT install a heap. This is intentional: `execute_bytecode`
returns `Value`s that outlive the VM. If the root fiber's allocations went to
a `FiberHeap` owned by the VM, those Values would dangle after `VM::drop()`.
Root fiber allocations go to `HEAP_ARENA` (thread-local, outlives any VM).

`reset_fiber()` in `core.rs` extracts, clears, and reuses the heap `Box` to
maintain pointer stability (the thread-local stores a raw pointer to the heap).

## Shared allocator provisioning

When `with_child_fiber` swaps in a yielding child fiber, step 3b provisions
a `SharedAllocator` for zero-copy value exchange. The child's `FiberHeap`
receives a raw `*mut SharedAllocator` pointer — all allocations during child
execution route to this shared allocator instead of the child's private bump.

**Three-way branch** (after swap, `self.fiber` = child, `child_fiber` = parent):

1. **Parent has shared_alloc** (case a): Parent already received a shared_alloc
   from its own parent (A→B→C chain). Propagate the same pointer down.
2. **Root parent** (case b, `saved_heap.is_null()`): Root fiber has no FiberHeap.
   Child creates the shared allocator on its own FiberHeap's `owned_shared`.
3. **Non-root parent, no shared_alloc** (case c): Create a new shared allocator
   on the parent's FiberHeap's `owned_shared`.

**Effect gate (M1)**: Only fibers whose closure has `Effect::Yields` (checked
via `self.fiber.closure.effect.may_yield()`) get shared allocators. Non-yielding
fibers skip step 3b entirely.

**Per-resume creation (M2 tech debt)**: Each resume of a yielding child creates
a new shared allocator because `shared_alloc` was nulled on the previous
swap-back. Old shared allocators accumulate in `owned_shared` until the owner's
`FiberHeap::clear()`. Optimization (reuse across resumes) is deferred.

**Cleanup (step 7a)**: Before swap-back, `self.fiber.heap.clear_shared_alloc()`
nulls the child's `shared_alloc` pointer. The shared allocator data remains
alive in the owner's `owned_shared` Vec.

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

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~100 | VM struct, VmResult, public interface |
| `dispatch.rs` | ~373 | Main execution loop, instruction dispatch, returns `(SignalBits, usize)` |
| `call.rs` | ~823 | Call, TailCall, JIT dispatch (solo + batch), environment building |
| `signal.rs` | ~177 | Primitive signal dispatch (`handle_primitive_signal`), SIG_QUERY dispatch |
| `fiber.rs` | ~555 | Fiber resume/propagate/cancel, shared swap protocol, shared alloc provisioning |
| `execute.rs` | ~147 | `execute_bytecode_from_ip`, `execute_bytecode_saving_stack` |
| `core.rs` | ~456 | VM struct, `resume_suspended`, stack trace helpers |
| `stack.rs` | ~100 | Stack operations: LoadConst, Pop, Dup |
| `variables.rs` | ~150 | LoadGlobal, StoreGlobal, LoadUpvalue, etc. |
| `parameters.rs` | ~50 | Parameter resolution: `resolve_parameter` helper |
| `control.rs` | ~100 | Jump, JumpIfFalse, Return |
| `closure.rs` | ~100 | MakeClosure |
| `arithmetic.rs` | ~150 | Add, Sub, Mul, Div |
| `comparison.rs` | ~100 | Eq, Lt, Gt, Le, Ge |
| `types.rs` | ~50 | IsNil, IsEmptyList, IsPair, Not |
| `data.rs` | ~100 | Cons, Car, Cdr, MakeVector |
| `literals.rs` | ~18 | Nil, EmptyList, True, False literal handlers |
| `eval.rs` | ~180 | Runtime eval: compile+execute datum, env wrapping |
| `cell.rs` | ~70 | Cell operations: MakeCell, UnwrapCell, UpdateCell |

## Truthiness

The VM evaluates truthiness via `Value::is_truthy()`:
- `Value::NIL` → falsy
- `Value::FALSE` → falsy  
- Everything else (including `Value::EMPTY_LIST`, `Value::int(0)`) → truthy

The `Instruction::Nil` pushes `Value::NIL` (falsy).
The `Instruction::EmptyList` pushes `Value::EMPTY_LIST` (truthy).
