# Fiber Architecture

<!-- audited: 2026-09-28 -->

Fibers are Elle's unified control-flow mechanism.

## The Fiber

A `Fiber` ([src/value/fiber.rs](../../src/value/fiber.rs)) is an independent
execution context. These fields decide how a fiber behaves toward the fibers
around it:

- **The operand stack.** Each activation's locals sit beneath its operands.
- **`callers`.** The caller activations that wait while an interpreted callee
  runs. It is empty whenever the fiber is parked.
- **`status`.** One of the five statuses below.
- **`mask`.** Which of this fiber's signals its parent catches. The parent
  sets it at creation, and it never changes.
- **`parent` and `child`.** A weak back-pointer and a strong pointer to the
  most recently resumed child. Each has a cached `Value`, so `fiber/parent`
  and `fiber/child` answer the same fiber value every time.
- **`signal`.** The bits and payload of the last signal, or the return value.
- **`suspended`.** The parked frame chain that a resume replays.
- **`delivery`.** The ledger of how the current park's references are funded
  ([park.md](../impl/region/park.md)).
- **`withheld`.** The capabilities the fiber may not use
  ([capabilities.md](capabilities.md)).
- **`fuel`.** The instruction budget ([runtime.md](../runtime.md)).

[vm.md](../impl/vm.md) describes the rest: the region-remap frames, the trace
frames, and the executing-closure register.

### FiberHandle

`FiberHandle` wraps `Rc<RefCell<Option<Fiber>>>`. The `Option` makes "fiber
is currently executing on the VM" representable as `None`.

- `take()` — extract the fiber (sets slot to None)
- `put()` — return the fiber (sets slot to Some)
- `with()`/`with_mut()` — borrow in-place for read/write
- `try_with()`/`try_with_mut()` — the same, answering `None` for an
  executing fiber

`WeakFiberHandle` wraps `Weak<RefCell<Option<Fiber>>>` for parent
back-pointers, avoiding Rc cycles.

### FiberStatus

| Status | Meaning |
|--------|---------|
| `New` | Created but never resumed |
| `Alive` | Currently executing on the VM |
| `Paused` | Waiting for a resume: it yielded or emitted, or its mask caught its error |
| `Dead` | Completed, halted, or cancelled. It never runs again |
| `Error` | Stopped by an error its mask did not catch, or by `fiber/abort` |

A resume restarts an `Error` fiber at the call that raised
([primitives.md](primitives.md)).

## Signals

Signal types are bit positions in a 64-bit mask. User-facing signals
use keywords in set literals for fiber masks:

| Keyword | Bit | Purpose |
|---------|-----|---------|
| `:error` | 0 | Error propagation |
| `:yield` | 1 | Cooperative suspension |
| `:debug` | 2 | Breakpoint / trace |
| `:ffi` | 4 | Calls foreign code |
| `:halt` | 8 | VM termination |
| `:io` | 9 | I/O request to scheduler |
| `:exec` | 11 | Subprocess execution |
| `:fuel` | 12 | Instruction budget exhaustion |

Bits 15–17 name the capabilities `:gpu`, `:os-signal` and `:fs`. The other
bits below 32 are internal to the VM or reserved, and bit 6 is unused —
[runtime.md](../runtime.md) has the full table. Bits 32–63 are for
user-defined signals (via `(signal :keyword)`) — see
[protocol.md](protocol.md).

The terminal signal (bit 10) is **uncatchable** — it passes through all
mask checks. A self-cancel uses it to end the fiber at once.

### Signal emission

When code emits a signal (`emit`):

1. Signal value stored on the fiber
2. Fiber suspends
3. Parent checks: does the mask include this signal?
   - **Caught**: parent handles the signal
   - **Not caught**: parent also suspends, signal propagates up the chain

### Signal mask

The mask on a fiber determines which of its signals the parent catches.
Set at creation time, immutable after. The **caller** decides what to
handle, not the callee.

```lisp
(defn my-fn [] 42)

# Create a fiber that catches errors from its closure
(fiber/new my-fn |:error|)

# Create a fiber that catches yields
(fiber/new my-fn |:yield|)

# Create a fiber that catches both
(fiber/new my-fn |:error :yield|)
```

## Suspension and Resumption

### SuspendedFrame

A `SuspendedFrame` ([src/value/fiber/frame.rs](../../src/value/fiber/frame.rs))
is one step of a parked chain, and has two kinds:

- **`Bytecode`** — a `BytecodeFrame`: the code object, the environment, the
  offset to resume at, and the operand stack, with the activation's region
  remap and what it owed at suspension. `push_resume_value` says whether the
  resume value goes on the stack before the frame runs.
- **`FiberResume`** — a sub-fiber to resume first. A `defer` or `protect`
  body runs in a sub-fiber, and when that sub-fiber's signal passes it, the
  outer fiber parks this frame so the resume reaches the sub-fiber first.

### How a fiber suspends

Every suspension builds a chain of frames. The signalling activation parks
its own frame with its operand stack: `handle_emit` does this for an `emit`,
and `handle_primitive_signal` for a suspending primitive in call position.
A signal that leaves no frame of its own — a suspend in tail position, a
fuel pause, an error — has its frame parked by the driver it returns to. As
a suspending signal leaves each callee, its paused caller parks a frame
behind it. A
fuel pause parks with `push_resume_value` false, so the paused instruction
runs again with the stack as it was.

### Frame ordering

Innermost (yielder/signaler) at index 0, outermost (caller) at last index.
On resume, frames are replayed forward: index 0 first, last index last.

### resume_suspended

`VM::resume_suspended` replays the frame chain:

1. For each frame: restore its stack, push the value from the previous
   frame (or the resume value for the innermost), execute from the saved IP
2. On `SIG_OK`: extract the result, pass it to the next frame
3. On any other signal: park the frame that stopped, keep the outer frames
   behind it, and return the signal bits. A halt keeps nothing, because a
   halted fiber never runs again

### Resume value destination

When a suspended fiber is resumed with a value, the value is pushed onto
the fiber's operand stack. The IP points to the instruction *after* the
signal, so execution continues as if the signal expression evaluated to
the resume value.

A fiber stopped on an error parks its innermost frame after the call that
raised, and the resume value takes the place of that call's result.
[primitives.md](primitives.md) shows where a restart lands.

When `fiber/resume` returns to the parent, the child's signal value is
pushed onto the parent's operand stack. Use `fiber/status` to check
whether the child completed normally, errored, or suspended. Use
`fiber/value` to read the signal payload.

## Parent/Child Chain

Fibers form a chain via `parent` (weak) and `child` (strong) pointers.

- `parent.child = child_handle` is set before executing the child
- On signal caught (SIG_OK or mask match): `parent.child = None`
- On signal NOT caught (propagates): `parent.child` stays set (trace chain)

The `child` field tracks the most recently resumed child, not all children.
It's set on resume and cleared on completion or when a different child is
resumed.

### Signal propagation

When a child signals and the parent's mask doesn't catch it, the signal
continues past the parent's `fiber/resume` call. Each fiber in the chain
stops and stays inspectable. Walk `fiber/child` to find the originating
fiber.

- **A suspending signal** parks each fiber it passes. The fiber parks a
  `FiberResume` frame for the child ahead of its own frames, and its
  `signal` holds the same bits and payload.
- **An error** stops the child `:error`, and stops each fiber it passes at
  that fiber's `fiber/resume` call. Each of those fibers' own masks decides
  whether the error goes further, and a restart of one answers its
  `fiber/resume` call.

## Signal System Integration

Closures carry a `Signal` with signal bits describing what they might emit.
The fiber's mask determines which signals are caught. The signal system is
compile-time; runtime signals are runtime events. Same bitfield, different timing.

See [index.md](index.md) for the signal system design.

---

## See also

- [Signal index](index.md)
- [Fiber primitives](primitives.md)
- [Processes](../processes.md) — Erlang-style processes, GenServer, and supervisors built on fibers
