# Signal Design

## Motivation

Elle previously had separate mechanisms for coroutines (continuation
capture/replay), exception handling (handler stack with unwind semantics),
and signal inference (boolean fields for yields and errors). The JIT could
only compile silent functions.

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

**"This callback must be silent"**: The caller grants *no* capabilities. If the
callback tries to do anything — yield, error, IO, allocate — it signals.
The caller treats any signal as a contract violation. (Note: silent functions are a superset of pure functions — silent means only that no signals are emitted, not that there are no side-effects.)

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

---

## See also

- [Signal index](index.md)
