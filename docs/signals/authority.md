# Authority

<!-- audited: 2026-09-21 -->

What holds authority in a running program, and where the runtime asks whether a
fiber may spend it.

## The two questions the runtime asks

A fiber carries a withheld set. Every native call tests the primitive's declared
bits against that set, and an overlapping call does not run.
[capabilities.md](capabilities.md) states the rule, the denial payload, and how a
parent mediates.

That check asks one question: **may this fiber name this primitive?** The call
site asks it, and the primitive's declaration answers it.

A second question follows: **may this fiber cause this effect?** The two differ
whenever the effect's cost rides a value rather than the primitive's name. The
runtime asks this second question at three call sites now, each reading the
requirement from an argument: a request handed to `io/submit`, a native library
named to `import`, and the bits a dynamic `emit` raises. This document states the
rule they follow, and the crossings that still ask nothing.

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

The call-site check gates the primitive that **mints** authority, against what
that primitive declares. It does not, by itself, gate authority a value carries,
because the value says what the spend costs and the declaration does not. An
`io-request` travels through a closure capture, a resume value, a channel, or a
struct field like any other value, and a path or a keyword is plainer still.

Three primitives read the requirement from the value instead of the name:

- `io/submit` reads the operation its request carries — a spawn needs
  `|:io :exec|`, an open needs `|:io :fs|`, a plain read needs `|:io|`.
- native `import` reads the path it is given — a shared library needs `:ffi`
  for the foreign `elle_plugin_init` its load runs, a `.lisp` module needs none.
- dynamic `emit` reads the bits its first argument names.

Each tests the requirement against the fiber making the call, so a fiber cannot
spend what it withholds however it obtained the value.
[capabilities.md](capabilities.md) states the checks and the denial payload.

Three edges still ask nothing, and each follows from the rule below rather than
defeats it. `io/submit` tests the submitter, so a scheduler that submits on
another fiber's behalf spends its own authority. A loaded module's primitives
carry the plugin's declared bits, so a fiber handed a module reaches them. And a
literal `emit` compiles to a bytecode instruction, not a call, so no call-site
gate reaches it. **A check on minting is not a check on spending**, and the rule
closes a route only where a crossing reads the requirement off the value.

## Where a fiber's work crosses out

An effect leaves a fiber by one of four routes. The table names them, and says
what the runtime asks today.

| Crossing | What crosses | Asked today |
|---|---|---|
| The scheduler | a request submitted to `io/submit` | the submitter's spend check |
| A signal | a dynamic `emit`, or the literal `Emit` instruction | the dynamic form's spend check; nothing on the literal |
| Native code | a primitive call, its requirement fixed or argument-derived | the call site's check, reading `import`'s `:ffi` off its path |
| A device | a fiber lowered and dispatched | nothing exists yet |
| A child fiber or a thread | the withheld set itself | the transitive check |

The child crossing has always worked, and it works because the requirement
travels with the thing that crosses. A child cannot argue its way past a denial,
because it inherits the set rather than consulting one. The three argument-derived
checks work the same way: the requirement is the value's own — the request's
operation, the import's path, the emit's bits — so the crossing reads it off the
value rather than trusting the primitive's declaration.

## The rule

**A requirement rides the carrier that crosses, and the crossing asks the fiber
that owns it.**

The requirement is minted with the authority, so nothing can separate the two. A
value that escapes a sandbox still carries what spending it costs, and the fiber
that spends it is the fiber the crossing asks about.

This holds where a declaration does not, for three reasons. No optimizer deletes
it, because the record is data rather than a call site. No author forgets it,
because the requirement is the value's own — read where the value is spent. No
plugin misstates it across the stable ABI, because the crossing reads what the
value needs rather than what its author claimed.

Three primitives are built to this rule through one seam. A primitive can declare
that its requirement depends on its arguments, and the capability gate reads it
off the argument for every tier alike: `io/submit` derives its bits from the
request's operation, `import` from the path's extension, dynamic `emit` from the
bits its argument names. The gate is the same in each case; only the derivation
is the primitive's own, so a fourth such primitive adds a derivation and changes
no gate.

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
