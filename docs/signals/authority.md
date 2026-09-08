# Authority

<!-- audited: 2026-09-07 -->

What holds authority in a running program, and where the runtime asks whether a
fiber may spend it.

## The question the check asks today

A fiber carries a withheld set. Every native call tests the primitive's declared
bits against that set, and an overlapping call does not run.
[capabilities.md](capabilities.md) states the rule, the denial payload, and how a
parent mediates.

That check asks one question: **may this fiber name this primitive?** The call
site asks it, and the primitive's declaration answers it.

This document argues for a second question, asked in a different place: **may
this fiber cause this effect?** The two differ whenever authority outlives the
call that minted it, and whenever the effect leaves by a route the call site does
not own.

## The unit is the fiber

An enforcement point needs something durable to sit on. A construct the compiler
may erase enforces nothing, because the optimizer decides at build time whether
the check survives into the program at all.

A fiber is durable. It is a runtime object with identity, state, a parent, and
the withheld set itself. Capabilities flow down it at creation and at resume, a
denial parks it, and no pass rewrites it away.

```lisp
(let [outer (fiber/new
              (fn []
                (let [inner (fiber/new (fn [] (fiber/caps)) |:error| :deny |:ffi|)]
                  (fiber/resume inner)))
              |:error|
              :deny |:fs|)]
  (let [caps (fiber/resume outer)]
    (assert (not (caps :fs)) "a child cannot regain a capability its parent withheld")
    (assert (not (caps :ffi)) "a child keeps its own denial")
    (assert (caps :io) "a child keeps what nobody withheld")))
```

### A closure does not survive

The compiler dissolves closures. `(map f xs)` over a proven immutable array
becomes an index-walk loop with the body of `f` inlined, and the closure stops
existing — [dissolution.md](../impl/dissolution.md) describes the transform and
every shape it covers.

So a capability declared on a closure is one the optimizer may delete. Worse, it
deletes the check silently and only for the programs where fusion applies, which
makes enforcement depend on how hot a loop is.

### A primitive call does not always happen

Two constructs leave the dispatch path the check lives on. Arithmetic compiles to
specialized instructions that run no capability test, which
[capabilities.md](capabilities.md) states. `emit` is a bytecode instruction whose
handler never reads the withheld set, which [emit.md](emit.md) states.

A fiber therefore raises any signal bit it names, including one it does not hold.
A parent that masks a bit to watch for an operation sees whatever the child
chooses to raise, so a mask audits a cooperative child and not a hostile one.

Neither construct is a mistake on its own. Together they say the call site cannot
carry the whole check, because the language has more than one way to reach an
effect.

## Minting is not spending

The check gates the primitive that **mints** authority. It does not gate the
authority that primitive hands back.

An `io-request` is an ordinary value, with `io-request?` as its public predicate.
So is a port, and so is the module value an `import` of a native plugin returns.
Each travels through a closure capture, a resume value, a channel, or a struct
field exactly like any other value.

A fiber that holds one spends it in full. The submission API — `io/backend`,
`io/submit`, `io/wait`, `io/reap` and `io/cancel` — declares `:error` alone, so
no denial reaches a request that arrives from somewhere else.

This is the rule the model turns on: **a check on minting is not a check on
spending.** A sandbox built from denial holds only while every capability-bearing
value stays outside it, and nothing in the language marks which values those are.

## Where a fiber's work crosses out

An effect leaves a fiber by one of four routes. The table names them, and says
what the runtime asks today.

| Crossing | What crosses | Asked today |
|---|---|---|
| The scheduler | a request, submitted or emitted | nothing |
| Native code | a primitive that acts without suspending | the call site's check |
| A device | a fiber lowered and dispatched | nothing exists yet |
| A child fiber or a thread | the withheld set itself | the transitive check |

The child crossing is the one that already works, and it works because the
requirement travels with the thing that crosses. A child cannot argue its way
past a denial, because it inherits the set rather than consulting one.

## The rule

**A requirement rides the carrier that crosses, and the crossing asks the fiber
that owns it.**

The requirement is minted with the authority, so nothing can separate the two. A
value that escapes a sandbox still carries what spending it costs, and the fiber
that spends it is the fiber the crossing asks about.

This holds where a declaration does not, for three reasons. No optimizer deletes
it, because the record is data rather than a call site. No author forgets it,
because the minting primitive already declares the bits and the carrier copies
them. No plugin misstates it across the stable ABI, because the carrier records
what the operation needs rather than what its author claimed.

## What the model makes possible

### A capability for the GPU

A fiber that lowers through MLIR and runs on a device is a unit of work crossing
out of the process. The question is whether **this fiber** may run there, and the
dispatch is where the runtime asks it.

That question has no closure in it, so no fusion pass can erase the answer. It
has no plugin declaration in it either, so a device backend cannot grant itself
authority by describing itself generously.

An on-device region is the same shape one level down. A region that materializes
on a device is authority held as a value, so it carries what spending it costs
and the materialization asks the owning fiber. The carrier rule is what makes an
automatically parallelized region safe to move, because moving it moves the
requirement with it.

### Mediation, and what it does not settle

A parent that catches a denial can perform the operation and resume the child.
The seam a mediator answers is whatever primitive the implementation reached, so
a composed operation presents one seam for each helper it calls.

Moving the check to the crossing does not change that count. A composed operation
crosses once for each request it makes, so a mediator still answers at the
granularity of the implementation rather than the operation.

What the model does say is that denial retrofits a boundary onto ambient
authority. The child names a global, and the runtime intercepts whatever that
global reaches. A supervisor that hands the child an operation, rather than
denying a global the child can already name, has a seam by construction — and
the seam is the operation the supervisor wrote.

Whether the standard library should also present one deniable seam for each
composed operation stays open. The alternatives are a declaration on the
operation, which this document argues against, and a supervisor-supplied API,
which needs no runtime support at all.

## See also

- [capabilities.md](capabilities.md) — the enforcement reference: `:deny`,
  `fiber/caps`, denial payloads, mediation and refusal
- [emit.md](emit.md) — signal emission, and what it does not check
- [protocol.md](protocol.md) — the signal protocol and the registry
