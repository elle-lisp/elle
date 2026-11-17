# HIR — High-level IR

<!-- audited: 2026-11-17 -->

The HIR pass converts expanded syntax trees into a typed intermediate
representation. It resolves bindings, computes captures, and infers
signal profiles.

## Key types

- **`Hir`** — a node in the HIR tree, carrying `HirKind`, source
  location, and inferred `Signal`
- **`HirKind`** — the node variant: literal, variable reference, call,
  lambda, let, if, begin, etc.
- **`Binding`** — a resolved variable reference: a `u32` index into a
  `BindingArena` ([the arena](../../src/hir/arena.rs)), which holds the
  per-binding metadata (`BindingScope` — `Parameter` or `Local` — plus
  `is_mutated`, `is_captured`, `is_immutable`, `is_primitive`, etc.)
- **`Signal`** — inferred effect profile: a `SignalBits` set plus a
  `propagates` parameter mask (silent, yields, polymorphic, …)

## A fragment is closed over its bindings

A `Binding` addresses a `BindingArena`, and the arena is not part of the term.
An HIR body on its own therefore means nothing outside the unit that analyzed
it: its indices name a table the reader does not hold. Carrying such a body to
another unit, or to a file, means carrying the binding facts it needs. A
carrier that hoists a chosen few of them out of `BindingInner` is wrong the day
someone adds the twelfth field.

An `HirFragment` ([the fragment](../../src/hir/fragment.rs)) closes the body
over its own table instead. Every `Binding` in a fragment's body is an index
into that table, and each entry says what the binding is:

| Entry | The binding | What `graft` does with it |
|---|---|---|
| `Local(BindingInner)` | one the body introduces — a parameter, or a `let` binding | mints a fresh binding in the host arena carrying this metadata |
| `Global(SymbolId)` | a free variable, which must be a module name (`is_file_scope`) or a primitive | resolves the name against the host unit, and declines if it does not resolve |

`HirFragment::close` builds a fragment from a body, its parameters, and the
defining arena. `HirFragment::graft` re-hosts one into any arena, minting fresh
`HirId`s, because a reused id collides in the region walk's per-id side
tables. Both run over one rebuild (`rebind`), so `close` declines exactly the
forms `graft` could not rebuild. A fragment that exists always rebuilds, and the
only thing left for `graft` to refuse is a global the host unit cannot name.

The admitted forms are the pure-expression ones (literals, `Var`, `Call`, `if`,
`cond`, `begin`, `and`/`or`, `emit`, and a raw `%`-intrinsic) plus `let` and
`match`. A `let` is closable because its bindings are introduced in evaluation
order. Each value is rebuilt before its own binding enters the table, so a later
value sees the earlier binding and no value refers to itself. A `match` arm
reads the same way: its pattern's bindings enter the table before its guard and
body are rebuilt, and no pattern names a binding of its own. A `letrec` defeats
that order, because its value may name its own binding. The other binding forms
(`loop`, a nested `lambda`) introduce through a route this walk does not model.
All of them decline.

A fragment holds a `BindingInner` per local rather than a summary of one. So a
`let` binding's mutability and a parameter's `(numeric!)` floor travel with it
like any other field, and the whole fragment is plain serializable data.

A function's code payload carries the fragment of its own body as bytes, closed
before functionalization in the unit that defines it
([code objects](region/template.md)). The graft ([grafting a call](graft.md))
and loop fusion ([named functions](dissolution/inline.md)) read a callee's
fragment off the value its binding holds, so a body crosses a unit boundary with
the function itself. The stdlib disk cache stores the bytes with the payload
([the stdlib cache](stdlib-cache.md)), and a boot image maps them with it
([booting from an image](image/boot.md)). So a cache hit and an image boot
compile user code to the same bytecode a stdlib compile does.

## What analysis does

1. **Binding resolution** — names → `Binding` arena indices, recording
   each binding's scope (`Parameter`/`Local`), mutation, and capture
   status in the `BindingArena`
2. **Capture analysis** — which free variables a closure captures, and
   whether they are mutable. Each capture carries a `CaptureKind`: `Local`
   (from the parent's slot), `Capture` (transitive, from the parent's own
   captures), or `Recursive` (the closure captures its *own* enclosing
   `letrec`/`def` binding — a self-reference). A `Recursive` self-edge does
   **not** mark the binding captured, so a binding captured only by itself is
   cell-free and its self-reference resolves to the currently-executing closure
   (`LoadSelf` / a self-call). The lowerer reads this classified fact rather than
   re-deriving the self-edge. It carries no escape authority (see
   [escape analysis](escape.md)). The graft appends a capture after analysis
   when a body it grafts into a lambda names a module binding the lambda does
   not hold ([the capture a graft adds](graft/captures.md))
3. **Signal inference** — interprocedural: traces call chains to
   determine whether a function can yield, error, or is silent
4. **Tail position marking** — sets `is_tail` on the calls in tail position,
   for TCO. Analysis runs it, so every `AnalyzeResult` carries honest flags:
   the linter and the call-graph builder both read `is_tail` off the analyzed
   tree. `regularize` marks again after map fusion and the graft introduce new
   nodes
5. **Special form analysis** — `if`, `let`, `begin`, `block`,
   `match`, `fn`, etc. each have dedicated handlers, listed in
   [the special-form registry](../../src/hir/analyze/forms/registry.rs)

## Signal inference

Every function carries a `Signal`: the bits it may emit, and the parameters
whose signal it passes on. It is silent when both are empty, and polymorphic
when it passes a parameter's signal on, as `(map f xs)` passes on `f`'s.
`(attune! spec)` and `(silent!)` check a function's own signal, and
`(bound p spec)` bounds a parameter's. The inference propagates through call
chains interprocedurally, and [signals/inference.md](../signals/inference.md)
owns its rules.

## Dead binding elimination

[Dead binding elimination](../../src/hir/dead.rs) removes a `let`/`letrec`
binding when two facts hold: the binding has zero uses, and its initializer is
provably effect-free. Removing the binding removes the initializer with it, so a
dead call to an effect-free function never reaches LIR.

The pass runs from `regularize`, after dead-arm pruning, map fusion and the
graft, and before `functionalize`. That altitude is the same one `prune_typeof_match_arms`
uses, for the same reason: the region solver runs after the HIR transforms, so
a call deleted here never mints a region and strands no release obligation.

### A silent function is not a pure function

`Signal::silent()` says a function emits no signal bits. It does **not** say the
function has no effect. Every `%`-intrinsic is registered silent, and
`%push-array-mut` appends to its argument in place. A rule that eliminated any
silent call would delete that append and change the program's output.

So the pass proves effect-freedom, not silence, and it proves it from two
facts that cannot lie:

- **The node's signal is `Signal::silent()`.** The analyzer combines the callee
  signal with every argument's signal. A silent call node therefore also means
  every argument is silent, and that a polymorphic callee got silent arguments.
  A yielding callback handed to an otherwise-silent higher-order function shows
  up here and blocks elimination.
- **The callee stores nothing.** For a primitive, `RegionEffect::Immediate` and
  `RegionEffect::Fresh` are the two declarations that state no argument is
  stored anywhere outliving the call. `moves_out` marks the natives that remove
  an element from a container argument. In-place mutation is a store into an
  argument, so `Funnel`, `Stores`, `Sends`, `Mixed`, and `PassThrough` are all
  rejected. For a user-defined callee, the same predicate runs over its lambda
  body, to a fixpoint.

The fixpoint starts with nothing proven pure and grows. A self-recursive
function therefore never proves pure, which keeps the pass out of the
termination question: it can only delete calls that provably return.

### What the pass declines to touch

- **File-scope bindings** (`is_file_scope`). A module's value is its body's last
  expression, but the top-level names are also what `(environment)` reflects.
- **`Define` nodes.** A `Define` evaluates to its own value, so it can be the
  result of the enclosing body. Deleting one can change that result; deleting a
  `let` binding cannot.
- **Mutated, synthetic, primitive, and cell-materialized bindings.** An `assign`
  records a def rather than a use, so a mutated binding can read as unused while
  an `Assign` node still names it.
- **Lambda initializers.** Binding an unused closure allocates and does nothing
  else, so removing it would be sound. The pass leaves it for now; the win is
  small and the blast radius across the corpus is not.

## Kernel and sugar (design target)

`HirKind` is currently a **wide** vocabulary. `functionalize` and `anf_lift`
normalize a few constructs (`while → loop/recur`, `assign → ssa-let/setcell`,
captured `var → derefcell`), and ANF names allocating subexpressions. They
eliminate almost nothing: the lowerer still matches every variant, so `cond`,
`match`, `and`/`or`, `begin` and `destructure` reach LIR intact. The design
direction is a real desugaring boundary, so that the lowerer consumes only the
irreducible **kernel**:

- **kernel — spine:** `var`, `let`, `letrec`, `lambda`, `call`, `if`, literals/`quote`
- **kernel — for the region model, not semantics:** `loop`/`recur`, `return`
  (semantically reducible to letrec + tail-call, kept primitive because
  per-iteration region reuse and the ownership boundary need to name them)
- **kernel — state/effect/FFI:** `makecell`/`derefcell`/`setcell`, `emit`,
  `intrinsic`, `eval`
- **kernel — control:** `block`/`break`, `parameterize`
- **sugar (should desugar into the kernel):** `cond`→`if`, `and`/`or`→`if`+`let`,
  `match`→`if`+gets, `destructure`→`intrinsic` gets, `begin`→`let`-chain,
  `while`→`loop`, `assign`→ssa/`setcell`, **`do`/`def`**→`let`/`letrec`

What lowering needs to name sets the kernel boundary, not surface convenience.
That is why `loop` and `return` are kernel although they are semantically
derivable.

## Bodies and `def`

There are exactly two binding kernels: `let` (= `let*`, sequential,
non-recursive) and `letrec` (= `letrec*`, sequential, recursive). A `def` in a
function body or a file body is a `letrec` of one binding over the rest of that
body: its value may name it from inside a lambda, and the forms after it read
it. `begin` opens no scope, so a `def` inside it extends the body around it.
[bindings.md](../bindings.md) owns the rules a program sees.

The body binds each `def` when it reaches it. It collects the `def` after macro
expansion, not as a closed-form local macro, because a macro may produce a
`def`. A read before the `def` resolves outside the body, so a function written
before it cannot call it. Each `def` of a name makes a binding of its own.

There is **no "module layer."** A module is just the body's return value. Per
[modules.md](../modules.md), a program file runs as a body whose last
expression is its value, with no export declarations and no special syntax. A
module file is one form, which needs no file body. `(signal :kw)` is an
orthogonal *declaration form*, and its compile-time effect on the signal
registry is exactly like `defmacro`'s effect on the expander. Strip it away and
the file is a body.

`analyze_file_letrec` ([the file letrec](../../src/hir/analyze/fileletrec.rs))
analyzes a file's forms, and the `%file-body` thunk of a whole-module test
reaches it too. No other form takes that path. The target is the ordinary body:
a file's forms compile through the path a function body takes. A
user-replaceable top-level wrapper (Racket `#%module-begin`-style) would be a
macro named in valid Elle, such as `body*`, whose default expansion is exactly
that body. It cannot be `#%body`, because `#` is the comment character and `%`
the intrinsic prefix.

## Files

- [Hir and HirKind](../../src/hir/expr.rs) — the node definitions
- [the analyzer](../../src/hir/analyze/mod.rs) — the analysis entry point
- [binding resolution](../../src/hir/analyze/binding.rs)
- [special forms](../../src/hir/analyze/forms.rs), and
  [more special forms](../../src/hir/analyze/special.rs)
- [the fragment](../../src/hir/fragment.rs) — a body closed over its bindings
- [tail marking](../../src/hir/tailcall.rs)
- [dead binding elimination](../../src/hir/dead.rs)

---

## See also

- [LIR](lir.md) — lowering HIR to LIR
- [the reader](reader.md) — parsing before analysis
- [signals](../signals/index.md) — signal system design
