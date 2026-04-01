# Signal Protocol

## The Signal Protocol

### Signal Types

Signal types are bit positions in a bitfield. The first 16 are
compiler-reserved:

| Bit | Name | Value | Meaning |
|-----|------|-------|---------|
| — | ok | 0 | Normal return (no bits set) |
| 0 | error | 1 | Error |
| 1 | yield | 2 | Cooperative suspension |
| 2 | debug | 4 | Breakpoint / trace |
| 3 | resume | 8 | VM-internal: fiber resume request |
| 4 | ffi | 16 | Calls foreign code |
| 5 | propagate | 32 | VM-internal: propagate caught signal |
| 6 | abort | SIG_ERROR\|SIG_TERMINAL | VM-internal: graceful fiber termination |
| 7 | query | 128 | VM-internal: read VM state |
| 8 | halt | 256 | Graceful VM termination |
| 9 | io | 512 | I/O request to scheduler |
| 10 | terminal | 1024 | Uncatchable — passes through mask checks |
| 11–15 | reserved | — | Future compiler-known signals |
| 16+ | user | — | User-defined signal types |

Bit 0 is special: "ok" means no bits are set. A normal return has an empty
signal bitfield.

`SIG_RESUME` is how `fiber/resume` works without VM access. The
primitive returns `(SIG_RESUME, fiber_value)` and the VM dispatch
loop performs the actual context switch.

### Signal Values

A signal carries a type (which bit) and a payload (an Elle Value). The
return from `run()` is:

```
signal_bits: SignalBits
```

Where `signal_bits == 0` means normal return with the result on the fiber's
operand stack. Non-zero `signal_bits` means something happened that may require
handling — the signal value is stored in `fiber.signal` (the canonical location).

**Signal payloads are arbitrary Values.** Any value can be an error payload,
a yield value, or a user-defined signal payload. There is no Condition type
or exception hierarchy. Pattern matching on the payload replaces hierarchy
checks — the handler inspects the signal value and dispatches accordingly.

At the Rust level, the signal is stored on the fiber:

```rust
pub signal: Option<(SignalBits, Value)>
```

The `run()` function returns only the `SignalBits`. The value is stored on the
fiber's `signal` field — the canonical location. The fast path (normal return)
is `bits == 0`, which is a single branch.

### Signal Composition

**`SignalBits` is a pure bitmask. Every bit is independent and orthogonal.**
There are no "types" of signals — only bits. The VM and schedulers check for
individual bits using `contains()`, never with equality. Any combination of
bits is valid and meaningful — the caller decides what the combination means.

Examples of valid composed signals:

- `|:yield|` — suspend, return a value to the caller
- `|:yield :io|` — suspend AND request I/O; the scheduler sees both bits and
  handles accordingly
- `|:io :error|` — I/O error; a scheduler might log it and halt
- `|:io :error :halt|` — I/O error, halt the VM; the scheduler interprets all
  three bits
- `|:yield :audit|` — suspend AND emit an audit signal; a monitoring fiber
  catches both

No bit has a predetermined relationship with any other bit. The design makes
no pre-determinations on how bits are mixed. Users and schedulers define the
semantics of combinations.

**Fiber masks work the same way.** A fiber mask like `|:yield :io|` catches
fibers that have either bit set. The mask is a bitmask, not an enum.

**User-defined signals (bits 16–31) compose freely** with built-in bits. A
user-defined signal can be combined with `:yield`, `:error`, `:io`, or any
other bit.

### Terminal vs Resumable Signals

**Whether a signal is terminal or resumable is a handler decision, not a
signal property.** The handler catches the signal and either resumes the child
(resumable) or doesn't (terminal). The same signal type can be handled
resumably in one context and terminally in another, depending on the handler's
choice.

### Propagation

Signal propagation (Janet model):

1. Child fiber emits signal: stores value in `child.signal`, sets status → Suspended
2. `run()` returns signal bits to the parent
3. Parent checks: `child.mask & bits != 0`?
   (The child's mask records what signals the parent should catch from it)
    - **Caught**: Parent handles the signal. Child is suspended and
      reachable via parent's `child` pointer.
    - **Not caught**: Parent also suspends (entire chain freezes).
      Signal propagates up until caught or reaches root.
4. Handler walks `child` chain to find originator. Every fiber in the
   chain is suspended and inspectable via `fiber/value`.

This is O(1) dispatch — a single AND operation. No handler chain traversal.
When a handler catches a signal, it can walk the fiber chain to inspect the
propagation path. Every fiber in the chain is suspended and can be resumed
independently for non-unwinding recovery.

### The Fiber Structure

```
Fiber {
    stack: SmallVec<[Value# 256]>           -- operand stack
    frames: Vec<Frame>                       -- call frames (closure + ip + base)
    status: FiberStatus                      -- New/Alive/Suspended/Dead/Error
    mask: SignalBits                          -- which signals parent catches
    parent: Option<WeakFiberHandle>          -- weak back-pointer (avoids Rc cycles)
    child: Option<FiberHandle>               -- most recently resumed child
    closure: Rc<Closure>                     -- the closure this fiber wraps
    env: Option<HashMap<u32, Value>>         -- dynamic bindings (future)
    signal: Option<(SignalBits, Value)>       -- signal payload or return value
    suspended: Option<Vec<SuspendedFrame>>   -- frames for resumption
    call_depth: usize                        -- stack overflow detection
    call_stack: Vec<CallFrame>               -- for stack traces
}
```

The closure carries its signal bits. The fiber's mask determines which
signals it catches from children. There is no `signals` field on the Fiber
— signals are a compile-time property of the closure, not the fiber.

See `docs/fibers.md` for the full Fiber, SuspendedFrame, and FiberHandle
documentation.



## The Signal System

### Signal Bits (Static)

A signal (static) is a set of signal types that a function might emit. Represented
as a bitfield (same type as signal bits).

```
type SignalBits = SignalBits  -- same bitfield type, same bit positions
```

Operations:

- **Combine**: `a | b` (union — a block's signal is the union of its parts)
- **Check**: `actual & ~permitted == 0` (subset — are all actual signals permitted?)
- **Silent**: `bits == 0` (no signals)
- **Has**: `bits & YIELD != 0` (membership test)

### Compile-Time Inference

The compiler walks the AST and infers signals:

- A literal is silent (no bits)
- A primitive has known signal bits (declared at registration)
- A call's signal is the callee's signal combined with the call overhead
- A `begin` block's signal is the union of its children
- A lambda's body signal is stored on the lambda but the lambda itself is silent
- A handler that catches signal X removes bit X from the enclosed expression's
  signal

### Parametric Polymorphism

Higher-order functions propagate their arguments' signals. `map`'s signal is
"whatever `f` does, plus my own base signals." The compile-time
representation:

```
Signal {
    bits: SignalBits,           -- from own body
    propagates: u32,            -- bitmask of parameter indices
}
```

Resolved signal at a call site:

```
call_signal = f.bits | union(signal(arg[i]) for i in 0..param_count if (propagates & (1 << i)) != 0)
```

If the compiler can see the concrete argument (e.g., it's the `+` primitive),
it can resolve the polymorphism statically and potentially prove the call site
has fewer signals than the general case.

**Note**: Signal bounds on parameters (constraining what signals callbacks may
have) are deferred to a future phase. When needed, they'll be tracked in the
analysis environment, not on the Signal struct itself — keeping Signal as a
simple Copy pair.

### Signal Restrictions

The programmer can restrict signals on functions using `silence` (compile-time total suppression):

```lisp
# Require the function body to be completely silent
(defn add (x y)
  (silence)
  (+ x y))
```

And signal bounds on parameters:

```lisp
(defn fast-map (f xs)
  (silence f)   # f must be completely silent
  (map f xs))
```

These are compile-time contracts. The system enforces them statically and at runtime.

### Runtime Signal Enforcement with squelch

`squelch` is a **runtime closure transform primitive** that takes a closure and returns a new closure with runtime signal enforcement:

```lisp
(defn f [] (yield 42))

# squelch as a primitive function (not a preamble declaration)
(def safe-f (squelch f :yield))

# Composable: squelch on top of squelch
(def f2 (squelch (squelch f :yield) :io))
```

When a squelched closure is called, if it emits a squelched signal, a `signal-violation` error is raised instead. Non-squelched signals pass through normally. Errors are never affected by squelch (they pass through unchanged).

**Signature:** `(squelch closure :kw1 :kw2 ...)`
- **First argument:** must be a closure
- **Remaining arguments:** signal keywords to squelch (at least one required)
- **Returns:** a new closure with the squelch mask applied
- **Signal:** `Signal::errors()` (can error on bad arguments, otherwise silent)
- **Arity:** `AtLeast(2)` — closure + at least one keyword

**Error cases:**
- `(squelch f)` with no keywords → arity error
- `(squelch non-closure :yield)` → type error
- `(squelch f :unknown-signal)` → error (signal not registered)



## I/O Signals

### SIG_IO and the Scheduler

I/O signals use the `:io` signal bit (bit 9). A fiber performing I/O signals
`|:yield :io|` because it wants to both suspend AND request I/O handling.
This is a convention, not a language rule — the bits compose freely.

**Signal bit**: Bit 9 (`SIG_IO = 1 << 9`)

**Signal constructors**:
- `Signal::io()` — function may perform I/O
- `Signal::io_errors()` — function may perform I/O and may error

**Predicate**: `may_io()` — check if signal includes I/O

### Stream Primitives and I/O Requests

Stream primitives (`port/read-line`, `port/read`, `port/read-all`,
`port/write`, `port/flush`) have signal `io_errors()`. They do not
perform I/O themselves. Instead, they:

1. Build an `IoRequest` (typed descriptor of the I/O operation)
2. Return `(|:yield :io|, request)` to suspend the fiber and signal I/O
3. Let the scheduler catch the fiber (because `:yield` is in its mask), see
   the `:io` bit, and dispatch the `IoRequest` payload to a backend

The backend (`SyncBackend` in Phase 3) performs the actual I/O and returns
`(|:ok|, result)` or `(|:error|, error)`. The scheduler resumes the fiber
with the result.

### Signal Composition in I/O

The `:io` bit is just a bit — it has no special relationship with `:yield`.
A fiber can signal:

- `|:yield|` — suspend without I/O
- `|:yield :io|` — suspend and request I/O (the common case)
- `|:io :error|` — I/O error; a scheduler might log it and halt
- `|:yield :io :audit|` — suspend, request I/O, and emit an audit signal

The scheduler's job is to check the bits it cares about. A scheduler that
catches `:yield` will see fibers signaling `|:yield :io|` and can inspect the
`:io` bit to decide how to handle the I/O request. A different scheduler might
catch `|:yield :io|` directly. The semantics of combinations are defined by
the scheduler, not by the language.


## Signal Registry

The signal registry maps signal keywords to bit positions. Built-in signals occupy bits 0–15; user-defined signals use bits 16–31.

### Built-in Signals

| Keyword | Bit | Meaning |
|---------|-----|---------|
| `:error` | 0 | Error signal |
| `:yield` | 1 | Cooperative suspension |
| `:debug` | 2 | Breakpoint/trace |
| `:ffi` | 4 | Calls foreign code |
| `:halt` | 8 | Graceful VM termination |
| `:io` | 9 | I/O request to scheduler |

Bits 3, 5–7 are VM-internal (resume, propagate, query). Bits 10–14 are
VM-internal (terminal, exec, fuel, switch, wait). Bit 15 is reserved.

### User-Defined Signals

User-defined signals are registered via the `(signal :keyword)` special
form and allocated bits 16–31 sequentially. Up to 16 user signals are
supported. Registration happens at analysis time.

```lisp
(signal :heartbeat)
(signal :rate-limit)
# :heartbeat gets bit 16, :rate-limit gets bit 17

# Expression position — returns the keyword
(def my-signal (signal :custom))
my-signal  # => :custom
```

Duplicate registration is a compile-time error. Built-in signal keywords
cannot be re-registered.

---

## See also

- [Signal index](index.md)
