# Dissolution: the scalar terminals

<!-- audited: 2026-10-06 -->

How a fused loop ends in a scalar: `fold`, `count`, and the four searches.
`reduce` is `fold` under a second name.

The [dissolution](../dissolution.md) document owns the pass and its gate, and
[the stages](stages.md) own the ops a terminal chains over.

```lisp
(def {:fused fused :occurrences occurrences}
  (import-file "hir.lisp"))
```

## Fold — the scalar terminal

`map` and `filter` each *collect* their per-element results into a fresh
immutable array, and that array is the pipeline's **terminal**. `fold`/`reduce`
replaces the terminal with a **scalar accumulator**. In `(fold f init xs)`, `f`
is called `(f acc element)`: the same left-fold that the `fold` of
[src/core.lisp](../../../src/core.lisp) runs. `reduce` is `(def reduce fold)`,
the identical op recognized by either name. The call dissolves to:

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

There is no `@array`, no `push` and no `freeze`. The accumulator is a reassigned
scalar seeded by `init` and updated one left-fold step per element, and the
result is its final value. `f`'s two parameters bind 1:1: the accumulator
parameter to the current `acc`, the element parameter to `(get coll i)`. Its
body is spliced inline exactly as a `map` transform is.

`init` is bound to an immutable `seed` **outermost**, before the base
collection. It therefore evaluates in the source order of `(fold f init coll)`,
init before coll, even though the loop needs `coll` and `len` first.

A fold is always **outermost**: its scalar result is not a collection, so no
`map` or `filter` chains over it. The pipeline is therefore unchanged between the
terminals, and only the accumulator setup and the per-element base case differ.

`(fold f init (map g xs))`, `(fold f init (filter p xs))` and any map/filter
prefix fuse to **one** loop, whose base case is the fold step instead of the
push. There is **no intermediate array** between the inner ops and the fold. This
is map-reduce, the canonical parallel-reduction shape and the reason to prove
this leg. `Build::element` threads the value through the map/filter stages, and
its base case is the terminal: a `push` (Collect), a fold `assign` (Fold), or a
tally `assign` (Count). The recursion is otherwise identical.

## Count — the terminal that is a guard plus a tally

`(count pred coll)` answers how many elements satisfy `pred`. It takes the same
`(function, collection)` shape a `filter` does and produces a **number**, so it
is a terminal exactly as `fold` is: nothing chains over it. Its fused form is one
already built, a `filter` **stage** whose base case counts instead of pushing:

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

The predicate is appended as the **last** stage of the pipeline. It is the
outermost op, so it runs after every inner transform and guard. The terminal is
a scalar accumulator seeded at 0, whose base case is `(assign n (%add n 1))`.
The count's own stage is a guard, so the value that reaches that base case is
always the local a `Filter` stage binds: the tally discards a name, never work.

Even a lone `(count p xs)` saves allocations, which a lone fold does not.
`count`'s array arm walks with a `letrec`-bound self-recursive closure
([src/stdlib.lisp](../../../src/stdlib.lisp)), so the un-fused call mints that
closure and its forward cell every time. It also mints the predicate closure
wherever `p` is a lambda literal. The fused loop mints none of the three. Over a
prefix, `(count p (map f xs))`, the intermediate array dissolves too, exactly as
it does under a fold.

`count`'s array arm errors on a string or bytes collection, where `map` and
`filter` accept one. The base gate proves the `array` keyword specifically, so
the fused form is reached only where the stdlib op would have taken its array
arm.

## Search — the terminal that stops early

`any?`, `all?`, `find` and `find-index` each answer a question about the
**first** element their predicate decides, and stop walking there. Each takes
the same `(function, collection)` shape a `filter` does and produces a scalar:
a boolean, an element, or an index. So each is a terminal exactly as `fold` and
`count` are.

The fused form is the count's shape plus an early exit:

- The predicate is the pipeline's guard stage.
- The accumulator is seeded with the answer for "no element decided it".
- Where the search is the chain's only op, the loop leaves through a **sentinel
  the condition reads**: a `more` flag that the deciding element clears.

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

`all?` is the one whose guard runs the other way round. A **failing** element
decides its answer, so the stage it appends continues the pipeline where the
predicate does *not* pass. That is a guard's other side. A `map` transforms the
threaded value, a `filter` **keeps** what its predicate admits, and an `all?`
**rejects** it: one `if` either way, which differs only in the branch that
carries the rest of the pipeline.

### The early exit stops the search's own work, not the pipeline's

A search fuses over a `map`/`filter` prefix as the other terminals do. The
prefix changes where the early exit applies. A staged `(any? p (map f xs))` runs
`f` over the **whole** input, and `p` over the elements up to the decision. The
fused loop must make exactly those calls. So the walk stays exhaustive, with the
bare range test as the loop condition, and the sentinel gates the **search's own
guard stage** instead.

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

Stopping the whole walk instead would leave the prefix's per-element work
unrun, and the composition gate's argument does not cover that. That argument
is about *reordering* two lambdas' calls. It permits `SIG_ERROR` because each
error still surfaces, but an error the staged form raises on an element past the
decision would not be raised at all.

So a prefix costs the fused form the early exit's *walk*. It keeps the
dissolution of the intermediate collection, which is the whole of the saving
over the staged form anyway. A **lone** search keeps the condition-read sentinel
of the first shape above: nothing runs after its stage, so ending the walk there
omits nothing.

`find-index` carries one further obligation, and only where the prefix
**renumbers**. Its answer is a position in the collection it walks. A
`filter`'s survivors renumber, and so do a `drop-while`'s once its leading run
is gone. The loop then carries the surviving element's own count, bumped once
per element that reaches the search's stage, and the deciding element records
that count in place of the base index. A `map` prefix and a `take-while`
preserve both the count and the order of the elements, so there the base index
is already the answer.

As for a fold or a count, a search does not fuse over a mutable `@array` base,
whose length each array arm re-reads per iteration.

A lone search never reorders and needs no purity gate, exactly as a lone `count`
needs none. Over a prefix it carries the composition gate like every other
terminal. It saves what a lone count saves. Each search's array arm walks with a
`letrec`-bound self-recursive closure ([src/stdlib.lisp](../../../src/stdlib.lisp)),
so the un-fused call mints that closure and its forward cell every time, plus
the predicate closure wherever the argument is a lambda literal.

The fused loop mints none of the three, and over a prefix it also removes the
intermediate collection. Where the search is lone, the loop also stops reading
the collection once the answer is known.
