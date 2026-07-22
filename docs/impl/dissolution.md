# Dissolution — HOF loop fusion

Dissolution is the third leg of the region system (see `memory.md` § "The three
legs"): a closure is a first-class *value* but the *unit of nothing* at runtime,
and — guided by the escape/ownership facts legs 1 and 2 infer — the compiler
realizes a higher-order call as its most efficient form. `(map f xs)` over an
owned, non-escaping `xs` exposes no observable closure and no observable
intermediate collection, so the compiler is free to realize it as a plain loop
with `f`'s body spliced in, a JIT'd group, CPU SIMD, or a device dispatch. This
document specifies the first realization: **map-chain loop fusion** on the VM
substrate (`src/hir/typeinfer/fuse.rs`).

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

## When it is legal — the gate

Fusion preserves the program's value and, for the single-`map` case, its exact
per-element evaluation order (the loop applies `f` to each element left to
right, identically to `map`). The gate:

- **The callee is the canonical stdlib `map`.** Recognized by the callee binding
  being `is_primitive` (every stdlib export is bound so by `bind_primitives`;
  a user redefinition shadows it with a non-primitive binding) and named `map`.
  A user `map` is never rewritten.
- **`xs` is a proven immutable array.** Either an array literal (`[ … ]`, which
  analyzes to a call to the `array` primitive, `RetType::Array`) or another
  fusable `map` chain over such a base. The immutable-array proof selects the
  frozen-result arm; a mutable `@array` input is left to the stdlib `map` (its
  result aliases the input's mutability — handled by the general path, not here).
- **`f` is a non-capturing single-parameter lambda** written directly as the
  call's argument. No captures means `f`'s body references only its parameter
  and globals, so splicing it at the call site is always in scope; a single
  fixed parameter (no rest) means the element binds 1:1. The lambda is consumed
  by the rewrite (moved out of the call), so no other use of it can observe the
  change.

For a **composition** (`map`-of-`map`), the pass additionally requires each
lambda body to be free of **sequencing effects** — no yield, I/O, emit, FFI, or
halt (`reorder_safe`): composition interleaves the transforms
(`f x0; g …; f x1; g …`) rather than running all of `f` then all of `g`, so it
reorders the per-element work. Reordering is observable only through such an
effect, and a non-capturing lambda's only cross-element channel is one; a body
with none reorders unobservably. `SIG_ERROR` is deliberately permitted — error
reordering changes only *which* of several errors surfaces (each still surfaces
as an error), and a dissolvable numeric kernel over proven data does not error;
refusing it would forbid every arithmetic tower, the shape this fusion exists to
collapse. A single `map` never reorders and carries no such requirement.

Signals on the synthesized helper calls (`get`/`push`/`freeze`/`<`/`+`/
`length`/`@array`) are set to the original `map` call's signal — a sound upper
bound (that call's signal already subsumes every op in `map`'s body) — so the
bottom-up signal re-propagation (`hir/narrow.rs`) never under-reports the fused
form's effects. The spliced lambda bodies keep their own signals.

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
  / `map`-of-`map` form and assert on the lowered HIR: the `map` callee is gone,
  `f`'s body op appears inline in the loop, and the composed case has a single
  accumulator with no intermediate. Decline pins guard the gate (user-shadowed
  `map`, capturing lambda, unproven collection, raw-intrinsic body).
- **Realization (execution).** `tests/elle/dissolution-map-alloc.lisp` proves the
  consequence the mission names — *fewer allocations*. It measures
  `arena/total-allocs` (a **cumulative, monotonic** count of objects ever minted;
  `src/value/fiberheap/`) around a fused chain versus an un-fused reference
  computing the same value, and asserts the fused form mints strictly fewer, with
  the saving scaling per composition layer (one intermediate array each). The
  intermediate is non-escaping and freed before the call returns, so it is
  invisible to every live/peak/steady-state axis — the leak oracle included; only
  a cumulative allocation-event count sees it, and it is deterministic (no
  GC-timing noise), so these are exact `<` relations.
- **Value + soundness.** `tests/elle/dissolution-map-fuse.lisp` (value-preserving,
  incl. the declined shapes) and `tests/elle/region-map-fuse-uaf.lisp` (guardfree
  over heap element/base values).

The leak oracle is only a non-regression check here.
