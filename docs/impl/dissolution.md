# Dissolution — HOF loop fusion

Dissolution is the third leg of the region system (see `memory.md` § "The three
legs"): a closure is a first-class *value* but the *unit of nothing* at runtime,
and — guided by the escape/ownership facts legs 1 and 2 infer — the compiler
realizes a higher-order call as its most efficient form. `(map f xs)` over an
owned, non-escaping `xs` exposes no observable closure and no observable
intermediate collection, so the compiler is free to realize it as a plain loop
with `f`'s body spliced in, a JIT'd group, CPU SIMD, or a device dispatch. This
document specifies the first realization: **HOF-chain loop fusion** on the VM
substrate (`src/hir/typeinfer/fuse.rs`), covering the two array-producing
higher-order ops `map` and `filter` and the scalar-producing left-fold
`fold`/`reduce`.

## What the pass does

At a call `(map f xs)` where `xs` is a statically-proven immutable array and `f`
is a non-capturing single-parameter lambda written at the call site, the pass
rewrites the call to the index-walk loop `map`'s own array arm runs
(`src/stdlib.lisp`, the `(array? coll)` arm) — but with **`f`'s body spliced
directly into the loop body** instead of called through a closure value:

```
(map (fn [x] BODY) [ … ])
⇒
(let [coll [ … ]]
  (let [len (length coll)]
    (let [acc (@array)]
      (define i 0)
      (while (< i len)
        (push acc (let [x (get coll i)] BODY))
        (assign i (+ i 1)))
      (freeze acc))))
```

The closure `f` is gone: no closure value is allocated and no indirect call
through one happens per element — `BODY` runs inline against a let-bound element.
The `map` dispatch (a cross-unit stdlib call whose `cond` selects the array arm)
is gone too. The emitted form is *surface* HIR — the same shape `map`'s body has
before functionalization — so every downstream pass (functionalize, ANF, region
inference, the lowerer) consumes it exactly as it consumes `map`'s own body: the
`while` becomes a `loop`/`recur`, `push` monomorphizes to `%push-array-mut` on
the proven `@array` accumulator, and the result is one frozen array.

## Composition — the intermediate collection dissolves

A chain `(map g (map f xs))` fuses to a **single** loop. The pass peels the
chain down to its base array `xs`, collecting the per-element transforms
`[f, g]` in application order, and emits one accumulator loop whose element
expression is the transforms nested innermost-first:

```
(push acc (let [gp (let [fp (get coll i)] F-BODY)] G-BODY))
```

The intermediate array that the inner `map` would have allocated, frozen, and
the outer `map` would have walked **never exists** — one loop, one accumulator,
no intermediate. This is the shape a `map`-tower reduces to; the depth of the
chain becomes the nesting depth of one element expression, not a stack of
allocations.

## Filter — the conditional push

`filter` shares `map`'s scaffold — the same `(get`/`push`/`freeze)` index-walk
over the base's array arm — and differs only in the per-element loop body. Where
`map` pushes `f`'s *result*, `filter` pushes the *element itself*, gated by the
predicate (mirroring `filter`'s own array arm, `src/stdlib.lisp`):

```
(filter (fn [x] PRED) [ … ])
⇒
  (while (< i len)
    (push acc … )   ⟶   (let [item (get coll i)]
                          (if (let [x item] PRED) (push acc item) nil))
    …)
```

A `filter`-of-`filter` chain nests the guards innermost-first — the element is
bound once and pushed only when every predicate passes:

```
(let [item (get coll i)]
  (if (let [p item] P-BODY)
    (if (let [q item] Q-BODY) (push acc item) nil)
    nil))
```

The predicate closures dissolve exactly as `map`'s transform closures do, and the
accumulator/`freeze` are identical, so the result is one frozen array of the
survivors with no per-element closure and no `filter` dispatch.

## Mixed chains — one loop

A chain need not be homogeneous. `(map f (filter p xs))` and `(filter q (map g
xs))` — any mix of `map` and `filter` over the same proven base — fuse to a
**single** loop through one unified transform/guard pipeline. Each op in the
chain is a *stage*: a `map` stage transforms the threaded element value; a
`filter` stage binds the current value once and continues the pipeline only when
its predicate passes. The stages nest in application order (innermost op first),
bottoming out at the push. For `(map f (filter p xs))`:

```
(let [item (get coll i)]
  (if (let [p item] P-BODY)              ; filter p — does it survive?
    (push acc (let [x item] F-BODY))     ; map f — push the transform of the survivor
    nil))
```

and for `(filter q (map g xs))` the map stage transforms first and the guard
tests the transformed value:

```
(let [v (let [x (get coll i)] G-BODY)]   ; map g — the transformed value
  (if (let [q v] Q-BODY) (push acc v) nil))
```

The intermediate array the inner op would have allocated — the survivors between
`filter` and `map`, or the mapped values between `map` and `filter` — never
exists, exactly as it does not for a homogeneous chain. `map`-only and
`filter`-only chains are the two ends of this one pipeline (all-transform stages,
or all-guard stages); the builder (`build_loop`/`Build::element`) is the same for
all three.

## Fold — the scalar terminal

`map` and `filter` each *collect* their per-element results into a fresh
immutable array; that array is the pipeline's **terminal**. `fold`/`reduce`
replaces the terminal with a **scalar accumulator**. `(fold f init xs)` — `f`
called `(f acc element)`, the same left-fold `src/core.lisp`'s `fold` runs
(`reduce` is `(def reduce fold)`, the identical op recognized by either name) —
dissolves to:

```
(fold (fn [acc x] STEP) INIT [ … ])
⇒
(let [seed INIT]
  (let [coll [ … ]]
    (let [len (length coll)]
      (define acc seed)
      (define i 0)
      (while (< i len)
        (assign acc (let [acc-p acc] (let [x (get coll i)] STEP)))
        (assign i (+ i 1)))
      acc)))
```

No `@array`, no `push`, no `freeze`: the accumulator is a reassigned scalar
seeded by `init`, updated one left-fold step per element, and the result is its
final value. `f`'s two parameters bind 1:1 — the accumulator param to the
current `acc`, the element param to `(get coll i)` — and its body is spliced
inline exactly as a `map` transform is. `init` is bound to an immutable `seed`
**outermost**, before the base collection, so it evaluates in the source order
of `(fold f init coll)` (init before coll) even though the loop needs `coll`
and `len` first.

Fold is always the **outermost/terminal** op — its scalar result is not a
collection, so no `map`/`filter` chains over it. So the pipeline is unchanged
between the two terminals; only the accumulator setup and the per-element base
case differ. `(fold f init (map g xs))` / `(fold f init (filter p xs))` — and any
map/filter prefix — fuse to **one** loop whose base case is the fold step instead
of the push, with **no intermediate array** between the inner ops and the fold.
This is map-reduce: the canonical parallel-reduction shape and the reason to prove
this leg. `Build::element` threads the value through the map/filter stages and its
base case is the terminal — a `push` (Collect) or a fold `assign` (Fold); the
recursion is otherwise identical.

## When it is legal — the gate

Fusion preserves the program's value and, for a single op (`map` or `filter`),
its exact per-element evaluation order (the loop visits each element left to
right, applying `f`/`p` identically to the stdlib op). The gate:

- **The callee is a canonical stdlib HOF.** A pipeline op is `map` or `filter`;
  the optional outermost terminal op is `fold` or `reduce`. Recognized by the
  callee binding being `is_primitive` (every stdlib/core export is bound so by
  `bind_primitives`, and the canonical core-env override is marked `is_primitive`
  too, so `fold`/`reduce` reach the gate exactly as `map`/`filter` do; a user
  redefinition shadows with a non-primitive binding) and named accordingly. A user
  redefinition is never rewritten.
- **`xs` is a proven immutable array.** One of: an array literal (`[ … ]`, which
  analyzes to a call to the `array` primitive, `RetType::Array`) or any
  `RetType::Array` primitive call at the call site; a `Var` alias whose
  initializer is such an array, followed through immutable, unmutated,
  singly-bound `let`/`def` bindings to a fixpoint; or another fusable same-HOF
  chain over such a base. The alias proof is the **same** one dead-arm pruning reads at
  this stage — the binding→concrete-`type-of`-keyword map `prune.rs` builds
  (`classify_init`/`resolve`); a base whose keyword resolves to `array` is a
  proven immutable array. Reusing that map is not incidental: an over-broad
  classification there deletes a live match arm (a UAF), so the map already
  carries the soundness bar fusion needs (an over-broad base is a miscompile).
  A base whose keyword resolves to `array` selects the frozen-result arm; a
  mutable `@array` base (keyword `@array`, or a `RetType::MutableArray` producer
  call) selects the unfrozen-result arm under the tighter gate below (see
  "The mutable-array arm").
- **The lambda is non-capturing with the op's fixed arity** — one parameter for
  a `map`/`filter` (the element), two for a `fold` (the accumulator and the
  element) — written directly as the call's argument, with no rest parameter.
  No captures means the body references only its parameters and globals, so
  splicing it at the call site is always in scope; the fixed parameter count
  means the loop's element (and, for a fold, the accumulator) bind 1:1. The lambda
  is consumed by the rewrite (moved out of the call), so no other use of it can
  observe the change.

For a **composition** (a chain of length ≥ 2 — homogeneous *or* mixed), the pass
additionally requires each lambda body to be free of **sequencing effects** — no
yield, I/O, emit, FFI, or halt (`reorder_safe`): composition interleaves the
per-element work (`f x0; g …; f x1; g …`, or `p x0; q …; p x1; q …`) rather than
running all of the first op then all of the second, so it reorders that work. The
*value* is unchanged either way — each stage still runs on exactly the elements it
would have (the outer op on the inner's outputs, left to right: a `filter` on its
predecessor's survivors/mapped values, a `map` on its predecessor's survivors);
only the *interleaving* of the two lambdas' calls differs, which the gate makes
unobservable. This is why a mixed chain is gated identically to a homogeneous one:
a mixed chain is always length ≥ 2, so it always carries the reorder requirement,
and a non-reorder-safe stage (a variadic comparison like `>`, which routes through
`apply`) declines the whole composition — the chain then falls back to fusing only
its inner reorder-safe run. Reordering is observable only through a sequencing
effect, and a non-capturing lambda's only cross-element channel is one; a body
with none reorders unobservably. `SIG_ERROR` is
deliberately permitted — error reordering changes only *which* of several errors
surfaces (each still surfaces as an error), and a dissolvable numeric kernel over
proven data does not error; refusing it would forbid every arithmetic tower, the
shape this fusion exists to collapse. A single op never reorders and carries no
such requirement.

The **fold counts as an op** in the chain length. A lone `fold` (`(fold f init
xs)`, length 1) threads its accumulator strictly in element order — exactly the
stdlib fold — so it never reorders and needs no gate, even with a
sequencing-effectful body. A fold *with* an inner `map`/`filter` prefix is length
≥ 2 and carries the reorder requirement over every lambda (the fold's and the
prefix's): the fold step interleaves with the prefix transforms per element
(`g x0; f …; g x1; f …`) rather than running the whole prefix first, the same
reorder a mixed chain makes. A non-reorder-safe stage declines the whole
composition, and the pass falls back to fusing the inner reorder-safe run (the
prefix), leaving the fold a plain call over the fused loop.

## The mutable-array arm

`map`/`filter` are **type-preserving**: over an immutable array they return a
frozen array, but over a **mutable** `@array` they return the accumulator
*unfrozen* — the stdlib arm is literally `(if (mutable? coll) acc (freeze acc))`
(`src/stdlib.lisp`). Fusion mirrors this. When the base's proof resolves to the
`@array` keyword — a `@[ … ]` literal, a `RetType::MutableArray` producer call
(`thaw`, …), or a `Var` alias of one — the base is **statically** known mutable
(`freeze` never mutates in place; it copies to a *new* immutable value and leaves
its input mutable, so a proven-`@array` binding is mutable at every use), so the
fused loop emits the accumulator **unfrozen** instead of `(freeze acc)`. The
loop body is otherwise identical to the immutable arm.

A mutable base fuses under a **strictly tighter gate: a single `map` or `filter`
only** — no `fold`, no composition. The reason is that the fused loop walks the
base *live* (it reads `(get coll i)` each iteration against a `len` captured
once), and this matches the stdlib op **exactly** for a single `map`/`filter`
(whose own array arm captures `len` once and reads `coll` live) — so the value is
preserved even if the lambda mutates the base through a global alias. The two
excluded shapes break that match:

- **`fold`** first snapshots its input (`(->array coll)` copies a mutable array)
  and walks the copy; a fused fold would walk the live base, so a mutating
  combinator would observe a divergence. A `fold` over a mutable base stays a
  plain call.
- **A composition** (`(map g (filter p @xs))`, …) runs each stdlib op to
  completion over a *fresh* array before the next begins, so a later op's lambda
  mutating the original base can no longer affect the result; the single fused
  loop interleaves the ops against the live base, where such a mutation *would*
  affect later reads. A composition over a mutable base declines, and the
  pre-order recursion still fuses its innermost single `map`/`filter` (sound in
  isolation), leaving the outer ops as plain calls over that fused loop.

For an **immutable** base neither hazard exists — the base cannot be mutated — so
`fold` and compositions fuse over it exactly as before.

Signals on the synthesized helper calls (`get`/`push`/`freeze`/`<`/`+`/
`length`/`@array`) and on the synthesized `if`/`let` scaffolding are set to the
original call's signal — a sound upper bound (that call's signal already subsumes
every op in the stdlib op's body) — so the bottom-up signal re-propagation
(`hir/narrow.rs`) never under-reports the fused form's effects. The spliced lambda
bodies keep their own signals.

## Why a call-site rewrite, not a stdlib edit

Dissolution is one mechanism keyed on proven-owned-non-escaping inputs, not a
per-function hand-rewrite. The production `zip` was hand-fused once for an RSS
win; that is exactly the manual gap-bridging this pass exists to replace. The
pass fires structurally on *any* `(map f xs)` matching the gate — it enumerates
no user function and hand-writes no composition. It mirrors the container-
dispatch monomorphization (`src/hir/typeinfer/monomorphize.rs`): recognize a
proven-type call across the compile-unit boundary and collapse it to the direct
form the proof selects, so the general dispatch and everything it strands cease
to exist.

## Where it runs

In `regularize` (`src/hir/regularize.rs`), after dead-arm pruning and **before**
`functionalize`, on surface HIR. Running pre-functionalize is what lets the pass
emit ordinary `while`/`push`/`freeze` HIR and hand the loop lowering to the same
machinery that lowers `map`'s own body — the pass never constructs a
`loop`/`recur` or a capture cell by hand. The proven-immutable-array fact it
needs is the same one dead-arm pruning reads at this stage
(`typeinfer/prune.rs`, `classify_init`): a literal's constructor, or a primitive
call's declared `RetType`.

## The gauge

Dissolution is a **realization** goal, not a leak goal — it is proven at the
codegen and execution levels, not on the leak oracle. Three pins:

- **Codegen (structure).** `src/hir/typeinfer/fuse.rs` `mod tests` compile a `map`
  / `map`-of-`map` / `filter` / `filter`-of-`filter` / mixed `map`-of-`filter` /
  mixed `filter`-of-`map` / `fold` / `fold`-of-`map` / `fold`-of-`filter` form and
  assert on the lowered HIR: the HOF callee is gone, the body op appears inline in
  the loop, the composed case has a single accumulator with no intermediate,
  `filter` emits the guarded push (an `if`), a mixed chain fuses both ops into one
  loop (one accumulator, both body ops inline), and a fold dissolves to a scalar
  accumulator (no `@array`/`freeze`, the fold step inline) that composes with a
  map/filter prefix into one loop. A mutable-`@array`-base `map`/`filter` fuses
  with the accumulator returned **unfrozen** (no `freeze` call). Decline pins
  guard the gate (user-shadowed callee, capturing lambda, unproven collection,
  raw-intrinsic body, a non-reorder-safe composition — which declines and fuses
  its inner run only — and a `fold` or composition over a mutable base, which
  declines to the innermost single op).
- **Realization (execution).** `tests/elle/dissolution-map-alloc.lisp` (the filter
  cases in `dissolution-filter-fuse.lisp`, and the mixed cases in
  `dissolution-mixed-fuse.lisp`) prove the consequence the mission names — *fewer
  allocations*. It measures
  `arena/total-allocs` (a **cumulative, monotonic** count of objects ever minted;
  `src/value/fiberheap/`) around a fused chain versus an un-fused reference
  computing the same value, and asserts the fused form mints strictly fewer, with
  the saving scaling per composition layer (one intermediate array each — for a
  mixed chain, the survivor/mapped array between the two ops; for a fold chain,
  the array the map/filter prefix would have handed the fold). A lone `fold`
  produces a scalar, so it saves no *result* array — its win appears once it
  composes with a prefix (`dissolution-fold-fuse.lisp`). The
  intermediate is non-escaping and freed before the call returns, so it is
  invisible to every live/peak/steady-state axis — the leak oracle included; only
  a cumulative allocation-event count sees it, and it is deterministic (no
  GC-timing noise), so these are exact `<` relations.
- **Value + soundness.** `tests/elle/dissolution-map-fuse.lisp`,
  `dissolution-filter-fuse.lisp`, `dissolution-mixed-fuse.lisp`, and
  `dissolution-fold-fuse.lisp`
  (value-preserving, incl. the declined shapes, the reorder-gate fallback, and the
  mutable-base arm — an unfrozen, in-place-mutable result) and
  `tests/elle/region-map-fuse-uaf.lisp` / `region-filter-fuse-uaf.lisp` /
  `region-mixed-fuse-uaf.lisp` / `region-fold-fuse-uaf.lisp` (guardfree over heap
  element/base/accumulator values, including a mutable-base heap result mutated in
  place).

The leak oracle is only a non-regression check here.
