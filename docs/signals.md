# Signals: Design Document

This document captures the design of Elle's unified signal system.
It records not just decisions but the reasoning, alternatives, and open
questions that led to them. Future readers should be able to understand the

## Contents

- [Motivation](#motivation)
- [Prior Art](#prior-art)
- [Terminology](#terminology)
- [The Core Insight](#the-core-insight)
- [Capabilities Down, Signals Up](#capabilities-down-signals-up)
- [The Signal Protocol](#the-signal-protocol)
- [The Signal System](#the-signal-system)
- [JIT Integration](#jit-integration)
- [Surface Syntax](#surface-syntax)
- [Migration Status](#migration-status)
- [Non-Unwinding Recovery](#non-unwinding-recovery)
- [Error Signalling](#error-signalling)
- [Open Questions](#open-questions)
- [Resolved Questions](#resolved-questions)
trade-offs and pick up where we left off.


## Motivation

Elle previously had separate mechanisms for coroutines (continuation
capture/replay), exception handling (handler stack with unwind semantics),
and signal inference (boolean fields for yields and errors). The JIT could
only compile inert functions.

These have been unified into a single mechanism: **fibers with signals**.
Coroutines are fibers that yield. Errors are signals. The signal system
tracks signal bits. See `docs/fibers.md` for the implementation reference.


## Prior Art

### Janet

Janet unifies coroutines, error handling, generators, dynamic scoping, and
green threads into a single primitive: the **fiber**. Every control flow event
is a **signal** (an integer 0–13) propagating up a chain of fibers. A fiber's
**signal mask** (a bitmask set at creation time) determines which signals it
catches from its children.

Key insights from Janet:

- **Signals over exceptions.** All non-local control flow is a numbered signal.
  Error is signal 1. Yield is signal 3. User-defined signals extend the space.
  Dispatch is a single bitmask check.

- **Masks over handlers.** Instead of a handler chain with runtime dispatch,
  a bitmask determines catch-or-propagate at O(1). Branch-predictor-friendly.

- **Fibers over try/catch.** `try`/`catch` is sugar for "create a fiber that catches
   errors, resume it, check the result." No special VM support.

- **Declarations at instantiation.** The signal mask lives on the fiber, set
  at creation time. The *caller* decides what to handle, not the function.
  Functions are colorless# fibers are colored.

- **Composition over special forms.** `try`, `catch`, `finally`,
   `generate` — all macros over `fiber/new` + `resume` + `fiber/status` +
   `propagate`. One runtime primitive# the language provides sugar.

Janet's limitation: signals are a single integer (one thing happened), and
there's no static tracking of signals. You can't look at a function and know
what signals it might emit. Optimization opportunities that depend on static
knowledge are unavailable.

See `docs/janet.md` for the full architectural analysis.

### Koka

Koka has row-polymorphic effects in the type system. A function's type
includes its effect row, and higher-order functions are automatically
polymorphic over their arguments' effects. Effect handlers are the elimination
form — they remove an effect from the row.

What works: row polymorphism means `map` just works — its effect is
automatically "whatever `f` does." What doesn't: type signatures get noisy,
and when inference fails, error messages are brutal.

Koka's effects are purely compile-time — they erase at runtime. Handlers
compile to delimited continuations or CPS.

### OCaml 5

Effects are runtime-only. You perform an effect, the nearest handler catches
it. No static tracking. Maximally flexible, zero compile-time guarantees.

### Rust

No effect system per se, but trait bounds (`Send`, `Sync`, `async`) encode
effect-like properties. Each effect has its own mechanism. The "function
coloring" problem with async is exactly what happens when you hardcode one
effect into the language.

### Nim (cautionary tale)

Nim's `sink` annotation means "I'd love to own this parameter and save you a
copy." But it's a hint, not a guarantee. The compiler may or may not be able
to exploit it depending on downstream usage. Code changes can silently add
copies. The programmer gets a hopeful suggestion, not a contract.

**Lesson**: Signal declarations should be contracts. If you say "this callback
must not yield," the system guarantees it — by static proof or runtime check.
The programmer gets a real promise.

### Nim CPS (cautionary tale)

A CPS system was built in Nim that transformed "normal" function definitions
into delimited continuations via compile-time macros. The compromise was
hiding the full power of continuations behind familiar syntax, because
programmers "couldn't handle" the real thing. The terminology was intimidating
and alien, and users were turned off.

**Lesson**: Don't hide the power. Make it accessible with clear terminology.
Our users will be LLM agents — they're smart, they can handle precise
concepts. Use terms that match the technical reality. If the programmer's
mental model matches the mechanism, the tool is well-designed (cf. Don
Norman's _The Design of Everyday Things_).


## Terminology

These terms have precise meanings in Elle. Using them loosely leads to
sloppy thinking and sloppy implementation.

**Continuation**: The rest of the computation from a given point. A
mathematical concept — every program point has a continuation.

**Delimited continuation**: A continuation captured up to a specific boundary
(the delimiter). When invoked, it returns a value to the delimiter. Composes
cleanly. Strictly more useful than Scheme's `call/cc` for practical
programming.

**Fiber**: An execution context with its own stack, status, signal mask, and
dynamic bindings. The runtime representation that makes all control flow
patterns possible. A fiber is a *thing*# patterns like coroutines, generators,
and green threads are *ways to use it*.

**Signal**: A value emitted by a fiber to its parent. Classified by type
(a small integer / bit position) and carrying a payload (an Elle Value). The
parent's mask determines catch-or-propagate. Signals are the runtime
communication mechanism between fibers.

**Signal (static)**: The static description of what signals a function might emit over
its lifetime. A set of signal types, represented as a bitfield. Signals exist
at compile time for analysis and optimization. Every runtime signal corresponds to a
static signal bit, but static signals describe *possibility* while runtime signals describe *events*.

**Handler**: Code that catches a specific signal type and provides a response.
In Elle, a handler is a fiber with the appropriate mask bit set. Catching is
determined by the mask# handling is whatever code runs after the resume
returns. Surface syntax: `catch` in a `try` block.

**Signal mask**: A bitfield on a fiber indicating which signal types it catches
from its children. Set at fiber creation time. The caller decides what to
handle, not the callee.

**Green thread**: A fiber scheduled by a userspace scheduler. An Erlang
process is a green thread. Implies cooperative or preemptive scheduling by a
runtime. The long-term goal for Elle.

**Coroutine**: A use pattern of fibers — specifically, a fiber that yields
values and can be resumed. Less general than the fiber primitive. We may keep
the word "coroutine" as sugar in the surface language, but the mechanism is
fibers.


## The Core Insight

**There is no distinction between static signals and runtime signals except timing.** A
static signal is a runtime signal that hasn't been emitted yet. A runtime signal is a static
signal that just happened. The bitfield is the same for both.

Whether a particular signal requires the caller's attention is determined by
the fiber's mask, not by the signal itself. If the mask says "catch IO," then
IO is a signal — the function suspends and the handler runs. If the mask
doesn't mention IO, the function just does the IO and continues.

This means:

- IO can be silent (no mask bit) or interceptable (mask bit set)
- Allocation can be silent or interceptable
- Any capability can be turned into a control flow interception point
- The programmer decides, at fiber creation time, what to intercept

This is algebraic effects without the type-theoretic baggage. The mechanism
is Janet's signals# the static analysis is Koka's signal tracking# the
programmer interface is "create a fiber with a mask."


## Capabilities Down, Signals Up

The unified model has two flows:

**Capabilities flow down.** When you call a function (or create a fiber), you
pass a bitfield of what the callee is permitted to do. "You may allocate, you
may do IO, you may not yield, you may not FFI." The callee inherits these
capabilities and passes them — possibly narrowed, never widened — to its own
callees.

**Signals flow up.** When a function needs a capability it *has*, it uses it
silently. No signal, no interruption, no overhead. When a function needs a
capability it *doesn't have*, it signals: "I need to allocate but I'm not
permitted — what should I do?" The caller handles this: grant the capability
(resume), deny it (error), provide an alternative (mock/redirect), or
propagate the signal to its own caller.

This is capability-based security applied to control flow:

- **The caller is the authority.** It decides what the callee may do.
- **The callee is the requestor.** It either operates within its grant
  (fast path) or signals when it hits a boundary (slow path).
- **No signal means success.** The capability was available and used. The
  caller doesn't need to know. This is why most calls are zero-overhead.
- **A signal means a boundary was hit.** The callee couldn't proceed without
  something it wasn't granted. The handler decides what happens next.

**Note on implementation phases**: Phases 1–5 of the migration (below) implement
signal routing and signal tracking. Capability enforcement — checking what a
fiber is allowed to do and signaling when it exceeds its grant — is future work
requiring a different mechanism (likely a capability field on fibers and checks
at signal emission points). The vision is complete, but the early phases focus
on the signal infrastructure that makes capability enforcement possible.

### Examples

**"Let me know if you want to switch threads"**: The caller grants all
capabilities *except* thread-switch. The callee runs. If it needs to switch
threads, it signals. The caller intercepts and decides: allow it, deny it,
or schedule it differently.

**"Let me know if you want to allocate"**: Same pattern. The caller withholds
the alloc capability. The callee signals when it needs to allocate. The
caller can provide a pre-allocated buffer, deny the allocation, or grant it.

**"FFI is denied in this execution context"**: The caller withholds the FFI
capability. If the callee never needs FFI, no signal — full speed. If it
does need FFI, it signals, and the caller handles the denial.

**"This callback must be inert"**: The caller grants *no* capabilities. If the
callback tries to do anything — yield, error, IO, allocate — it signals.
The caller treats any signal as a contract violation. (Note: inert functions are a superset of pure functions — inert means only that no signals are emitted, not that there are no side-effects.)

### Narrowing, Not Widening

Capabilities can only be narrowed as they flow down. A callee cannot grant
its own callees capabilities it doesn't have. This is the security property:
if a fiber doesn't have FFI permission, nothing it calls can do FFI (unless
it explicitly handles the FFI signal and provides an alternative).

This means the root fiber — the top-level program — has all capabilities.
Each fiber boundary is an opportunity to restrict. Sandboxing is just
"create a fiber with a narrow capability set."

### Relationship to Signals

A function's **signal bits** describe what capabilities it *might need*. The
caller's **capability bits** describe what it *grants*. A signal occurs when
`needed & ~granted != 0` — the function needs something it wasn't given.

At compile time, the compiler can check: does the callee's signal set fit
within the caller's capability grant? If yes, no signals are possible from
this call (for capability reasons). If no, signals are possible and the
caller must be prepared to handle them.

At runtime, the capability bits flow down as an argument or fiber field. The
callee checks its needs against the grant. The check is one AND operation.

### The Fast Path

For the common case — calling a function with sufficient capabilities — the
overhead is zero. The callee has what it needs, does its work, returns a
value. No signal, no handler dispatch, no continuation capture.

The signal path is the slow path, and it's slow *on purpose* — something
unusual happened that requires the caller's attention. This is the right
performance profile: optimize for the common case, pay only when something
interesting happens.


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
- **Inert**: `bits == 0` (no signals)
- **Has**: `bits & YIELD != 0` (membership test)

### Compile-Time Inference

The compiler walks the AST and infers signals:

- A literal is inert (no bits)
- A primitive has known signal bits (declared at registration)
- A call's signal is the callee's signal combined with the call overhead
- A `begin` block's signal is the union of its children
- A lambda's body signal is stored on the lambda but the lambda itself is inert
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

The programmer can restrict signals on functions using the `silence` form:

```janet
(defn query (db sql)
  (silence :io :error)
  ...)
```

And signal bounds on parameters:

```janet
(defn fast-map (f xs)
  (silence f)
  (map f xs))
```

These are contracts. The system enforces them — statically when possible,
dynamically when not.

### Runtime Signal Checking

Closures carry their signal bits. When a closure is passed to a function
with a signal bound, the runtime checks:

```
closure.signals & ~bound == 0
```

This is one AND and one comparison. If it fails, it's a signal (an error
signal, specifically).


## JIT Integration

### The Current Problem

The JIT can only compile inert functions because it can't handle yields or
errors — it would need to save and restore the native stack, which is
complex and platform-specific.

### The Solution

In the fiber model, a JIT-compiled function is just another frame on the
fiber's stack. When it calls a function that signals:

1. The callee returns `signal_bits` — the value is on the fiber
2. The JIT code checks `signal_bits == 0`
3. If zero: continue with the value from the fiber's stack
4. If non-zero: propagate the signal to the caller

The JIT doesn't need to capture continuations or switch stacks. It just
checks a return code and propagates. This is the same thing Janet's C code
does — check the signal, propagate if not caught.

This means the JIT can compile *any* function, not just inert ones. The
overhead for non-inert functions is one branch per call (checking the signal).
For inert functions (where the compiler can prove no signals), the branch can
be elided entirely.

### Signal-Guided Optimization

The compiler's signal information guides JIT decisions:

- **No signals**: inline aggressively, no signal checks needed
- **Errors only**: signal checks needed, but no yield/continuation overhead
- **Yields**: full signal protocol, but the JIT still compiles the function —
  it just includes the propagation path
- **Known inert callback**: when a higher-order function is called with a
   provably-inert callback, the JIT can specialize the inner loop to skip
   signal checks


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

Stream primitives (`stream/read-line`, `stream/read`, `stream/read-all`,
`stream/write`, `stream/flush`) have signal `io_errors()`. They do not
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

Bits 3, 5, 6, 7, 10–15 are reserved for VM-internal use and are not user-visible.

### User-Defined Signals

User-defined signals are registered via the `(signal :keyword)` form and allocated bits 16–31. Up to 16 user signals are supported per compilation unit.

```janet
# Register a user-defined signal
(signal :heartbeat)
(signal :rate-limit)

# Signals are registered in order of appearance
# :heartbeat gets bit 16, :rate-limit gets bit 17
```

Duplicate registration is a compile-time error. Typos in signal keywords used in `silence` that don't match any registered signal are also compile-time errors.

## Declaring User Signals

### `(signal :keyword)` Form

Registers a new user-defined signal keyword and returns the keyword value.

**Syntax:**
```janet
(signal :keyword)
```

**Semantics:**
- Registers `:keyword` in the global signal registry (if not already registered)
- Returns the keyword value
- Valid in any position (file scope or expression position)
- Duplicate registration is a compile-time error
- Built-in signals (`:error`, `:yield`, `:debug`, `:ffi`, `:halt`, `:io`) cannot be re-registered

**Examples:**
```janet
# File scope
(signal :heartbeat)
(signal :rate-limit)

# Expression position
(def my-signal (signal :custom))
my-signal  # ⟹ :custom
```

## Signal Restrictions

### `(silence ...)` Form

Declares signal bounds on a function or its parameters. Appears as a preamble declaration in lambda bodies (after optional docstring, before first non-declaration expression).

**Syntax:**
```janet
# Function-level restriction (no signals)
(silence)

# Function-level restriction (specific signals allowed)
(silence :kw1 :kw2)

# Parameter-level restriction (parameter must be inert)
(silence param)

# Parameter-level restriction (parameter may emit specific signals)
(silence param :kw1 :kw2)
```

**Semantics:**

- `(silence)` — This function emits no signals (inert)
- `(silence :kw1 :kw2)` — This function may emit only these signals
- `(silence param)` — Parameter `param` must be inert (no signals)
- `(silence param :kw1 :kw2)` — Parameter `param` may emit at most these signals
- Multiple `silence` forms allowed in one lambda (one per parameter + one function-level)
- Keywords must be registered (via `signal` or built-in)
- Parameter names must match declared parameters
- Duplicate restrictions for the same parameter: the last one wins

**Outside lambda bodies**, `silence` is a call to the stdlib `silence` function, which signals `:error` at runtime. `silence` is implemented as:
```
(defn silence [& _]
  (error {:error :invalid-silence
          :message "silence must appear in a function body preamble"}))
```

**Examples:**
```janet
# Inert function
(defn add (x y)
  (silence)
  (+ x y))

# Function that may error
(defn validate (x)
  (silence :error)
  (if (< x 0) (error "negative") x))

# Higher-order function with inert callback
(defn apply-inert (f x)
  "Apply f to x, requiring f to be inert."
  (silence f)
  (f x))

# Higher-order function with bounded callback
(defn apply-safe (f x)
  "Apply f to x, allowing only errors."
  (silence f :error)
  (f x))

# Multiple restrictions
(defn map-safe (f xs)
  "Map f over xs, f may only error."
  (silence f :error)
  (silence :error)
  (map f xs))
```

## Compile-Time Verification

### Signal Inference with Bounds

Every lambda has `inferred_signals` — the minimum guaranteed set of signals the lambda may produce. It is always present (never Optional) and is accumulated from:

1. **Direct signal emissions** in the body (e.g., `(yield x)`, `(error "msg")`)
2. **Signals of internal calls** to statically-known functions — their `inferred_signals` bits propagate upward
3. **Signals contributed by parameter calls:**
   - If a parameter has a `silence` bound, its bound's bits are included in `inferred_signals`
   - If a parameter has NO bound, it contributes conservatively (Yields)

The `inferred_signals: Signal` field is always present and contains the minimum guaranteed set of signals the lambda may produce. The programmer-supplied ceiling constraint from `(silence)` or `(silence :kw ...)` is a separate concept — the `silence` form provides a bound that the compiler checks `inferred_signals` against. When a `silence` bound is present, the compiler checks that `inferred_signals.bits ⊆ bound.bits`. If the check passes, the lambda's final signal is the declared bound (tighter). If it fails, compile-time error.

**Example:**
```janet
# Function with parameter bound
(defn apply-inert (f x)
  (silence f)  # f must be inert
  (f x))

# Inferred signal: inert (because f is bounded to inert)
# No polymorphism — f's signal is known to be zero bits

# This works: + is inert
(apply-inert + 42)

# This fails at compile time: yielding function violates bound
(apply-inert (fn () (yield 1)) 42)
```

### Parameter Bounds Eliminate Polymorphism

A function with `(silence f)` is no longer polymorphic with respect to `f`. The compiler knows `f` must be inert, so the function's signal is determined by its own body only, not by what `f` might do.

**Example:**
```janet
# Without bound: polymorphic
(defn map-any (f xs)
  (map f xs))
# Signal: Polymorphic(0) — depends on f's signal

# With bound: not polymorphic
(defn map-inert (f xs)
  (silence f)
  (map f xs))
# Signal: inert — f is guaranteed inert, so map is inert
```

### Call-Site Checking

When a concrete function is passed to a parameter with a bound, the analyzer checks the argument's signal against the bound at compile time.

**Example:**
```janet
(defn apply-inert (f x)
  (silence f)
  (f x))

# Compile-time check passes: + is inert
(apply-inert + 42)

# Compile-time check fails: yielding function violates bound
(apply-inert (fn () (yield 1)) 42)
# Error: argument violates signal bound
```

## Runtime Verification

When a closure is passed to a function with a signal bound, the runtime checks that the closure's signal satisfies the bound. This is necessary for dynamic arguments where the signal cannot be determined at compile time.

**Mechanism:**
- The lowerer emits a `CheckSignalBound` instruction at function entry for each bounded parameter
- The VM checks: `closure.signal.bits & ~allowed != 0`
- If the check fails, the VM signals `:error` with a descriptive message

**Example:**
```janet
(defn apply-inert (f x)
  (silence f)
  (f x))

# At runtime, if f's signal violates the bound, error is signaled
(var f (eval '(fn () (yield 1))))
(apply-inert f 42)
# Runtime error: argument violates signal bound
```

## JIT Integration

Signal bounds enable JIT optimizations:

1. **Loop specialization**: When a higher-order function is called with a provably-inert callback, the JIT can specialize the inner loop to skip signal checks
2. **Inlining**: Inert callbacks can be inlined more aggressively
3. **Elimination of polymorphism**: Bounded parameters eliminate the need to track polymorphic signals, simplifying JIT compilation

**Example:**
```janet
# Without bounds: JIT cannot specialize
(defn map-any (f xs)
  (map f xs))

# With bounds: JIT can specialize for inert f
(defn map-inert (f xs)
  (silence f)
  (map f xs))
```

## `(signals)` Introspection Primitive

Returns the full signal registry as a struct with signal keywords as keys and bit positions as values.

**Syntax:**
```janet
(signals)
```

**Returns:**
A struct mapping signal keywords to bit positions. Includes both built-in and user-defined signals.

**Example:**
```janet
(signals)
# ⟹ {:error 0 :yield 1 :debug 2 :ffi 4 :halt 8 :io 9 :heartbeat 16 :rate-limit 17}

# After registering user signals
(signal :custom)
(signals)
# ⟹ {:error 0 :yield 1 :debug 2 :ffi 4 :halt 8 :io 9 :heartbeat 16 :rate-limit 17 :custom 18}
```

## Surface Syntax

### Fiber Primitives

```janet
;# === Creation and control ===

;# Create a fiber from a closure with a signal mask
(fiber/new fn mask) → fiber

;# Resume a fiber, delivering a value
(fiber/resume fiber value) → signal-bits

;# Emit a signal from the current fiber (suspends it)
(emit bits value) → (suspends)

;# === Introspection ===

;# Lifecycle status
(fiber/status fiber) → keyword  # :new :alive :suspended :dead :error

;# Signal payload from last signal or return value
(fiber/value fiber) → value

;# Signal bits from last signal
(fiber/bits fiber) → int

;# Capability mask (set at creation, immutable)
(fiber/mask fiber) → int

;# === Chain traversal ===

;# Parent fiber (nil at root)
(fiber/parent fiber) → fiber | nil

;# Most recently resumed child fiber (nil if none)
(fiber/child fiber) → fiber | nil

;# === Internals (for debugging/tooling) ===

;# The closure this fiber wraps
(fiber/closure fiber) → closure

;# The operand stack (for debugging)
(fiber/stack fiber) → array

;# Dynamic bindings (fiber-scoped state)
(fiber/env fiber) → @struct | nil
```

### Sugar and Aliases

```janet
;# try/catch/finally
(try body
  (catch e handler)
  (finally cleanup))

;# yield
(yield value) → (emit :yield value)

;# error (signal an error)
(error value) → (emit 1 value)

;# Thin aliases
(coro/new fn) → (fiber/new fn :yield)
(coro/resume co val) → (fiber/resume co val)
(coro/status co) → (fiber/status co)
```

### Signal Restrictions

```janet
(defn inert-add (x y)
  (silence)           ;# no signals — inert
  (+ x y))

(defn may-fail (x)
  (silence :error)   ;# may error, nothing else
  (/ 1 x))

(defn callback-must-be-inert (f xs)
  (silence f) ;# f must have no signals
  (map f xs))
```


## Migration Status

Steps 1–3 are complete. Steps 4–7 are future work.

1. ✅ **Fibers as execution context.** Fiber struct, FiberHandle,
   parent/child chain, signal mask, all fiber primitives implemented.
   Coroutines are fibers that yield.

2. ✅ **Unified signals.** Error and yield are signal types. `Condition`
   type removed. Errors are `[:keyword "message"]` tuples.

3. ✅ **Signal-bits-based Signal type.** `Signal { bits: SignalBits,
   propagates: u32 }`. Inference tracks signal bits. Old `yield_behavior`
    and `may_error` fields replaced.

4. ❌ **Relax JIT restrictions.** JIT still restricted to inert functions.
   Signal-aware calling convention not yet implemented.

5. ❌ **User-defined signals.** Bit positions 16–31 reserved but no
   allocation API.

6. ✅ **Signal restrictions.** `silence` forms for signal contracts implemented.

7. ❌ **Erlang-style processes.** Fibers on an event loop with a scheduler.


## Non-Unwinding Recovery

The fiber model supports non-unwinding recovery without additional mechanism.
Recovery options emerge from the interaction of signals and resume.

### How it works

When a fiber signals, it **suspends** — its frames remain intact. The
parent fiber (or any ancestor in the chain) catches the signal and
decides what to do. If the signaling fiber advertised recovery options in its
payload, the handler picks one and resumes the child with that choice.
The child receives the resume value, dispatches on it, and continues.

Signals travel **up** the fiber chain (parent links). Recovery choices
travel back **down** the chain (resume calls). This is bidirectional
communication along the chain, not unwinding.

### Two recovery patterns

**Non-unwinding recovery**: The handler catches the signal,
inspects it, resumes the child with a recovery choice. The child
continues from where it suspended. The child's frames are never
discarded.

**Unwinding recovery** (via `try`/`catch`): The handler catches the signal and does
NOT resume the child. The child fiber becomes garbage. The handler
runs its own code in the parent fiber. This is a one-way trip.

Both patterns are just different uses of the same mechanism — resume vs.
don't resume. No special syntax or VM support is needed.

### Why this is strictly more powerful than traditional restart systems

- **Recovery options are data, not syntax.** The signal payload is a value, so
   available recovery options can be computed dynamically.
- **Multiple round-trips.** The handler resumes the child, the child
   signals again ("your suggestion also failed"), the handler tries
   something else. Arbitrary dialogue along the chain.
- **Composition through the chain.** If the immediate parent doesn't
   know what to do, it propagates the signal up the chain. An ancestor
   that understands the situation handles it and the recovery choice travels
   back down through resume calls.
- **The handler has full context.** It's running code in its own fiber
   with access to its own state. It can query databases, ask the user,
   or try multiple strategies before deciding.

### Example

```janet
;# The callee: signals with available recovery options
(def (safe-divide a b)
   (if (= b 0)
     (emit :error
       (@struct :error :division-by-zero
                :options [:use-value :return-zero]))
     (/ a b)))

;# The handler: catches the signal, picks a recovery option
(def (compute)
   (let ((f (fiber/new (fn () (safe-divide 10 0)) :error)))
     (let ((result (fiber/resume f nil)))
       (if (= (fiber/status f) :suspended)
         ;# Child is suspended — we can resume it with a recovery choice
         (fiber/resume f (@struct :option :use-value :value 1))
         result))))
```


## Error Signalling

Errors in Elle are signals — values emitted on the `:error` bit (bit 0,
`SIG_ERROR`). There is no exception hierarchy, no `Condition` type, no
`handler-case`. Error handling is fiber signal handling.

### Error Representation

The stdlib convention is a struct: `{:error :keyword :message "message"}`.

```janet
# Stdlib primitive errors look like:
{:error :type-error :message "car: expected pair, got integer"}
{:error :division-by-zero :message "cannot divide by zero"}
{:error :arity-mismatch :message "expected 2 arguments, got 3"}
```

The `:error` keyword classifies the error. The `:message` string describes it.
Both are ordinary Elle values — no special types.

**This is a convention, not a hard rule.** Users can define their own error
value shapes. The signal system doesn't care what the payload is; it's just
a Value. Pattern matching on the payload is how handlers distinguish error
kinds. A user might prefer `[:boom "message"]`, or a plain string, or an
integer error code — whatever suits their domain.

### Two Failure Modes

**VM bugs** (stack underflow, bad bytecode, corrupted state): the compiler
emitted bad code or the VM has a defect. These panic immediately. Elle code
cannot catch them.

**Runtime errors** (type mismatch, arity error, division by zero, undefined
variable): program behavior on bad data. These are signalled via `SIG_ERROR`
and can be caught by a parent fiber with the appropriate mask.

### How Errors Flow

**From primitives**: All primitives are `NativeFn: fn(&[Value]) -> (SignalBits, Value)`.
Success returns `(SIG_OK, value)`. Error returns `(SIG_ERROR, error_struct)`.
The VM's dispatch checks signal bits after each primitive call.

**From instruction handlers**: Instructions like `Add`, `Car`, `Cdr` set
`fiber.signal` directly when they detect a type mismatch or other error.

**From Elle code**: Use `error` (a prelude macro) or `emit` directly:

```janet
# Prelude macro — signals {:error :the-kw :message "..."} on SIG_ERROR
(error {:error :bad-input :message "expected a number"})

# Or emit directly — any value works
(emit 1 {:error :custom :message "something failed"})

# User-defined error shape — completely valid
(emit 1 [:my-error "the details"])
```

### Catching Errors

Errors are caught by fibers whose mask includes the `:error` bit:

```janet
# try/catch is sugar for fiber signal handling
(try
  (risky-operation)
  (catch e
    (handle-error e)))

;# Expands to approximately:
(let ((f (fiber/new (fn () (risky-operation)) 1)))  # mask = SIG_ERROR
  (fiber/resume f nil)
  (match (fiber/status f)
    (:error (handle-error (fiber/value f)))
    (_ (fiber/value f))))
```

The `try`/`catch`, `protect`, `defer`, and `with` macros are all built on
fiber primitives. No special VM support.

### Error Propagation

Errors propagate up the fiber chain until caught:

1. Child signals `SIG_ERROR`
2. Parent checks: `child.mask & SIG_ERROR != 0`?
   - **Yes**: parent catches, child stays suspended (or becomes `error` state)
   - **No**: parent also suspends, signal propagates to grandparent
3. At the root fiber: uncaught error becomes `Err(String)` via the public API boundary

`fiber/propagate` re-signals a caught signal, preserving the child chain for
stack traces. `fiber/cancel` hard-kills a fiber (no unwinding).
`fiber/abort` injects an error and resumes a suspended fiber for graceful
unwinding (defer/protect blocks run).

### The Public API Boundary

`execute_bytecode` is the translation boundary between the signal-based
internal VM and the `Result<Value, String>` external API:

- `SIG_OK` → `Ok(value)`
- `SIG_ERROR` → `Err(format_error(signal_value))`

External callers (REPL, file execution, tests) see `Result`. Internal code
sees `SignalBits`.


## Open Questions

### Compound signals

Can a function emit multiple signal bits simultaneously? Current position:
probably not (signals are suspension points), but the representation supports
it. Revisit if we find a use case.

### Signal bit allocation

32 bits: 7 used (0–6) + 9 reserved (7–15) + 16 user-defined (16–31). If
users need more than 16 signal types, bump SignalBits to u64.

### Dynamic signal checking overhead

Checking `closure.signals & ~bound == 0` at every call boundary with a
signal bound — is this too expensive? It's one AND + one branch. Probably
fine, but worth measuring.

### Interaction with the type system

Elle doesn't have a static type system (yet). Signals are the closest thing
to static types. Should they evolve toward a type system, or remain a
separate concern?

### Signal subtyping

Should there be a hierarchy of signal types, or is the flat bitfield
sufficient? Janet uses a flat space. Koka uses a hierarchy. Flat is simpler
and faster. Current implementation: flat.

## Resolved Questions

- **Signal resumption**: Yes. Resume value is pushed onto the child's operand
  stack. See `docs/fibers.md`.

- **Error representation**: Errors are values — by convention a struct
  `{:error :keyword :message "..."}`, but any value works. No `Condition`
  type, no signal hierarchy. Pattern matching on the payload replaces hierarchy
  checks. See the "Error Signalling" section below.

- **Coroutine aliases**: `yield` works as a special form (emits
  `SIG_YIELD`). `make-coroutine` / `coro/resume` are thin wrappers
  around `fiber/new` / `fiber/resume`. `try`/`catch` macro is blocked on
  macro system work.

- **Signal erasure**: Signal bits are stored on the `Closure` struct (one
  `Signal` value = 8 bytes). Acceptable cost.
