# Effects and Signals: Design Document

This document captures the design of Elle's unified effect and signal system.
It records not just decisions but the reasoning, alternatives, and open
questions that led to them. Future readers should be able to understand the
trade-offs and pick up where we left off.


## Motivation

Elle previously had separate mechanisms for coroutines (continuation
capture/replay), exception handling (handler stack with unwind semantics),
and effect inference (boolean fields for yields and raises). The JIT could
only compile pure functions.

These have been unified into a single mechanism: **fibers with signals**.
Coroutines are fibers that yield. Errors are signals. The effect system
tracks signal bits. See `docs/FIBERS.md` for the implementation reference.


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
  Functions are colorless; fibers are colored.

- **Composition over special forms.** `try`, `catch`, `finally`,
   `generate` — all macros over `fiber/new` + `resume` + `fiber/status` +
   `propagate`. One runtime primitive; the language provides sugar.

Janet's limitation: signals are a single integer (one thing happened), and
there's no static tracking of effects. You can't look at a function and know
what signals it might emit. Optimization opportunities that depend on static
knowledge are unavailable.

See `docs/JANET.md` for the full architectural analysis.

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

**Lesson**: Effect declarations should be contracts. If you say "this callback
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
patterns possible. A fiber is a *thing*; patterns like coroutines, generators,
and green threads are *ways to use it*.

**Signal**: A value emitted by a fiber to its parent. Classified by type
(a small integer / bit position) and carrying a payload (an Elle Value). The
parent's mask determines catch-or-propagate. Signals are the runtime
communication mechanism between fibers.

**Effect**: The static description of what signals a function might emit over
its lifetime. A set of signal types, represented as a bitfield. Effects exist
at compile time for analysis and optimization. Every signal corresponds to an
effect bit, but effects describe *possibility* while signals describe *events*.

**Handler**: Code that catches a specific signal type and provides a response.
In Elle, a handler is a fiber with the appropriate mask bit set. Catching is
determined by the mask; handling is whatever code runs after the resume
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

**There is no distinction between effects and signals except timing.** An
effect is a signal that hasn't been emitted yet. A signal is an effect that
just happened. The bitfield is the same for both.

Whether a particular effect requires the caller's attention is determined by
the fiber's mask, not by the effect itself. If the mask says "catch IO," then
IO is a signal — the function suspends and the handler runs. If the mask
doesn't mention IO, the function just does the IO and continues.

This means:

- IO can be silent (no mask bit) or interceptable (mask bit set)
- Allocation can be silent or interceptable
- Any capability can be turned into a control flow interception point
- The programmer decides, at fiber creation time, what to intercept

This is algebraic effects without the type-theoretic baggage. The mechanism
is Janet's signals; the static analysis is Koka's effect tracking; the
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
signal routing and effect tracking. Capability enforcement — checking what a
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

**"This callback must be pure"**: The caller grants *no* capabilities. If the
callback tries to do anything — yield, raise, IO, allocate — it signals.
The caller treats any signal as a contract violation.

### Narrowing, Not Widening

Capabilities can only be narrowed as they flow down. A callee cannot grant
its own callees capabilities it doesn't have. This is the security property:
if a fiber doesn't have FFI permission, nothing it calls can do FFI (unless
it explicitly handles the FFI signal and provides an alternative).

This means the root fiber — the top-level program — has all capabilities.
Each fiber boundary is an opportunity to restrict. Sandboxing is just
"create a fiber with a narrow capability set."

### Relationship to Effects

A function's **effect bits** describe what capabilities it *might need*. The
caller's **capability bits** describe what it *grants*. A signal occurs when
`needed & ~granted != 0` — the function needs something it wasn't given.

At compile time, the compiler can check: does the callee's effect set fit
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
| 5 | propagate | 32 | VM-internal: re-raise caught signal |
| 6 | cancel | 64 | VM-internal: inject error into fiber |
| 7–15 | reserved | — | Future compiler-known signals |
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

### One Signal or Many?

**Open question.** Can a function emit multiple signal bits simultaneously?

The argument for single signals: a signal is a suspension point. The function
stops executing, the handler runs, the function (maybe) resumes. Only one
thing caused it to stop.

The argument for compound signals: a function might want to say "I need IO
AND I'm yielding" or "this is an error AND it's user-signal-5." Compound
signals carry more information.

**Current position**: Signals are primarily one-at-a-time because they
represent suspension points. But we don't foreclose on compound signals. The
bitfield representation supports them naturally. If we discover a need, we
can allow them without redesigning.

The payload value can carry additional context that doesn't need to be in the
signal bits. "I'm yielding, and here's a struct describing what IO I need" is
a single yield signal with a rich payload.

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
    stack: SmallVec<[Value; 256]>           -- operand stack
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

The closure carries its effect bits. The fiber's mask determines which
signals it catches from children. There is no `effects` field on the Fiber
— effects are a compile-time property of the closure, not the fiber.

See `docs/FIBERS.md` for the full Fiber, SuspendedFrame, and FiberHandle
documentation.


## The Effect System

### Effect Bits

An effect is a set of signal types that a function might emit. Represented
as a bitfield (same type as signal bits).

```
type EffectBits = SignalBits  -- same bitfield type, same bit positions
```

Operations:

- **Combine**: `a | b` (union — a block's effect is the union of its parts)
- **Check**: `actual & ~permitted == 0` (subset — are all actual effects permitted?)
- **Pure**: `bits == 0` (no effects)
- **Has**: `bits & YIELD != 0` (membership test)

### Compile-Time Inference

The compiler walks the AST and infers effects:

- A literal is pure (no bits)
- A primitive has known effect bits (declared at registration)
- A call's effect is the callee's effect combined with the call overhead
- A `begin` block's effect is the union of its children
- A lambda's body effect is stored on the lambda but the lambda itself is pure
- A handler that catches signal X removes bit X from the enclosed expression's
  effect

### Parametric Polymorphism

Higher-order functions propagate their arguments' effects. `map`'s effect is
"whatever `f` does, plus my own base effects." The compile-time
representation:

```
Effect {
    bits: SignalBits,           -- from own body
    propagates: u32,            -- bitmask of parameter indices
}
```

Resolved effect at a call site:

```
call_effect = f.bits | union(effect(arg[i]) for i in 0..param_count if (propagates & (1 << i)) != 0)
```

If the compiler can see the concrete argument (e.g., it's the `+` primitive),
it can resolve the polymorphism statically and potentially prove the call site
has fewer effects than the general case.

**Note**: Effect bounds on parameters (constraining what effects callbacks may
have) are deferred to a future phase. When needed, they'll be tracked in the
analysis environment, not on the Effect struct itself — keeping Effect as a
simple Copy pair.

### Effect Declarations

The programmer can declare effects on functions:

```lisp
(define (query db sql)
  (declare (effects :io :raises))
  ...)
```

And effect bounds on parameters:

```lisp
(define (fast-map f xs)
  (declare (param-effects f (not :yields :io)))
  ...)
```

These are contracts. The system enforces them — statically when possible,
dynamically when not.

### Runtime Effect Checking

Closures carry their effect bits. When a closure is passed to a function
with an effect bound, the runtime checks:

```
closure.effects & ~bound == 0
```

This is one AND and one comparison. If it fails, it's a signal (an error
signal, specifically).


## JIT Integration

### The Current Problem

The JIT can only compile pure functions because it can't handle yields or
raises — it would need to save and restore the native stack, which is
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

This means the JIT can compile *any* function, not just pure ones. The
overhead for non-pure functions is one branch per call (checking the signal).
For pure functions (where the compiler can prove no signals), the branch can
be elided entirely.

### Effect-Guided Optimization

The compiler's effect information guides JIT decisions:

- **No effects**: inline aggressively, no signal checks needed
- **Raises only**: signal checks needed, but no yield/continuation overhead
- **Yields**: full signal protocol, but the JIT still compiles the function —
  it just includes the propagation path
- **Known pure callback**: when a higher-order function is called with a
  provably-pure callback, the JIT can specialize the inner loop to skip
  signal checks


## Surface Syntax

### Fiber Primitives

```lisp
;; === Creation and control ===

;; Create a fiber from a closure with a signal mask
(fiber/new fn mask) → fiber

;; Resume a fiber, delivering a value
(fiber/resume fiber value) → signal-bits

;; Emit a signal from the current fiber (suspends it)
(fiber/signal bits value) → (suspends)

;; === Introspection ===

;; Lifecycle status
(fiber/status fiber) → keyword  ; :new :alive :suspended :dead :error

;; Signal payload from last signal or return value
(fiber/value fiber) → value

;; Signal bits from last signal
(fiber/bits fiber) → int

;; Capability mask (set at creation, immutable)
(fiber/mask fiber) → int

;; === Chain traversal ===

;; Parent fiber (nil at root)
(fiber/parent fiber) → fiber | nil

;; Most recently resumed child fiber (nil if none)
(fiber/child fiber) → fiber | nil

;; === Internals (for debugging/tooling) ===

;; The closure this fiber wraps
(fiber/closure fiber) → closure

;; The operand stack (for debugging)
(fiber/stack fiber) → vector

;; Dynamic bindings (fiber-scoped state)
(fiber/env fiber) → table | nil
```

### Sugar and Backward Compatibility

```lisp
;; try/catch/finally
(try body
  (catch e handler)
  (finally cleanup))

;; yield
(yield value) → (fiber/signal :yield value)

;; throw
(throw value) → (fiber/signal :error value)

;; Backward compat (thin aliases)
(make-coroutine fn) → (fiber/new fn :yield)
(coroutine-resume co val) → (fiber/resume co val)
(coroutine-status co) → (fiber/status co)
```

### Effect Declarations

```lisp
(define (pure-add x y)
  (declare (effects))           ;; no effects — pure
  (+ x y))

(define (may-fail x)
  (declare (effects :raises))   ;; may raise, nothing else
  (/ 1 x))

(define (callback-must-be-pure f xs)
  (declare (param-effects f ())) ;; f must have no effects
  (map f xs))
```


## Migration Status

Steps 1–3 are complete. Steps 4–7 are future work.

1. ✅ **Fibers as execution context.** Fiber struct, FiberHandle,
   parent/child chain, signal mask, all fiber primitives implemented.
   Coroutines are fibers that yield.

2. ✅ **Unified signals.** Error and yield are signal types. `Condition`
   type removed. Errors are `[:keyword "message"]` tuples.

3. ✅ **Signal-bits-based Effect type.** `Effect { bits: SignalBits,
   propagates: u32 }`. Inference tracks signal bits. Old `yield_behavior`
   and `may_raise` fields replaced.

4. ❌ **Relax JIT restrictions.** JIT still restricted to pure functions.
   Signal-aware calling convention not yet implemented.

5. ❌ **User-defined signals.** Bit positions 16–31 reserved but no
   allocation API.

6. ❌ **Effect declarations.** `declare` forms for effect contracts not
   yet implemented.

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

```lisp
;; The callee: signals with available recovery options
(define (safe-divide a b)
  (if (= b 0)
    (fiber/signal :error
      (table :error :division-by-zero
             :options [:use-value :return-zero]))
    (/ a b)))

;; The handler: catches the signal, picks a recovery option
(define (compute)
  (let ((f (fiber/new (fn () (safe-divide 10 0)) :error)))
    (let ((result (fiber/resume f nil)))
      (if (= (fiber/status f) :suspended)
        ;; Child is suspended — we can resume it with a recovery choice
        (fiber/resume f (table :option :use-value :value 1))
        result))))
```


## Open Questions

### Compound signals

Can a function emit multiple signal bits simultaneously? Current position:
probably not (signals are suspension points), but the representation supports
it. Revisit if we find a use case.

### Signal bit allocation

32 bits: 7 used (0–6) + 9 reserved (7–15) + 16 user-defined (16–31). If
users need more than 16 signal types, bump SignalBits to u64.

### Dynamic effect checking overhead

Checking `closure.effects & ~bound == 0` at every call boundary with an
effect bound — is this too expensive? It's one AND + one branch. Probably
fine, but worth measuring.

### Interaction with the type system

Elle doesn't have a static type system (yet). Effects are the closest thing
to static types. Should they evolve toward a type system, or remain a
separate concern?

### Effect subtyping

Should there be a hierarchy of signal types, or is the flat bitfield
sufficient? Janet uses a flat space. Koka uses a hierarchy. Flat is simpler
and faster. Current implementation: flat.

## Resolved Questions

- **Signal resumption**: Yes. Resume value is pushed onto the child's operand
  stack. See `docs/FIBERS.md`.

- **Error representation**: Errors are `[:keyword "message"]` tuples. No
  `Condition` type, no exception hierarchy. Pattern matching on the payload
  replaces hierarchy checks.

- **Backward compatibility**: `yield` works as a special form (emits
  `SIG_YIELD`). `make-coroutine` / `coroutine-resume` are thin wrappers
  around `fiber/new` / `fiber/resume`. `try`/`catch` macro is blocked on
  macro system work.

- **Effect erasure**: Effect bits are stored on the `Closure` struct (one
  `Effect` value = 8 bytes). Acceptable cost.
