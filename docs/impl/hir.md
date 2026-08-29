# HIR — High-level IR

The HIR pass converts expanded syntax trees into a typed intermediate
representation. It resolves bindings, computes captures, and infers
signal profiles.

## Key types

- **`Hir`** — a node in the HIR tree, carrying `HirKind`, source
  location, and inferred `Signal`
- **`HirKind`** — the node variant: literal, variable reference, call,
  lambda, let, if, begin, etc.
- **`Binding`** — a resolved variable reference: a `u32` index into a
  `BindingArena` (in `arena.rs`), which holds the per-binding metadata
  (`BindingScope` — `Parameter` or `Local` — plus `is_mutated`,
  `is_captured`, `is_immutable`, `is_primitive`, etc.)
- **`Signal`** — inferred effect profile: a `SignalBits` set plus a
  `propagates` parameter mask (silent, yields, polymorphic, …)

## What analysis does

1. **Binding resolution** — names → `Binding` arena indices, recording
   each binding's scope (`Parameter`/`Local`), mutation, and capture
   status in the `BindingArena`
2. **Capture analysis** — which free variables a closure captures,
   whether they are mutable. Each capture carries a `CaptureKind`: `Local`
   (from the parent's slot), `Capture` (transitive, from the parent's own
   captures), or `Recursive` (the closure captures its *own* enclosing
   `letrec`/`def` binding — a self-reference). A `Recursive` self-edge does
   **not** mark the binding captured, so a binding captured only by itself is
   cell-free and its self-reference resolves to the currently-executing closure
   (`LoadSelf` / a self-call); the lowerer reads this classified fact rather than
   re-deriving the self-edge. It carries no escape authority (see
   [impl/escape.md](escape.md))
3. **Signal inference** — interprocedural: traces call chains to
   determine whether a function can yield, error, or is silent
4. **Tail position marking** — sets `is_tail` on the calls in tail position,
   for TCO. Analysis runs it, so every `AnalyzeResult` carries honest flags:
   the linter and the call-graph builder both read `is_tail` off the analyzed
   tree, and `regularize` marks again after map fusion introduces new nodes
5. **Special form analysis** — `if`, `let`, `begin`, `block`,
   `match`, `defmacro`, etc. each have dedicated handlers

## Signal inference

Three signal categories:
- **Silent** — no signal bits set and no propagation (`Signal::silent()`)
- **Yields** — has signal bits set (`:io`, `:yield`, `:error`, etc.)
- **Polymorphic** — signal behavior depends on a parameter, encoded in
  the `propagates` mask (e.g., `(map f xs)` — signals depend on `f`)

`silence` constrains a parameter to be silent at compile time.
The inference propagates through call chains interprocedurally.

## Dead binding elimination

`src/hir/dead.rs` removes a `let`/`letrec` binding when two facts hold: the
binding has zero uses, and its initializer is provably effect-free. Removing
the binding removes the initializer with it, so a dead call to an effect-free
function never reaches LIR.

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
  signal with every argument's signal, so a silent call node also means every
  argument is silent, and that a polymorphic callee got silent arguments. A
  yielding callback handed to an otherwise-silent higher-order function shows up
  here and blocks elimination.
- **The callee stores nothing.** For a primitive, `RegionEffect::Immediate` and
  `RegionEffect::Fresh` are the two declarations that state no argument is
  stored anywhere outliving the call, and `moves_out` marks the natives that
  remove an element from a container argument. In-place mutation is a store into
  an argument, so `Funnel`, `Stores`, `Sends`, `Mixed`, and `PassThrough` are all
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

`HirKind` is currently a **wide** vocabulary: `functionalize` and `anf_lift`
normalize a few constructs (`while → loop/recur`, `assign → ssa-let/setcell`,
captured `var → derefcell`) and ANF names allocating subexpressions, but they
eliminate almost nothing — the lowerer still matches all ~39 variants, so
`cond`, `match`, `and`/`or`, `begin`, `destructure` reach LIR intact. The design
direction is a real desugaring boundary so the lowerer consumes only the
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

The kernel boundary is set by *what lowering needs to name* (hence `loop`/`return`
are kernel despite being semantically derivable), not by surface convenience.

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
a sequential body it desugars to a nested `let` (`(do (def x 4) (def x (+ x 1)))`
→ `(let [x 4] (let [x (+ x 1)] x))` — shadowing is free); in a recursive body it
is a `letrec` binding. It is a *body-level*, post-macro-expansion rewrite (a
`def` may be produced by a macro and must still be collected), not a closed-form
local macro.

There is **no "module layer."** A module is just the body's return value — per
[modules.md](../modules.md), a file *"runs as a single letrec, and whatever its
last expression evaluates to becomes the return value"*, with no export
declarations and no special syntax; `import-file` is *"essentially
`(eval (slurp path))`"*. The two things that looked like a layer aren't bound to
the body at all: the **export projection** (`compute_signal_projection`) is an
optional compile-time *signal-inference cache* over the returned struct (delete
it and modules still work, just with conservative cross-file signals), and
`(signal :kw)` is an orthogonal *declaration form* with a compile-time effect on
the signal registry, exactly like `defmacro`'s effect on the expander. Strip
both away and the file body is **strictly `letrec*`** — which is what
modules.md already says it is.

**So `analyze_file_letrec` is an out-of-band driver to retire, not to rename.**
It implements `letrec*`-with-redefinition as a bespoke path no `eval`/nested form
can reach, and its `deferred`-binding machinery exists only to support in-file
redefinition — which has no coherent meaning (forward references bind the first
definition, later references the second) and should instead be a
duplicate-definition error. The target is just the ordinary `letrec*` kernel:
the loader collects file forms into a `letrec` (via the same `def`-as-body sugar
and gensym-expr encoding) and runs them through the normal
expand→analyze→emit→`eval` path — which is what `import-file` already does. If a
user-replaceable top-level wrapper is wanted (Racket `#%module-begin`-style), it
is a macro named in valid Elle (e.g. `body*` — **not** `#%body`, since `#` is the
comment char and `%` the intrinsic prefix) whose default expansion is exactly
that `letrec`. Two consequences fall out: the file body stops being special, and
its heap literals are **ordinary allocations** (`MaterializeConst`) into the file
body's own per-activation regions, freed by the termination sweep (see
[region/model.md](region/model.md) — *Constants lower as ordinary allocations*),
closing the per-`eval` leak.

## Files

```text
src/hir/expr.rs           Hir and HirKind definitions
src/hir/analyze/mod.rs    Main analysis entry point
src/hir/analyze/binding.rs  Binding resolution
src/hir/analyze/forms.rs  Special form handlers
src/hir/analyze/special.rs  More special forms
src/hir/tailcall.rs       Tail position marking
src/hir/dead.rs           Dead binding elimination
```

---

## See also

- [impl/lir.md](lir.md) — lowering HIR to LIR
- [impl/reader.md](reader.md) — parsing before analysis
- [signals](../signals/index.md) — signal system design
