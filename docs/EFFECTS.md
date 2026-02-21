# Effects and Signals: Design Document

This document captures the design of Elle's unified effect and signal system.
It records not just decisions but the reasoning, alternatives, and open
questions that led to them. Future readers should be able to understand the
trade-offs and pick up where we left off.


## Motivation

Elle has delimited continuations, `handler-case`, coroutines, and a JIT
compiler. These are currently separate mechanisms with separate
implementations:

- Coroutines use continuation capture/replay machinery in the VM
- Exception handling uses a handler stack with unwind semantics
- The JIT can only compile "pure" functions (no yields, no raises)
- Effect inference tracks yields and raises as separate boolean fields

This creates tension. The continuation machinery for yield is different from
the exception machinery. The JIT draws a hard line at purity. Adding a new
kind of control flow (IO interception, debug breakpoints, user-defined
protocols) would require yet another mechanism.

We want one mechanism that subsumes all of these.


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

- **Fibers over try/catch.** `try` is sugar for "create a fiber that catches
  errors, resume it, check the result." No special VM support.

- **Declarations at instantiation.** The signal mask lives on the fiber, set
  at creation time. The *caller* decides what to handle, not the function.
  Functions are colorless; fibers are colored.

- **Composition over special forms.** `defer`, `with`, `try`, `protect`,
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
returns.

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

Signal types are bit positions in a bitfield. The first N are
compiler-known:

| Bit | Name | Meaning |
|-----|------|---------|
| 0 | ok | Normal return (implicit — no bit set) |
| 1 | error | Exception / panic |
| 2 | yield | Cooperative suspension |
| 3 | debug | Breakpoint |
| 4–7 | reserved | Future compiler-known signals |
| 8+ | user | User-defined signal types |

Bit 0 is special: "ok" means no bits are set. A normal return has an empty
signal bitfield.

### Signal Values

A signal carries a type (which bit) and a payload (an Elle Value). The
return from any function call is:

```
(signal_bits: EffectBits, value: Value)
```

Where `signal_bits == 0` means normal return with `value` as the result.
Non-zero `signal_bits` means something happened that may require handling.

At the Rust level, this should be stack-allocated and alloc-free:

```rust
struct Signal {
    bits: u32,   // which signal(s) — usually exactly one non-zero bit
    value: Value, // payload
}
```

The fast path (normal return) is `bits == 0`, which is a single branch.
The Rust compiler can optimize this to a register pair.

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

### Propagation

When a fiber resumes a child and the child signals:

1. Check the child's signal bits against the parent fiber's mask
2. If `signal_bits & mask != 0` → **caught**: the parent handles it
3. If `signal_bits & mask == 0` → **propagates**: the parent returns the
   same signal to its own parent

This is O(1) — a single AND operation. No handler chain traversal.

### The Fiber Structure

```
Fiber {
    stack: [Value]          -- call stack (frames + locals + temporaries)
    status: FiberStatus     -- current state (new, alive, suspended, dead, ...)
    signal_mask: EffectBits -- which signals this fiber catches
    last_signal: Signal     -- most recent signal (type + value)
    parent: Option<Fiber>   -- parent fiber (for propagation)
    child: Option<Fiber>    -- child fiber (for resume routing + traces)
    env: Option<Table>      -- dynamic bindings
    effects: EffectBits     -- static effect annotation (what signals this fiber's
                            -- function might emit — set at creation, used for
                            -- optimization and boundary checks)
}
```


## The Effect System

### Effect Bits

An effect is a set of signal types that a function might emit. Represented
as a bitfield (same type as signal bits).

```
type EffectBits = u32  // or u64 if we need more room
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
FunctionEffectInfo {
    base: EffectBits,                        // from own body
    propagates_from: Set<ParamIndex>,        // which params' effects propagate
    bounds: Map<ParamIndex, EffectBits>,     // max effects allowed on params
}
```

Resolved effect at a call site:

```
call_effect = f.base | union(effect(arg[i]) for i in f.propagates_from)
```

Boundary check at each parameter:

```
for i in f.propagates_from:
    assert effect(arg[i]) & ~f.bounds[i] == 0
```

If the compiler can see the concrete argument (e.g., it's the `+` primitive),
it can resolve the polymorphism statically and potentially prove the call site
has fewer effects than the general case.

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

1. The callee returns `(signal_bits, value)` — a register pair
2. The JIT code checks `signal_bits == 0`
3. If zero: continue with `value` as the result
4. If non-zero: save any necessary state to the fiber's stack, return the
   signal to the caller

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

### Fiber Creation

```lisp
(fiber (fn [] body) :mask)
```

Where `:mask` is a set of signal types to catch. Sugar for common patterns:

```lisp
(fiber/try body)        ;; catches :error
(fiber/gen body)        ;; catches :yield
(fiber/task body)       ;; catches :error :yield (for green threads)
```

### Signal Emission

```lisp
(signal :yield value)   ;; emit a yield signal with payload
(signal :error value)   ;; emit an error signal (replaces raise/throw)
(signal :my-signal val) ;; emit a user-defined signal
```

### Resume

```lisp
(resume fiber value)    ;; resume a suspended fiber, delivering value
```

Returns the fiber's signal. The caller inspects `(fiber/status fiber)` and
`(fiber/value fiber)` — or uses destructuring sugar.

### Existing Forms as Sugar

```lisp
;; try/catch becomes:
(try body
  (catch :error e handler))
;; expands to:
(let [f (fiber (fn [] body) #{:error})]
  (let [r (resume f)]
    (if (= (fiber/status f) :error)
      (let [e r] handler)
      r)))

;; yield becomes:
(yield value)
;; expands to:
(signal :yield value)

;; handler-case becomes fiber creation with appropriate mask
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


## Migration Path

The current system has:
- `handler-case` for exception handling
- `yield` for coroutine suspension
- `make-coroutine` / `coroutine-resume` for coroutine management
- `Effect` struct with `yield_behavior` and `may_raise` fields
- JIT restricted to pure functions

The migration:

1. **Implement fibers** as the execution context, replacing the current
   coroutine implementation. A coroutine becomes a fiber with a yield mask.

2. **Unify signals.** Error and yield become signal types. `handler-case`
   compiles to fiber creation + resume + signal dispatch.

3. **Replace the Effect type** with a bitfield. Update inference to track
   signal bits instead of the current struct.

4. **Relax JIT restrictions.** JIT-compiled functions check signal returns
   from callees. Pure functions get the fast path (no checks).

5. **Add user-defined signals.** `define-signal` allocates a bit. Signal
   masks accept user-defined signal types.

6. **Add effect declarations.** `declare` forms for effect contracts on
   functions and parameters.

7. **Erlang-style processes.** Fibers on an event loop with a scheduler.
   Inter-process communication via signals and channels.

Each step is independently valuable and testable. Step 1–2 is the
foundation. Step 3–4 is optimization. Step 5–6 is expressiveness. Step 7
is the long game.


## Open Questions

### Compound signals

Can a function emit multiple signal bits simultaneously? Current position:
probably not (signals are suspension points), but the representation supports
it. Revisit if we find a use case.

### Signal bit allocation

How many bits? 32 is probably enough (8 compiler-known + 24 user). 64 gives
more room. The choice affects the size of effect annotations on closures.

### Effect erasure

Do effect bits on closures cost anything at runtime beyond storage? If we
store them in the closure's header (one word), the cost is one word per
closure. This seems acceptable.

### Dynamic effect checking overhead

Checking `closure.effects & ~bound == 0` at every call boundary with an
effect bound — is this too expensive? It's one AND + one branch. Probably
fine, but worth measuring.

### Interaction with the type system

Elle doesn't have a static type system (yet). Effects are the closest thing
to static types. Should they evolve toward a type system, or remain a
separate concern?

### Handler resumption

Can a handler resume the signaling fiber with a value (like CL restarts)?
Janet's model supports this — `resume` delivers a value to the suspended
fiber. Algebraic effects require it. We should support it.

### Effect subtyping

Is `:error` a subtype of `:condition`? Should there be a hierarchy of signal
types, or is the flat bitfield sufficient? Janet uses a flat space. Koka uses
a hierarchy. Flat is simpler and faster.

### Backward compatibility

Existing Elle code uses `handler-case`, `yield`, `make-coroutine`, etc.
These should continue to work as sugar over the new mechanism. The migration
should be invisible to existing programs.
