# Fibers: Architecture Reference

Fibers are Elle's unified control-flow mechanism. A fiber is an independent
execution context with its own stack, call frames, and signal state. All
non-local control flow — errors, yields, coroutines, cancellation — is
implemented as signals propagating through a fiber chain.

This document describes the implemented system.


## The Fiber

```rust
pub struct Fiber {
    pub stack: SmallVec<[Value; 256]>,       // operand stack
    pub frames: Vec<Frame>,                   // call frames (closure + ip + base)
    pub status: FiberStatus,                  // New, Alive, Suspended, Dead, Error
    pub mask: SignalBits,                     // which signals parent catches from this fiber
    pub parent: Option<WeakFiberHandle>,      // weak back-pointer (avoids Rc cycles)
    pub parent_value: Option<Value>,          // cached NaN-boxed Value for parent
    pub child: Option<FiberHandle>,           // most recently resumed child
    pub child_value: Option<Value>,           // cached NaN-boxed Value for child
    pub closure: Rc<Closure>,                 // the closure this fiber wraps
    pub env: Option<HashMap<u32, Value>>,     // dynamic bindings (future)
    pub signal: Option<(SignalBits, Value)>,  // signal payload or return value
    pub suspended: Option<Vec<SuspendedFrame>>, // frames for resumption
    pub call_depth: usize,                    // stack overflow detection
    pub call_stack: Vec<CallFrame>,           // for stack traces
}
```

The `parent_value` and `child_value` fields cache the NaN-boxed `Value`
wrapping the handle, so `fiber/parent` and `fiber/child` return
identity-preserving values without re-allocating heap objects.

### FiberHandle

`FiberHandle` wraps `Rc<RefCell<Option<Fiber>>>`. The `Option` makes "fiber
is currently executing on the VM" representable as `None`.

- `take()` — extract the fiber (sets slot to None)
- `put()` — return the fiber (sets slot to Some)
- `with()`/`with_mut()` — borrow in-place for read/write

`WeakFiberHandle` wraps `Weak<RefCell<Option<Fiber>>>` for parent
back-pointers, avoiding Rc cycles.

### FiberStatus

| Status | Meaning |
|--------|---------|
| `New` | Created but never resumed |
| `Alive` | Currently executing on the VM |
| `Suspended` | Waiting for resume (signaled or yielded) |
| `Dead` | Completed normally |
| `Error` | Terminated by unhandled error |


## Signals

Signal types are bit positions in a `u32` bitmask:

| Bit | Constant | Value | Purpose |
|-----|----------|-------|---------|
| — | `SIG_OK` | 0 | Normal return (no bits set) |
| 0 | `SIG_ERROR` | 1 | Error |
| 1 | `SIG_YIELD` | 2 | Cooperative suspension |
| 2 | `SIG_DEBUG` | 4 | Breakpoint / trace |
| 3 | `SIG_RESUME` | 8 | VM-internal: fiber resume request |
| 4 | `SIG_FFI` | 16 | Calls foreign code |
| 5 | `SIG_PROPAGATE` | 32 | VM-internal: re-raise caught signal |
| 6 | `SIG_CANCEL` | 64 | VM-internal: inject error into fiber |
| 7–15 | — | — | Reserved |
| 16–31 | — | — | User-defined signal types |

Bits 0–2 are user-facing. Bits 3–6 are VM-internal. Bits 16–31 are for
user-defined signal types.

### Signal emission

When code emits a signal (`fiber/signal`):

1. Signal value stored in `fiber.signal`
2. Fiber status → `Suspended`
3. Dispatch loop returns `(SignalBits, ip)` to the caller
4. Parent checks: `child.mask & bits != 0`?
   - **Caught**: parent handles the signal
   - **Not caught**: parent also suspends, signal propagates up the chain

### Signal mask

The mask on a fiber determines which of its signals the parent catches.
Set at creation time, immutable after. The **caller** decides what to
handle, not the callee.

```lisp
;; Create a fiber that catches errors from its closure
(fiber/new my-fn 1)  ; mask = SIG_ERROR

;; Create a fiber that catches yields
(fiber/new my-fn 2)  ; mask = SIG_YIELD

;; Create a fiber that catches both
(fiber/new my-fn 3)  ; mask = SIG_ERROR | SIG_YIELD
```


## Suspension and Resumption

### SuspendedFrame

```rust
pub struct SuspendedFrame {
    pub bytecode: Rc<Vec<u8>>,      // Rc clone, not data copy
    pub constants: Rc<Vec<Value>>,  // Rc clone, not data copy
    pub env: Rc<Vec<Value>>,        // closure environment
    pub ip: usize,                  // instruction pointer to resume at
    pub stack: Vec<Value>,          // operand stack (empty for signal suspension)
}
```

`SuspendedFrame` captures everything needed to resume bytecode execution.
It replaces the former `SavedContext` and `ContinuationFrame` types with a
single representation. Bytecode and constants are `Rc` clones — no data
copying on suspension.

### Two suspension modes

**Signal suspension** (`fiber/signal`): single `SuspendedFrame` with empty
stack. The fiber's own operand stack is preserved in place.

**Yield suspension** (`yield` instruction): chain of `SuspendedFrame`s from
the yielder to the coroutine boundary. Each frame captures its operand stack.
When yield propagates through Call instructions, each caller's frame is
appended to the chain.

### Frame ordering

Innermost (yielder/signaler) at index 0, outermost (caller) at last index.
On resume, frames are replayed forward: index 0 first, last index last.

### resume_suspended

`VM::resume_suspended(frames, resume_value) -> SignalBits` replays the
frame chain:

1. For each frame: restore its stack, push the value from the previous
   frame (or the resume value for the innermost), execute from the saved IP
2. On `SIG_OK`: extract the result, pass it to the next frame
3. On non-OK signal: save context for potential future resume, merge
   remaining outer frames for yield signals, return the signal bits

### Resume value destination

When a suspended fiber is resumed with a value, the value is pushed onto
the fiber's operand stack. The IP points to the instruction *after* the
signal, so execution continues as if the signal expression evaluated to
the resume value.

When `fiber/resume` returns to the parent, the child's signal value is
pushed onto the parent's operand stack. Use `fiber/status` to check
whether the child completed normally, errored, or suspended. Use
`fiber/value` to read the signal payload.


## Rc Threading

Bytecode and constants flow through the dispatch loop as `&Rc<Vec<u8>>`
and `&Rc<Vec<Value>>`. This eliminates data copying:

- `execute_bytecode` wraps raw slices in `Rc` once at the public boundary
- `execute_bytecode_from_ip` / `execute_bytecode_coroutine` take `&Rc`
- `TailCallInfo` is `(Rc<Vec<u8>>, Rc<Vec<Value>>, Rc<Vec<Value>>)` —
  tail calls clone the Rc (cheap), not the Vec
- `handle_yield` / `handle_call` clone the Rc into `SuspendedFrame`
- Individual instruction handlers dereference to `&[u8]` / `&[Value]` —
  they don't need the Rc

The inner dispatch loop returns `(SignalBits, usize)` — signal bits and
the IP at exit. This eliminates the former `suspended_ip` staging field.


## The VM

```rust
pub struct VM {
    pub fiber: Fiber,                          // currently executing fiber (owned)
    pub current_fiber_handle: Option<FiberHandle>, // handle if from fiber/new
    pub globals: Vec<Value>,                   // global bindings (shared)
    pub ffi: FFISubsystem,                     // FFI subsystem (shared)
    pub modules: HashMap<String, HashMap<u32, Value>>,
    pub jit_cache: HashMap<*const u8, Rc<JitCode>>,
    pub pending_tail_call: Option<TailCallInfo>, // transient, never crosses suspension
    // ... other shared state
}
```

The VM owns the currently executing fiber directly — no `Rc`/`RefCell`
overhead in the hot path. Suspended fibers are wrapped in `FiberHandle`
when stored as values. On resume, the child is swapped in via
`FiberHandle::take()`; on suspend, swapped out via `FiberHandle::put()`.


## Parent/Child Chain

Fibers form a chain via `parent` (weak) and `child` (strong) pointers.

- `parent.child = child_handle` is set before executing the child
- On signal caught (SIG_OK or mask match): `parent.child = None`
- On signal NOT caught (propagates): `parent.child` stays set (trace chain)

The `child` field tracks the most recently resumed child, not all children.
It's set on resume and cleared on completion or when a different child is
resumed.

### Signal propagation

When a child signals and the parent's mask doesn't catch it, the parent
also suspends. The entire chain from signaler to eventual handler freezes.
Each fiber in the chain is suspended and inspectable.

Walk `fiber/child` to find the originating fiber. The originator's
`fiber/value` has the payload; intermediaries store `(bits, NIL)`.


## Fiber Primitives

| Primitive | Signature | Purpose |
|-----------|-----------|---------|
| `fiber/new` | `(fn mask) → fiber` | Create fiber from closure with signal mask |
| `fiber/resume` | `(fiber value) → value` | Resume fiber, delivering a value; returns signal value |
| `fiber/signal` | `(bits value) → (suspends)` | Emit signal from current fiber |
| `fiber/status` | `(fiber) → keyword` | `:new`, `:alive`, `:suspended`, `:dead`, `:error` |
| `fiber/value` | `(fiber) → value` | Signal payload or return value |
| `fiber/bits` | `(fiber) → int` | Signal bits from last signal |
| `fiber/mask` | `(fiber) → int` | Capability mask |
| `fiber/parent` | `(fiber) → fiber\|nil` | Parent fiber |
| `fiber/child` | `(fiber) → fiber\|nil` | Most recently resumed child |
| `fiber/propagate` | `(fiber) → (propagates)` | Re-raise caught signal, preserve chain |
| `fiber/cancel` | `(fiber value) → value` | Inject error into suspended fiber |
| `fiber?` | `(value) → bool` | Type predicate |

All fiber primitives are `NativeFn: fn(&[Value]) -> (SignalBits, Value)`.
Primitives that need VM-side execution (`fiber/resume`) return
`(SIG_RESUME, fiber_value)` and the VM dispatch loop handles the context
switch.


## Coroutines

A coroutine is a usage pattern, not a type. It's a fiber whose closure
yields:

```lisp
(def gen (fiber/new (fn () (yield 1) (yield 2) (yield 3)) 2))
(fiber/resume gen nil)  ; → SIG_YIELD, (fiber/value gen) → 1
(fiber/resume gen nil)  ; → SIG_YIELD, (fiber/value gen) → 2
(fiber/resume gen nil)  ; → SIG_YIELD, (fiber/value gen) → 3
(fiber/resume gen nil)  ; → SIG_OK, (fiber/value gen) → nil
```

Legacy coroutine primitives (`coro/new`, `coro/resume`,
`coro/done?`) are thin wrappers around fiber operations.


## Error Handling

Errors are values: `[:keyword "message"]` tuples. `error_val(kind, msg)`
constructs them; `format_error(value)` extracts human-readable text.

There is no `Condition` type, no exception hierarchy, no `handler-case`.
Error handling is signal handling:

1. Code signals `SIG_ERROR` with an error tuple
2. Signal propagates up the fiber chain
3. A fiber with `SIG_ERROR` in its mask catches it
4. The handler inspects `fiber/value` and decides: handle, resume, or
   propagate further

`try`/`catch` will be sugar for this pattern (blocked on macro system).


## Terminal vs. Resumable Signals

Whether a signal is terminal or resumable is a **handler decision**, not a
signal property. Any signal leaves the child in `Suspended` status. The
handler either:

- **Resumes** the child (delivering a value) → resumable
- **Doesn't resume** (lets it be GC'd) → terminal

An uncaught `SIG_ERROR` at the root fiber is terminal by convention.


## Effect System Integration

Closures carry an `Effect` with signal bits describing what they might emit.
The fiber's mask determines which signals are caught. The effect system is
compile-time; signals are runtime. Same bitfield, different timing.

See `docs/effects.md` for the effect system design.


## What's Not Implemented Yet

| Feature | Status |
|---------|--------|
| `try` macro | Blocked on macro system |
| `fiber/closure`, `fiber/stack`, `fiber/env` | Not started |
| Dynamic bindings (`dyn`/`setdyn`) | `env` field exists, no primitives |
| User-defined signal types (bits 16–31) | Infrastructure exists, no allocation API |
| JIT signal-aware calling convention | JIT still restricted to pure functions |
