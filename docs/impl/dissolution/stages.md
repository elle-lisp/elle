# Dissolution: the pipeline stages

<!-- audited: 2026-09-28 -->

How each op other than `map` threads its elements through a fused loop, stage by
stage.

The [dissolution](../dissolution.md) document owns the pass, its gate and the
`map` stage every other stage is read against.

```lisp
(def {:fused fused :occurrences occurrences}
  (import-file "docs/impl/dissolution/hir.lisp"))
```

## Filter — the conditional push

`filter` shares `map`'s scaffold — the same `(get`/`push`/`freeze)` index-walk
over the base's array arm — and differs only in the per-element loop body. Where
`map` pushes `f`'s *result*, `filter` pushes the *element itself*, gated by the
predicate (mirroring `filter`'s own array arm, [src/stdlib.lisp](../../../src/stdlib.lisp)):

```lisp
# (filter (fn [x] PRED) [ … ]) replaces map's (push acc …) with:
#
#   (let [item (get coll i)]
#     (if (let [x item] PRED) (push acc item) nil))

(def filter-hir (fused "(filter (fn [x] (odd? x)) [1 2 3])"))
(assert (= 0 (occurrences filter-hir "filter#")))
(assert (= 1 (occurrences filter-hir "(loop ")))
(assert (= (filter (fn [x] (odd? x)) [1 2 3]) [1 3]))
```

A `filter`-of-`filter` chain nests the guards innermost-first — the element is
bound once and pushed only when every predicate passes:

```lisp
# (let [item (get coll i)]
#   (if (let [p item] P-BODY)
#     (if (let [q item] Q-BODY) (push acc item) nil)
#     nil))

(def filter-filter-hir
  (fused "(filter (fn [y] (= y 3)) (filter (fn [x] (odd? x)) [1 2 3]))"))
(assert (= 0 (occurrences filter-filter-hir "filter#")))
(assert (= 1 (occurrences filter-filter-hir "(@array#")))
(assert (= (filter (fn [y] (= y 3)) (filter (fn [x] (odd? x)) [1 2 3])) [3]))
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

```lisp
# (let [item (get coll i)]
#   (if (let [p item] P-BODY)              # filter p — does it survive?
#     (push acc (let [x item] F-BODY))     # map f — push the transform of the survivor
#     nil))

(def map-filter-hir
  (fused "(map (fn [x] (* x 10)) (filter (fn [x] (odd? x)) [1 2 3]))"))
(assert (= 0 (occurrences map-filter-hir "map#")))
(assert (= 0 (occurrences map-filter-hir "filter#")))
(assert (= 1 (occurrences map-filter-hir "(@array#")))
(assert (= (map (fn [x] (* x 10)) (filter (fn [x] (odd? x)) [1 2 3])) [10 30]))
```

and for `(filter q (map g xs))` the map stage transforms first and the guard
tests the transformed value:

```lisp
# (let [v (let [x (get coll i)] G-BODY)]   # map g — the transformed value
#   (if (let [q v] Q-BODY) (push acc v) nil))

(def filter-map-hir
  (fused "(filter (fn [v] (odd? v)) (map (fn [x] (+ x 1)) [1 2 3]))"))
(assert (= 0 (occurrences filter-map-hir "filter#")))
(assert (= 0 (occurrences filter-map-hir "map#")))
(assert (= 1 (occurrences filter-map-hir "(@array#")))
(assert (= (filter (fn [v] (odd? v)) (map (fn [x] (+ x 1)) [1 2 3])) [3]))
```

The intermediate array the inner op would have allocated — the survivors between
`filter` and `map`, or the mapped values between `map` and `filter` — never
exists, exactly as it does not for a homogeneous chain. `map`-only and
`filter`-only chains are the two ends of this one pipeline (all-transform stages,
or all-guard stages); the builder (`build_loop`/`Build::element`) is the same for
all three.

## Take-while — the stage that ends the walk

`take-while` keeps the leading run of elements its predicate admits and stops at
the first one it rejects. It wears a `filter`'s `(function, collection)` shape and
produces a **collection**, so — unlike a search — it is a pipeline **stage**: ops
chain over its result. Its fused form is a `filter`'s guard with the search's early
exit hung off the other side, the rejecting element ending the run instead of
merely being skipped:

```lisp
# (take-while (fn [x] PRED) [ … ]) becomes:
#
#   (let [coll [ … ]]
#     (let [len (length coll)]
#       (if (< 0 len)
#         (let [acc (@array)]
#           (define i 0)
#           (define more true)
#           (while (and (< i len) more)
#             (let [item (get coll i)]
#               (if (let [x item] PRED)
#                 (push acc item)
#                 (assign more false)))
#             (assign i (%add i 1)))
#           acc)
#         ())))

(def take-while-hir (fused "(take-while (fn [x] (odd? x)) [1 3 4 5])"))
(assert (= 0 (occurrences take-while-hir "take-while#")))
(assert (= 1 (occurrences take-while-hir "(and ")))
(assert (= 0 (occurrences take-while-hir "freeze#")) "the accumulator stays mutable")
(assert (= (take-while (fn [x] (odd? x)) [1 3 4 5]) [1 3]))
(assert (= :@array (type-of (take-while (fn [x] (odd? x)) [1 3 4 5]))))
(assert (= () (take-while (fn [x] (odd? x)) [])) "an empty base answers ()")
```

### Which early exit may end the walk

A search states the rule for a chain of one; a `take-while` sits anywhere in the
pipeline, so it states the general one: **the chain's innermost op may end the
walk; every other early exit gates its own stage.** Nothing runs before the
innermost op, so ending the walk there omits no per-element work the staged form
would have done. Everything *after* it in the pipeline sees only the elements that
op passed on, so those stages lose nothing either. That is the lone search's
argument, read one op at a time rather than for the whole chain.

So a `take-while` with a **prefix** — a stage inner to it — keeps
the exhaustive walk (the loop condition is the bare range test) and rides its own
sentinel instead, as a prefixed search's guard does:

```lisp
# (take-while (fn [y] PRED) (map (fn [x] F-BODY) [ … ])) walks with:
#
#   (while (< i len)
#     (let [v (let [x (get coll i)] F-BODY)]      # runs on EVERY element
#       (if more
#         (if (let [y v] PRED) (push acc v) (assign more false))
#         nil))
#     (assign i (%add i 1)))

(def take-while-map-hir
  (fused "(take-while (fn [y] (odd? y)) (map (fn [x] (+ x 1)) [0 2 3 4]))"))
(assert (= 0 (occurrences take-while-map-hir "take-while#")))
(assert (= 0 (occurrences take-while-map-hir "map#")))
(assert (= 0 (occurrences take-while-map-hir "(and ")))
(assert (= (take-while (fn [y] (odd? y)) (map (fn [x] (+ x 1)) [0 2 3 4])) [1 3]))
```

Where a lone search and a walk-ending `take-while` both want the loop condition,
they cannot collide: a `take-while` is a stage, so a search sharing the chain with
one has a prefix by construction and takes its gate.

As for a fold, a count or a search, a `take-while` does not fuse over a mutable
`@array` base, whose length the array arm re-reads per iteration.

A lone `take-while` never reorders and needs no purity gate; in a chain it carries
the composition gate like every other op. What it saves is what a lone count
saves: its array arm walks with a `letrec`-bound self-recursive closure
([src/stdlib.lisp](../../../src/stdlib.lisp)), so the un-fused call mints that closure and its forward cell
every time, plus the predicate closure wherever the argument is a lambda literal.

## Drop-while — the stage that starts late

`drop-while` is `take-while`'s complement: it skips the leading run its predicate
admits and passes on every element from the first one the predicate rejects. It
wears the same `(function, collection)` shape and produces a **collection**, so it
is a pipeline **stage** too. Its fused form is a guard with the sides swapped and
the sentinel latched the other way round — a `dropping` flag the rejecting element
clears, after which every element passes:

```lisp
# (drop-while (fn [x] PRED) [ … ]) becomes:
#
#   (let [coll [ … ]]
#     (let [len (length coll)]
#       (if (< 0 len)
#         (let [acc (@array)]
#           (define i 0)
#           (define dropping true)
#           (while (< i len)
#             (let [item (get coll i)]
#               (begin
#                 (if dropping
#                   (if (let [x item] PRED) nil (assign dropping false))
#                   nil)
#                 (if dropping nil (push acc item))))
#             (assign i (%add i 1)))
#           acc)
#         ())))

(def drop-while-hir (fused "(drop-while (fn [x] (odd? x)) [1 3 4 5])"))
(assert (= 0 (occurrences drop-while-hir "drop-while#")))
(assert (= 0 (occurrences drop-while-hir "(and ")) "no early exit")
(assert (= (drop-while (fn [x] (odd? x)) [1 3 4 5]) [4 5]))
```

The predicate runs on exactly the elements the stdlib op gives it — the leading run,
plus the element that ends it — because the first `if` reads the flag before testing.
The walk itself stays exhaustive on every chain: a `drop-while` carries no early exit
at all. Its decision *opens* the rest of the pipeline rather than closing the walk,
so it never contends for the loop condition, and the innermost-op rule
(§ "Which early exit may end the walk") has nothing to say about it.

The two `if`s are one decision read twice, not two decisions. Writing it as a single
`if` whose rejecting side both clears the flag and continues the pipeline would put
the rest of the pipeline in two places — the deciding element's branch, and every
later element's — duplicating every stage spliced after this one.

A `drop-while` **renumbers**. It removes a leading run, so an element's position in
its output is its base index less that run's length, and a `find-index` over one
reads the survivor count the pipeline carries, exactly as it does over a `filter`
([terminals](terminals.md)). A `take-while`
needs no such count: it keeps a leading run, so every survivor keeps its position.

As for a fold, a count, a search or a `take-while`, a `drop-while` does not fuse over
a mutable `@array` base, whose length its array arm re-reads per iteration.

A lone `drop-while` never reorders and needs no purity gate; in a chain it carries
the composition gate like every other op. It saves more than a lone `take-while`
does: its array arm walks with **two** `letrec`-bound self-recursive closures
([src/stdlib.lisp](../../../src/stdlib.lisp)) — one to find the start, one to copy from it — so the un-fused
call mints two closures and two forward cells every time, plus the predicate closure
wherever the argument is a lambda literal.

## Map-indexed — the stage that carries the position

`map-indexed` transforms each element as a `map` does, but hands its function the
element's **position** beside it — `(f i elem)`, index first, the order
[src/stdlib.lisp](../../../src/stdlib.lisp)'s array arm calls it in. It produces a collection, so it is a
pipeline **stage**. Its fused form is a `map`'s with one extra binding: the loop's
own induction variable, bound to the function's first parameter.

```lisp
# (map-indexed (fn [i x] BODY) [ … ]) walks with:
#
#   (while (< i len)
#     (push acc (let [ip i] (let [xp (get coll i)] BODY)))
#     (assign i (%add i 1)))

(def map-indexed-hir (fused "(map-indexed (fn [i x] (+ i x)) [10 20 30])"))
(assert (= 0 (occurrences map-indexed-hir "map-indexed#")))
(assert (= (map-indexed (fn [i x] (+ i x)) [10 20 30]) [10 21 32]))
```

The position the function reads is an index into `map-indexed`'s **own** input, and
the loop's induction variable indexes the **base**. The two agree exactly while
every stage inner to this one preserves the walk's numbering, and one always does —
by construction, not by a further gate. `map-indexed`'s array arm is one of the
untyped ones (§ "The two facts an untyped array arm decides"), so the emptiness rule
already refuses every inner stage that is not length-preserving, and the ops that
renumber are exactly the ops that shorten. So the induction variable is the answer
and no survivor count is owed, where a `find-index` past a `filter` or a
`drop-while` does owe one.

`map-indexed` carries no early exit, so it never contends for the loop condition,
and it renumbers nothing it passes on. As for every op but a single `map`, `filter` or `mapcat`,
it does not fuse over a mutable `@array` base, whose array arm re-reads
`(length coll)` per iteration.

A lone `map-indexed` never reorders and needs no purity gate; in a chain it carries
the composition gate like every other op. What it saves is what a lone `take-while`
saves: its array arm walks with a `letrec`-bound self-recursive closure
([src/stdlib.lisp](../../../src/stdlib.lisp)), so the un-fused call mints that closure and its forward cell
every time, plus the function closure wherever the argument is a lambda literal.

## Mapcat — the stage that fans out

`mapcat` applies its function to each element and **splices** the collection that
function returns into one flat result — `(each x in coll (each y in (f x) (push
result y)))`, its array arm's whole body ([src/stdlib.lisp](../../../src/stdlib.lisp)). It produces a
collection, so it is a pipeline **stage**. Every other stage threads exactly one
value on per element; this one threads a whole run of them, so its fused form is
the first to put a **second walk** inside the element statement:

```lisp
# (mapcat (fn [x] BODY) [ … ]) becomes:
#
#   (let [coll [ … ]]
#     (let [len (length coll)]
#       (if (< 0 len)
#         (let [acc (@array)]
#           (define j 0)
#           (define i 0)
#           (while (< i len)
#             (let [inner (let [x (get coll i)] BODY)]
#               (let [ilen (length inner)]
#                 (begin
#                   (assign j 0)
#                   (while (< j ilen)
#                     (push acc (get inner j))
#                     (assign j (%add j 1))))))
#             (assign i (%add i 1)))
#           acc)
#         ())))

(def mapcat-hir (fused "(mapcat (fn [x] [x x]) [1 2])"))
(assert (= 0 (occurrences mapcat-hir "mapcat#")))
(assert (= 2 (occurrences mapcat-hir "(loop ")) "a second walk inside the first")
(assert (= (mapcat (fn [x] [x x]) [1 2]) [1 1 2 2]))
```

The rest of the pipeline is spliced **inside** the inner `while`, so every stage
outer to a `mapcat` runs once per spliced element rather than once per base element
— which is what a stage outer to the stdlib op sees, the flat collection being all
it is given. The inner index is one binding the loop scaffold owns, `define`d beside
the walk's own and reset per base element, so no `define` sits inside a loop body.

### The function's result must be a proven array

`each y in (f x)` walks whatever `f` returns — a list with `first`/`rest`, an
indexed collection with `get`. The fused inner walk is the indexed one, so
`mapcat` fuses **only where `f`'s body proves it returns an array**, read by the
same `classify_base` proof the base collection is read by. The reason is cost, not
value: over a list, `(get inner j)` is O(j) and the fused walk would be quadratic
where the stdlib op's is linear — a bounded scratch saving traded for an unbounded
time cost, which no rewrite may do. A function whose result is unproven declines and
the `mapcat` stays a plain call.

The proof reads the bindings of the unit that holds the body. A call-site lambda
literal is read in this unit. A named function is read once, when its fragment
closes in the unit that defines it, and the fragment records the answer
(`FnFragment::returns_array`), so a same-unit and a cross-unit named function
qualify alike ([inline](inline.md)).

Like `each`, the fused inner walk captures `(length inner)` **once** per base
element and reads `inner` live, so a function returning a collection something else
mutates diverges from the stdlib op nowhere.

### What a mapcat decides for the chain around it

- **Its array arm is untyped** — the accumulator is returned unfrozen and an empty
  base answers `()` (§ "The two facts an untyped array arm decides") — so every
  stage inner to a `mapcat` must be length-preserving.
- **It is not length-preserving itself**, so a `mapcat` inner to any untyped array
  arm — another `mapcat`, a `map-indexed`, a `take-while`, a `drop-while` — declines
  the chain whole. Each of those reads its own emptiness off the base's `len`, and a
  `mapcat` can hand an empty collection on from a non-empty base.
- **It renumbers**, one base element becoming a run of any length, so a `find-index`
  outer to a `mapcat` answers with the survivor count the pipeline carries, exactly
  as it does past a `filter` ([terminals](terminals.md)).
- **It carries no early exit**, so it never contends for the loop condition, and the
  innermost-op rule (§ "Which early exit may end the walk") has nothing to say about
  it. Nothing that owns a sentinel can land inside the inner walk either: a
  `take-while` or a `drop-while` outer to a `mapcat` is exactly what the emptiness
  rule refuses.

A lone `mapcat` never reorders and needs no purity gate; in a chain it carries the
composition gate like every other op. What it saves is the per-element intermediate
its array arm has no way to avoid — plus, over a prefix or under a terminal, the
whole flat collection between the ops.

## The two facts an untyped array arm decides

The array arms of `map-indexed`, `take-while`, `drop-while` and `mapcat` are not
type-preserving the way `map`'s and `filter`'s are, and fusion reproduces each
exactly — a rewrite may not change a value.

- **The result is unfrozen.** Each array arm returns its `@array` accumulator with
  no `(if (mutable? coll) acc (freeze acc))`, so any of the four over an immutable
  array yields a **mutable** one. `map` and `filter` are type-preserving, so a
  Collect chain holding any of the four anywhere is unfrozen throughout.
- **An empty input answers `()`.** The `(or (pair? coll) (empty? coll))` clause
  precedes the array arm in all four ops, so an empty array takes the *list* arm and
  the op returns the empty list. The fused Collect form answers `(< 0 len)`'s false
  side with `()` for the same reason.

The second fact is why every stage inner to one of these four ops must be
**length-preserving** — a `map` or a `map-indexed`. `len` decides the emptiness of
the **base**, and a length-preserving stage carries it through; a `filter`, a
`take-while`, a `drop-while` or a `mapcat` can hand an empty collection on from a
non-empty base,
where the staged form would answer `()` and the fused loop its accumulator. Such a
chain declines whole, and the pre-order recursion still fuses the inner run. A scalar
terminal cannot observe the difference — an exhausted walk answers with its seed
either way — but the rule is stated over the pipeline rather than over the terminal,
so one reading covers every chain.

Length preservation is the stronger fact, and its other half is what a `map-indexed`
inner to such an op rests on: a stage that keeps every element also keeps every
element's position (§ "Map-indexed — the stage that carries the position").
