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
higher-order ops `map` and `filter`.

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

## When it is legal — the gate

Fusion preserves the program's value and, for a single op (`map` or `filter`),
its exact per-element evaluation order (the loop visits each element left to
right, applying `f`/`p` identically to the stdlib op). The gate:

- **The callee is the canonical stdlib `map` or `filter`.** Recognized by the
  callee binding being `is_primitive` (every stdlib export is bound so by
  `bind_primitives`; a user redefinition shadows it with a non-primitive binding)
  and named `map` or `filter`. A user redefinition is never rewritten.
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
  The immutable-array proof selects the frozen-result arm; a mutable `@array`
  input (keyword `@array`) is left to the stdlib `map` (its result aliases the
  input's mutability — handled by the general path, not here).
- **`f`/`p` is a non-capturing single-parameter lambda** written directly as the
  call's argument. No captures means the body references only its parameter
  and globals, so splicing it at the call site is always in scope; a single
  fixed parameter (no rest) means the element binds 1:1. The lambda is consumed
  by the rewrite (moved out of the call), so no other use of it can observe the
  change.

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
  mixed `filter`-of-`map` form and assert on the lowered HIR: the HOF callee is
  gone, the body op appears inline in the loop, the composed case has a single
  accumulator with no intermediate, `filter` emits the guarded push (an `if`), and
  a mixed chain fuses both ops into one loop (one accumulator, both body ops
  inline). Decline pins guard the gate (user-shadowed callee, capturing lambda,
  unproven collection, raw-intrinsic body, and a non-reorder-safe mixed chain,
  which declines composition and fuses its inner run only).
- **Realization (execution).** `tests/elle/dissolution-map-alloc.lisp` (the filter
  cases in `dissolution-filter-fuse.lisp`, and the mixed cases in
  `dissolution-mixed-fuse.lisp`) prove the consequence the mission names — *fewer
  allocations*. It measures
  `arena/total-allocs` (a **cumulative, monotonic** count of objects ever minted;
  `src/value/fiberheap/`) around a fused chain versus an un-fused reference
  computing the same value, and asserts the fused form mints strictly fewer, with
  the saving scaling per composition layer (one intermediate array each — for a
  mixed chain, the survivor/mapped array between the two ops). The
  intermediate is non-escaping and freed before the call returns, so it is
  invisible to every live/peak/steady-state axis — the leak oracle included; only
  a cumulative allocation-event count sees it, and it is deterministic (no
  GC-timing noise), so these are exact `<` relations.
- **Value + soundness.** `tests/elle/dissolution-map-fuse.lisp`,
  `dissolution-filter-fuse.lisp`, and `dissolution-mixed-fuse.lisp`
  (value-preserving, incl. the declined shapes and the reorder-gate fallback) and
  `tests/elle/region-map-fuse-uaf.lisp` / `region-filter-fuse-uaf.lisp` /
  `region-mixed-fuse-uaf.lisp` (guardfree over heap element/base values).

The leak oracle is only a non-regression check here.
