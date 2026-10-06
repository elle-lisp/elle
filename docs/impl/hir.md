# HIR — High-level IR

<!-- audited: 2026-10-06 -->

The HIR pass converts expanded syntax trees into a typed intermediate
representation. It resolves bindings, computes captures, and infers
signal profiles.

## Key types

- **`Hir`** — a node in the HIR tree, carrying `HirKind`, source
  location, and inferred `Signal`
- **`HirKind`** — the node variant: literal, variable reference, call,
  lambda, let, if, begin, etc.
- **`Binding`** — a resolved variable reference: a `u32` index into a
  `BindingArena` (in [arena.rs](../../src/hir/arena.rs)), which holds the
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

An `HirFragment` ([fragment.rs](../../src/hir/fragment.rs)) closes the body
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
`cond`, `begin`, `and`/`or`, and a raw `%`-intrinsic) plus `let`. A `let` is
closable because its bindings are introduced in evaluation order. Each value is
rebuilt before its own binding enters the table, so a later value sees the
earlier binding and no value refers to itself. A `letrec` defeats that order,
because its value may name its own binding. Every other binding form (`loop`, a
`match` pattern, a nested `lambda`) introduces through a route this walk does
not model. All of them decline.

A fragment holds a `BindingInner` per local rather than a summary of one. So a
`let` binding's mutability and a parameter's `(numeric!)` floor travel with it
like any other field, and the whole fragment is plain serializable data. The
stdlib disk cache stores the cross-unit inline registry as fragments
([stdlib_cache.rs](../../src/compiler/stdlib_cache.rs)), so a cache hit
compiles user code to the same bytecode a stdlib compile does.

## What analysis does

1. **Binding resolution** — names → `Binding` arena indices, recording
   each binding's scope (`Parameter`/`Local`), mutation, and capture
   status in the `BindingArena`
2. **Capture analysis** — which free variables a closure captures, and
   whether they are mutable. Each capture carries a `CaptureKind`: `Local`
   (from the parent's slot), `Capture` (transitive, from the parent's own
   captures), or `Recursive` (the closure captures its *own* enclosing
   `letrec`/`def` binding — a self-reference). A `Recursive` self-edge does
   **not** mark the binding captured. A binding captured only by itself is
   therefore cell-free, and its self-reference resolves to the
   currently-executing closure (`LoadSelf` / a self-call). The lowerer reads
   this classified fact rather than re-deriving the self-edge. It carries no
   escape authority (see [impl/escape.md](escape.md))
3. **Signal inference** — interprocedural: traces call chains to
   determine whether a function can yield, error, or is silent
4. **Tail position marking** — sets `is_tail` on the calls in tail position,
   for TCO. Analysis runs it, so every `AnalyzeResult` carries honest flags:
   the linter and the call-graph builder both read `is_tail` off the analyzed
   tree. `regularize` marks again after map fusion introduces new nodes
5. **Special form analysis** — `if`, `let`, `begin`, `block`,
   `match`, `defmacro`, etc. each have dedicated handlers

## Signal inference

Three signal categories:
- **Silent** — no signal bits set and no propagation (`Signal::silent()`)
- **Yields** — has signal bits set (`:io`, `:yield`, `:error`, etc.)
- **Polymorphic** — signal behavior depends on a parameter, encoded in
  the `propagates` mask (for example, `(map f xs)` — signals depend on `f`)

`(silence)` bounds the function's own signal at compile time, and `(silence p)`
bounds a parameter, checked when the function is entered
([signals/inference.md](../signals/inference.md)). The inference propagates
through call chains interprocedurally.

## Dead binding elimination

[dead.rs](../../src/hir/dead.rs) removes a `let`/`letrec` binding when two
facts hold: the binding has zero uses, and its initializer is provably
effect-free. Removing the binding removes the initializer with it, so a dead
call to an effect-free function never reaches LIR.

The pass runs from `regularize`, after dead-arm pruning and map fusion, and
before `functionalize`. That altitude is the same one `prune_typeof_match_arms`
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
non-recursive) and `letrec` (= `letrec*`, sequential, recursive). Each surface
*body* desugars to one of them:

| Context | Body semantics |
|---|---|
| `do` / `begin` | `let*` — sequential; no forward refs / no mutual recursion |
| lambda body | `letrec*` — strict |
| file / module body | `letrec*` — strict; the body's *value* is the module |

`def` is **polymorphic sugar**: "extend the enclosing body with one binding." In
a sequential body it desugars to a nested `let`: `(do (def x 4) (def x (+ x
1)))` → `(let [x 4] (let [x (+ x 1)] x))`, and shadowing is free. In a recursive
body it is a `letrec` binding. It is a *body-level*, post-macro-expansion
rewrite, not a closed-form local macro, because a macro may produce a `def` and
the body must still collect it.

There is **no "module layer."** A module is just the body's return value. Per
[modules.md](../modules.md), a file runs as a single letrec whose last
expression is its value, with no export declarations and no special syntax, and
`import/load-file` compiles and runs it. The two things that looked like a
layer are not bound to the body at all:

- The **export projection** (`compute_signal_projection`) is an optional
  compile-time *signal-inference cache* over the returned struct. Delete it and
  modules still work, with conservative cross-file signals.
- `(signal :kw)` is an orthogonal *declaration form*. Its compile-time effect
  on the signal registry is exactly like `defmacro`'s effect on the expander.

Strip both away and the file body is **strictly `letrec*`**, which is what
modules.md already says it is.

**So `analyze_file_letrec` is an out-of-band driver to retire, not to rename.**
It implements `letrec*`-with-redefinition as a bespoke path that no `eval` or
nested form can reach. Its `deferred`-binding machinery exists only to support
in-file redefinition. Redefinition has no coherent meaning, because forward
references bind the first definition and later references the second, so it
should be a duplicate-definition error.

The target is the ordinary `letrec*` kernel. The loader collects file forms
into a `letrec`, through the same `def`-as-body sugar and gensym-expr encoding,
and runs them through the normal expand→analyze→emit→`eval` path, which is what
`import/load-file` already does. A user-replaceable top-level wrapper
(Racket `#%module-begin`-style) would be a macro named in valid Elle, such as
`body*`, whose default expansion is exactly that `letrec`. It cannot be
`#%body`, because `#` is the comment character and `%` the intrinsic prefix.

Two consequences fall out. The file body stops being special. And its heap
literals become **ordinary allocations** (`MaterializeConst`) into the file
body's own per-activation regions, freed by the termination sweep, which closes
the per-`eval` leak ([region/model.md](region/model.md), *Constants lower as
ordinary allocations*).

## Files

| File | Holds |
|------|-------|
| [expr.rs](../../src/hir/expr.rs) | `Hir` and `HirKind` |
| [analyze/mod.rs](../../src/hir/analyze/mod.rs) | The analysis entry point |
| [analyze/binding.rs](../../src/hir/analyze/binding.rs) | Binding resolution |
| [analyze/forms.rs](../../src/hir/analyze/forms.rs) | Special form handlers |
| [analyze/special.rs](../../src/hir/analyze/special.rs) | More special forms |
| [tailcall.rs](../../src/hir/tailcall.rs) | Tail position marking |
| [dead.rs](../../src/hir/dead.rs) | Dead binding elimination |

---

## See also

- [impl/lir.md](lir.md) — lowering HIR to LIR
- [impl/reader.md](reader.md) — parsing before analysis
- [signals](../signals/index.md) — signal system design
