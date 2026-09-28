# Dissolution: the scalar terminals

<!-- audited: 2026-09-28 -->

How a fused loop ends in a scalar: `fold`, `count`, and the four searches.
`reduce` is `fold` under a second name.

The [dissolution](../dissolution.md) document owns the pass and its gate, and
[the stages](stages.md) own the ops a terminal chains over.

```lisp
(def {:fused fused :occurrences occurrences}
  (import-file "docs/impl/dissolution/hir.lisp"))
```

## Fold — the scalar terminal

`map` and `filter` each *collect* their per-element results into a fresh
immutable array; that array is the pipeline's **terminal**. `fold`/`reduce`
replaces the terminal with a **scalar accumulator**. `(fold f init xs)` — `f`
called `(f acc element)`, the same left-fold [src/core.lisp](../../../src/core.lisp)'s `fold` runs
(`reduce` is `(def reduce fold)`, the identical op recognized by either name) —
dissolves to:

```lisp
# (fold (fn [acc x] STEP) INIT [ … ]) becomes:
#
#   (let [seed INIT]
#     (let [coll [ … ]]
#       (let [len (length coll)]
#         (define acc seed)
#         (define i 0)
#         (while (< i len)
#           (assign acc (let [acc-p acc] (let [x (get coll i)] STEP)))
#           (assign i (%add i 1)))
#         acc)))

(def fold-hir (fused "(fold (fn [acc x] (+ acc x)) 0 [1 2 3])"))
(assert (= 0 (occurrences fold-hir "fold#")))
(assert (= 0 (occurrences fold-hir "(@array#")) "no accumulator array")
(assert (= 0 (occurrences fold-hir "freeze#")))
(assert (= (fold (fn [acc x] (+ acc x)) 0 [1 2 3]) 6))
```

No `@array`, no `push`, no `freeze`: the accumulator is a reassigned scalar
seeded by `init`, updated one left-fold step per element, and the result is its
final value. `f`'s two parameters bind 1:1 — the accumulator param to the
current `acc`, the element param to `(get coll i)` — and its body is spliced
inline exactly as a `map` transform is. `init` is bound to an immutable `seed`
**outermost**, before the base collection, so it evaluates in the source order
of `(fold f init coll)` (init before coll) even though the loop needs `coll`
and `len` first.

A fold is always **outermost** — its scalar result is not a
collection, so no `map`/`filter` chains over it. So the pipeline is unchanged
between the terminals; only the accumulator setup and the per-element base
case differ. `(fold f init (map g xs))` / `(fold f init (filter p xs))` — and any
map/filter prefix — fuse to **one** loop whose base case is the fold step instead
of the push, with **no intermediate array** between the inner ops and the fold.
This is map-reduce: the canonical parallel-reduction shape and the reason to prove
this leg. `Build::element` threads the value through the map/filter stages and its
base case is the terminal — a `push` (Collect), a fold `assign` (Fold), or a tally
`assign` (Count); the recursion is otherwise identical.

## Count — the terminal that is a guard plus a tally

`(count pred coll)` answers how many elements satisfy `pred`. It takes the same
`(function, collection)` shape a `filter` does and produces a **number**, so it is
a terminal exactly as `fold` is: nothing chains over it. Its fused form is the one
already built — a `filter` **stage** whose base case counts instead of pushing:

```lisp
# (count (fn [x] PRED) [ … ]) becomes:
#
#   (let [seed 0]
#     (let [coll [ … ]]
#       (let [len (length coll)]
#         (define n seed)
#         (define i 0)
#         (while (< i len)
#           (let [item (get coll i)]
#             (if (let [x item] PRED) (assign n (%add n 1)) nil))
#           (assign i (%add i 1)))
#         n)))

(def count-hir (fused "(count (fn [x] (odd? x)) [1 2 3])"))
(assert (= 0 (occurrences count-hir "count#")))
(assert (= 0 (occurrences count-hir "(@array#")))
(assert (= (count (fn [x] (odd? x)) [1 2 3]) 2))
```

The predicate is appended as the **last** stage of the pipeline (it is the
outermost op, so it runs after every inner transform/guard), and the terminal is a
scalar accumulator seeded at 0 whose base case is `(assign n (%add n 1))`. Because
the count's own stage is a guard, the value reaching that base case is always the
local a `Filter` stage binds — the tally discards a name, never work.

Even a lone `(count p xs)` saves allocations, which a lone fold does not:
`count`'s array arm walks with a `letrec`-bound self-recursive closure
([src/stdlib.lisp](../../../src/stdlib.lisp)), so the un-fused call mints that closure and its forward cell
every time — and the predicate closure on top of them wherever `p` is a lambda
literal. The fused loop mints none of the three. Over a prefix —
`(count p (map f xs))` — the intermediate array dissolves too, exactly as it does
under a fold.

`count`'s array arm errors on a string or bytes collection where `map`/`filter`
accept one, but the base gate proves the `array` keyword specifically, so the
fused form is reached only where the stdlib op would have taken its array arm.

## Search — the terminal that stops early

`any?`, `all?`, `find` and `find-index` each answer a question about the **first**
element their predicate decides and stop walking there. Each takes the same
`(function, collection)` shape a `filter` does and produces a scalar — a boolean,
an element, or an index — so each is a terminal exactly as `fold` and `count` are.

The fused form is the count's shape plus an early exit. The predicate is the
pipeline's guard stage, the accumulator is seeded with the answer for "no element
decided it", and — where the search is the chain's only op — the loop leaves
through a **sentinel the condition reads**: a `more` flag the deciding element
clears.

```lisp
# (any? (fn [x] PRED) [ … ]) becomes:
#
#   (let [seed false]
#     (let [coll [ … ]]
#       (let [len (length coll)]
#         (define ans seed)
#         (define i 0)
#         (define more true)
#         (while (and (< i len) more)
#           (let [item (get coll i)]
#             (if (let [x item] PRED)
#               (begin (assign ans true) (assign more false))
#               nil))
#           (assign i (%add i 1)))
#         ans)))

(def any-hir (fused "(any? (fn [x] (odd? x)) [2 3 4])"))
(assert (= 0 (occurrences any-hir "any?#")))
(assert (= 1 (occurrences any-hir "(and ")) "the loop condition reads the sentinel")
(assert (any? (fn [x] (odd? x)) [2 3 4]))
```

The four differ in three values, and in nothing else:

| op | seed — no element decided | the deciding element records | decided by |
|----|---------------------------|------------------------------|------------|
| `any?` | `false` | `true` | the first element the predicate **admits** |
| `all?` | `true` | `false` | the first element the predicate **rejects** |
| `find` | `nil` | the element itself | the first element the predicate admits |
| `find-index` | `nil` | its position in the walk | the first element the predicate admits |

`all?` is the one whose guard runs the other way round: its answer is decided by a
**failing** element, so the stage it appends continues the pipeline where the
predicate does *not* pass. That is a guard's other side — a `map` transforms the
threaded value, a `filter` **keeps** what its predicate admits, and an `all?`
**rejects** it — one `if` either way, differing only in which branch carries the
rest of the pipeline.

### The early exit stops the search's own work, not the pipeline's

A search fuses over a `map`/`filter` prefix as the other terminals do. What the
prefix changes is where the early exit applies. A staged `(any? p (map f xs))`
runs `f` over the **whole** input and `p` over the elements up to the decision, so
the fused loop must make exactly those calls: the walk stays exhaustive (the loop
condition is the bare range test) and the sentinel gates the **search's own guard
stage** instead.

```lisp
# (any? (fn [y] PRED) (map (fn [x] F-BODY) [ … ])) walks with:
#
#   (while (< i len)
#     (let [v (let [x (get coll i)] F-BODY)]        # runs on EVERY element
#       (if more
#         (if (let [y v] PRED)
#           (begin (assign ans true) (assign more false))
#           nil)
#         nil))
#     (assign i (%add i 1)))

(def any-map-hir (fused "(any? (fn [y] (odd? y)) (map (fn [x] (+ x 1)) [1 2 3]))"))
(assert (= 0 (occurrences any-map-hir "any?#")))
(assert (= 0 (occurrences any-map-hir "map#")))
(assert (= 0 (occurrences any-map-hir "(and ")) "the loop condition is the bare range test")
(assert (= 0 (occurrences any-map-hir "(@array#")) "no intermediate array")
(assert (any? (fn [y] (odd? y)) (map (fn [x] (+ x 1)) [1 2 3])))
```

Stopping the whole walk instead would leave the prefix's per-element work unrun,
which the composition gate's argument does not cover: that argument is about
*reordering* two lambdas' calls, and it permits `SIG_ERROR` because each error
still surfaces — where an error the staged form raises on an element past the
decision would not be raised at all. So a prefix costs the fused form the early
exit's *walk*; what it keeps is the intermediate collection's dissolution, which
is the whole of the saving over the staged form anyway. A **lone** search keeps
the condition-read sentinel of the first shape above: nothing runs after its
stage, so ending the walk there omits nothing.

`find-index` carries one further obligation, and only where the prefix **renumbers**:
its answer is a position in the collection it walks, and a `filter`'s survivors
renumber, as do a `drop-while`'s once its leading run is gone. The loop then carries
the surviving element's own count — bumped once per element that reaches the search's
stage — and the deciding element records that in place of the base index. A `map`
prefix, and a `take-while`, preserve both the count and the order of the elements, so
there the base index is already the answer.

As for a fold or a count, a search does not fuse over a mutable `@array` base,
whose length each array arm re-reads per iteration.

A lone search never reorders and needs no purity gate, exactly as a lone `count`
needs none; over a prefix it carries the composition gate like every other
terminal. What it saves is what a lone count saves: each search's array arm walks
with a `letrec`-bound self-recursive closure ([src/stdlib.lisp](../../../src/stdlib.lisp)), so the un-fused
call mints that closure and its forward cell every time, plus the predicate
closure wherever the argument is a lambda literal. The fused loop mints none of
the three — plus, over a prefix, the intermediate collection — and, where it is
lone, it also stops reading the collection once the answer is known.
