# Dissolution — HOF loop fusion

Dissolution is the third leg of the region system (see `memory.md` § "The three
legs"): a closure is a first-class *value* but the *unit of nothing* at runtime,
and — guided by the escape/ownership facts legs 1 and 2 infer — the compiler
realizes a higher-order call as its most efficient form. `(map f xs)` over an
owned, non-escaping `xs` exposes no observable closure and no observable
intermediate collection, so the compiler is free to realize it as a plain loop
with `f`'s body spliced in, a JIT'd group, CPU SIMD, or a device dispatch. This
document specifies the first realization: **HOF-chain loop fusion** on the VM
substrate (`src/hir/typeinfer/fuse.rs`), covering the four array-producing
higher-order ops `map`, `filter`, `take-while` and `drop-while` and the
scalar-producing
terminals: the left-fold `fold`/`reduce`, the predicate tally `count`, and the four
short-circuiting searches `any?`/`all?`/`find`/`find-index`.

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

```
(count (fn [x] PRED) [ … ])
⇒
(let [seed 0]
  (let [coll [ … ]]
    (let [len (length coll)]
      (define n seed)
      (define i 0)
      (while (< i len)
        (let [item (get coll i)]
          (if (let [x item] PRED) (assign n (+ n 1)) nil))
        (assign i (+ i 1)))
      n)))
```

The predicate is appended as the **last** stage of the pipeline (it is the
outermost op, so it runs after every inner transform/guard), and the terminal is a
scalar accumulator seeded at 0 whose base case is `(assign n (+ n 1))`. Because
the count's own stage is a guard, the value reaching that base case is always the
local a `Filter` stage binds — the tally discards a name, never work.

Even a lone `(count p xs)` saves allocations, which a lone fold does not:
`count`'s array arm walks with a `letrec`-bound self-recursive closure
(`src/stdlib.lisp`), so the un-fused call mints that closure and its forward cell
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

```
(any? (fn [x] PRED) [ … ])
⇒
(let [seed false]
  (let [coll [ … ]]
    (let [len (length coll)]
      (define ans seed)
      (define i 0)
      (define more true)
      (while (and (< i len) more)
        (let [item (get coll i)]
          (if (let [x item] PRED)
            (begin (assign ans true) (assign more false))
            nil))
        (assign i (+ i 1)))
      ans)))
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

```
(any? (fn [y] PRED) (map (fn [x] F-BODY) [ … ]))
⇒
  (while (< i len)
    (let [v (let [x (get coll i)] F-BODY)]        ; runs on EVERY element
      (if more
        (if (let [y v] PRED)
          (begin (assign ans true) (assign more false))
          nil)
        nil))
    (assign i (+ i 1)))
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
with a `letrec`-bound self-recursive closure (`src/stdlib.lisp`), so the un-fused
call mints that closure and its forward cell every time, plus the predicate
closure wherever the argument is a lambda literal. The fused loop mints none of
the three — plus, over a prefix, the intermediate collection — and, where it is
lone, it also stops reading the collection once the answer is known.

## Take-while — the stage that ends the walk

`take-while` keeps the leading run of elements its predicate admits and stops at
the first one it rejects. It wears a `filter`'s `(function, collection)` shape and
produces a **collection**, so — unlike a search — it is a pipeline **stage**: ops
chain over its result. Its fused form is a `filter`'s guard with the search's early
exit hung off the other side, the rejecting element ending the run instead of
merely being skipped:

```
(take-while (fn [x] PRED) [ … ])
⇒
(let [coll [ … ]]
  (let [len (length coll)]
    (if (< 0 len)
      (let [acc (@array)]
        (define i 0)
        (define more true)
        (while (and (< i len) more)
          (let [item (get coll i)]
            (if (let [x item] PRED)
              (push acc item)
              (assign more false)))
          (assign i (+ i 1)))
        acc)
      ())))
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

```
(take-while (fn [y] PRED) (map (fn [x] F-BODY) [ … ]))
⇒
  (while (< i len)
    (let [v (let [x (get coll i)] F-BODY)]      ; runs on EVERY element
      (if more
        (if (let [y v] PRED) (push acc v) (assign more false))
        nil))
    (assign i (+ i 1)))
```

Where a lone search and a walk-ending `take-while` both want the loop condition,
they cannot collide: a `take-while` is a stage, so a search sharing the chain with
one has a prefix by construction and takes its gate.

As for a fold, a count or a search, a `take-while` does not fuse over a mutable
`@array` base, whose length the array arm re-reads per iteration.

A lone `take-while` never reorders and needs no purity gate; in a chain it carries
the composition gate like every other op. What it saves is what a lone count
saves: its array arm walks with a `letrec`-bound self-recursive closure
(`src/stdlib.lisp`), so the un-fused call mints that closure and its forward cell
every time, plus the predicate closure wherever the argument is a lambda literal.

## Drop-while — the stage that starts late

`drop-while` is `take-while`'s complement: it skips the leading run its predicate
admits and passes on every element from the first one the predicate rejects. It
wears the same `(function, collection)` shape and produces a **collection**, so it
is a pipeline **stage** too. Its fused form is a guard with the sides swapped and
the sentinel latched the other way round — a `dropping` flag the rejecting element
clears, after which every element passes:

```
(drop-while (fn [x] PRED) [ … ])
⇒
(let [coll [ … ]]
  (let [len (length coll)]
    (if (< 0 len)
      (let [acc (@array)]
        (define i 0)
        (define dropping true)
        (while (< i len)
          (let [item (get coll i)]
            (begin
              (if dropping
                (if (let [x item] PRED) nil (assign dropping false))
                nil)
              (if dropping nil (push acc item))))
          (assign i (+ i 1)))
        acc)
      ())))
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
(§ "The early exit stops the search's own work, not the pipeline's"). A `take-while`
needs no such count: it keeps a leading run, so every survivor keeps its position.

As for a fold, a count, a search or a `take-while`, a `drop-while` does not fuse over
a mutable `@array` base, whose length its array arm re-reads per iteration.

A lone `drop-while` never reorders and needs no purity gate; in a chain it carries
the composition gate like every other op. It saves more than a lone `take-while`
does: its array arm walks with **two** `letrec`-bound self-recursive closures
(`src/stdlib.lisp`) — one to find the start, one to copy from it — so the un-fused
call mints two closures and two forward cells every time, plus the predicate closure
wherever the argument is a lambda literal.

## The two facts an untyped array arm decides

`take-while`'s and `drop-while`'s array arms are not type-preserving the way `map`'s
and `filter`'s are, and fusion reproduces each exactly — a rewrite may not change a
value.

- **The result is unfrozen.** Each array arm returns its `@array` accumulator with
  no `(if (mutable? coll) acc (freeze acc))`, so either op over an immutable array
  yields a **mutable** one. `map` and `filter` are type-preserving, so a Collect
  chain holding either op anywhere is unfrozen throughout.
- **An empty input answers `()`.** The `(or (pair? coll) (empty? coll))` clause
  precedes the array arm in both ops, so an empty array takes the *list* arm and the
  op returns the empty list. The fused Collect form answers `(< 0 len)`'s false side
  with `()` for the same reason.

The second fact is why every stage inner to one of these two ops must be a `map`.
`len` decides the emptiness of the **base**, and a `map` preserves it; a `filter`, a
`take-while` or a `drop-while` can hand an empty collection on from a non-empty base,
where the staged form would answer `()` and the fused loop its accumulator. Such a
chain declines whole, and the pre-order recursion still fuses the inner run. A scalar
terminal cannot observe the difference — an exhausted walk answers with its seed
either way — but the rule is stated over the pipeline rather than over the terminal,
so one reading covers every chain.

## When it is legal — the gate

Fusion preserves the program's value and, for a single op (`map` or `filter`),
its exact per-element evaluation order (the loop visits each element left to
right, applying `f`/`p` identically to the stdlib op). The gate:

- **The callee is a canonical stdlib HOF.** A pipeline op is `map`, `filter`,
  `take-while` or `drop-while`; the optional outermost terminal op is `fold`,
  `reduce`, `count`, or one of the
  four short-circuiting searches `any?`/`all?`/`find`/`find-index`. Recognized by
  the callee binding being `is_primitive` (every stdlib/core export is bound so by
  `bind_primitives`, and the canonical core-env override is marked `is_primitive`
  too, so `fold`/`reduce` reach the gate exactly as `map`/`filter` do; a user
  redefinition shadows with a non-primitive binding) and named accordingly. A user
  redefinition is never rewritten. A `count` or a search call has a `filter`'s
  two-argument shape, so the terminal is recognized before the pipeline walk starts
  and neither is ever read as a stage.
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
- **The function is non-capturing with the op's fixed arity** — one parameter for
  a `map`/`filter`/`take-while`/`drop-while`/`count`/search (the element), two for a
  `fold` (the accumulator
  and the element) — with no rest parameter, and a body free of nested lambdas and of
  call-position `%`-intrinsics unless the function declares `(numeric!)` (see
  "Raw `%`-intrinsic bodies" below). It is one of two forms:
  - a **lambda literal** written directly as the call's argument. It is consumed
    by the rewrite (moved out of the call), so no other use can observe the
    change, and its parameter is retyped to a loop-local in place.
  - a **`Var` referencing a same-compile-unit function** whose initializer is such
    a lambda (a top-level `(defn f …)` or a `let`/`def`-bound `(fn …)`) — inlined
    by cloning, see "Named same-unit functions" below.
  No captures means the body references only its parameters and globals, so
  splicing it at the call site is always in scope; the fixed parameter count
  means the loop's element (and, for a fold, the accumulator) bind 1:1.

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

The **terminal counts as an op** in the chain length. A lone `fold` (`(fold f init
xs)`, length 1) threads its accumulator strictly in element order — exactly the
stdlib fold — so it never reorders and needs no gate, even with a
sequencing-effectful body; a lone `count` visits each element left to right and
applies its predicate identically to the stdlib op, so it reads the same way. A
lone search reads the same way again, up to the element that decides it. A
terminal *with* an inner `map`/`filter` prefix is length ≥ 2 and carries the
reorder requirement over every lambda (the terminal's and the prefix's): the
terminal's per-element work interleaves with the prefix transforms
(`g x0; f …; g x1; f …`) rather than running the whole prefix first, the same
reorder a mixed chain makes. A non-reorder-safe stage declines the whole
composition, and the pass falls back to fusing the inner reorder-safe run (the
prefix), leaving the terminal a plain call over the fused loop.

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
only** — no terminal, no composition. The reason is that the fused loop walks the
base *live* (it reads `(get coll i)` each iteration against a `len` captured
once), and this matches the stdlib op **exactly** for a single `map`/`filter`
(whose own array arm captures `len` once and reads `coll` live) — so the value is
preserved even if the lambda mutates the base through a global alias. The three
excluded shapes break that match:

- **`fold`** first snapshots its input (`(->array coll)` copies a mutable array)
  and walks the copy; a fused fold would walk the live base, so a mutating
  combinator would observe a divergence. A `fold` over a mutable base stays a
  plain call.
- **`count`** re-reads `(length coll)` on every iteration where the fused loop
  captures `len` once, so a predicate that pushes to or pops from the base would
  observe a divergence. A `count` over a mutable base stays a plain call. Each
  **search** array arm re-reads it the same way, and so do **`take-while`**'s and
  **`drop-while`**'s; all decline for the same reason.
- **A composition** (`(map g (filter p @xs))`, …) runs each stdlib op to
  completion over a *fresh* array before the next begins, so a later op's lambda
  mutating the original base can no longer affect the result; the single fused
  loop interleaves the ops against the live base, where such a mutation *would*
  affect later reads. A composition over a mutable base declines, and the
  pre-order recursion still fuses its innermost single `map`/`filter` (sound in
  isolation), leaving the outer ops as plain calls over that fused loop.

For an **immutable** base none of the hazards exists — the base cannot be mutated
— so the terminals and compositions fuse over it exactly as before.

## Named same-unit functions

A HOF's function argument need not be a call-site lambda literal. When it is a
`Var` referencing a binding **in the same compile unit** whose initializer is a
qualifying non-capturing lambda — a top-level `(defn f …)` (which desugars to
`(def f (fn …))`) or any `let`/`def`-bound `(fn …)` — the function's body is
inlined into the fused loop, exactly as a literal's would be. The map from a
binding to its lambda template is collected by the same walk `prune.rs` uses for
init keywords (over `Let`/`Letrec`/`Define` bindings), restricted to immutable,
singly-bound, non-capturing lambdas with the op's arity.

The one structural difference from a literal is **who owns the body**. A literal
is consumed — moved out of the call — so its parameter and node ids belong
uniquely to the splice. A **named** function *persists*: it stays bound and may be
called elsewhere as a first-class value, so its body cannot be moved. It is
**cloned with fresh bindings and fresh HirIds** per call site (an alpha-rename):
every parameter is re-minted and its references rewritten, and every node is
rebuilt through `Hir::new` (ids come from a global counter, and a plain `.clone()`
would duplicate them, colliding in the region walk's per-id side tables). Globals
the body references stay **shared** — a global is already referenced from many
sites, so sharing its binding is the norm, not a duplication hazard; only the
lambda's own parameters are freshened.

The clone is a **whitelist** over pure-expression forms — literals, `Var`, `Call`,
`if`, `cond`, `begin`, `and`/`or` — plus `let`. A `let` body is cloned with its
**own** bindings freshened exactly as the parameters are: each `let`-bound binding
is re-minted (faithful to the source's mutability) and added to the rename map
before the body and any later sibling values are cloned, so a sequential `let`'s
later value that references an earlier binding rewrites to the fresh one. `letrec`
is **not** admitted — its value may reference its own binding (a forward/self
reference the sequential rename order cannot satisfy), and the recursive-closure
cell it builds is the shape this fusion exists to avoid. A body that introduces a
binding through any **other** form (a `loop`, a `match` pattern, or a nested
lambda) or uses any unrecognized form **declines**: the clone returns nothing and
the HOF stays a plain call, so the definition's own bindings are never duplicated
(correct-by-construction — an unhandled form is left un-optimized, never
miscompiled).

## Cross-unit named functions

A named function need not live in the unit that calls it. A **stdlib** `defn` —
`inc`, `dec`, and any non-capturing lambda with a whitelisted body — is defined in
the `<stdlib>` compile unit, so a later user unit that writes `(map inc xs)` has no
body for `inc` in its own tree: `collect_inline_fns` finds nothing, and the
same-unit path declines. The cross-unit path carries the body across the
compile-unit boundary, mirroring the dispatch-wrapper registry
(`monomorphize.rs`): a per-instance registry keyed by function **name**
(`SymbolId`, stable across arenas where a `Binding` is not) is populated as each
unit compiles — the `<stdlib>` compile records `inc` — and consulted by every later
unit, gated on the callee being `is_primitive` (a `bind_primitives` stdlib export;
a user redefinition shadows it with a non-primitive binding and is left alone).

The registry entry carries the template body plus its **free globals**, each recorded
by `SymbolId`. A `Binding` is a per-arena index, meaningless in the consuming unit,
so every global the body references — the arithmetic op in `(+ x 1)`, say — is
re-resolved by name against the consuming unit's own primitive bindings at the call
site. If any free global fails to resolve there, the inline **declines** (the HOF
stays a plain call) — correct-by-construction, never a mis-resolved reference.

The free-global gate is what admits a stdlib body the *same-unit* gate would reject.
When the stdlib compiles, its own exports are **not yet** `is_primitive` — `+` is a
file-scope (`is_file_scope`) letrec sibling, so `inc`'s lambda *captures* it, and the
non-capturing same-unit gate declines. The cross-unit collector instead admits a
lambda whose every free variable is a genuine **global** — an `is_file_scope`
module name or an `is_primitive` binding — and records those names for re-resolution;
a free variable that is a plain enclosing local (a real capture of a runtime value)
declines, exactly as the same-unit gate intends. So a stdlib `defn` referencing only
other globals inlines, while a capturing local function never does.

The clone is otherwise identical to the same-unit one (`clone_fresh`): the
parameters and any `let`-bound bindings are freshened per call site, and every node
is rebuilt with a fresh `HirId`. The only addition is that the free globals are
seeded into the rename map up front — each mapped to the consuming unit's binding for
that name — so the shared `clone_fresh` rewrites them with no further machinery. The
same whitelist (pure-expression forms plus `let`), arity, unmutated-parameter, and
composition-reorder gates apply.

Everything else in the gate is identical — non-capturing, fixed arity, no rest
parameter, unmutated parameters, and the composition reorder requirement (read
from the template body's signal).

Signals on the synthesized helper calls (`get`/`push`/`freeze`/`<`/`+`/
`length`/`@array`) and on the synthesized `if`/`let` scaffolding are set to the
original call's signal — a sound upper bound (that call's signal already subsumes
every op in the stdlib op's body) — so the bottom-up signal re-propagation
(`hir/narrow.rs`) never under-reports the fused form's effects. The spliced lambda
bodies keep their own signals.

## Raw `%`-intrinsic bodies — the declaration travels with the binding

A `%`-intrinsic in call position must discharge its operand contract from the
inferred operand types (`docs/intrinsics.md` § "The contract: prove or reject"),
and for a numeric kernel written over a parameter — `(fn [x] (numeric!) (%mul x
x))` — the fact that discharges it is the `(numeric!)` declaration, which floors
every parameter of the function at Number. Fusion dissolves the function, so a
declaration scoped to the *lambda node* would vanish with it: the spliced
`(%mul x x)` now reads the loop's `(get coll i)` element, whose type is not
tracked, and the site would fail to prove. That would turn a compiling program
into a compile error — which no rewrite may do.

So the declaration is recorded where it is *about*. `(numeric!)` floors the
function's **parameter bindings** (`BindingInner::declared_numeric`), and
inference applies that floor wherever such a binding gets its type: as a
lambda parameter, as a parameter joined from its call sites, and as the
`let`-bound loop local the splice turns the parameter into. Fusion carries the
flag with the parameter — a call-site lambda literal is *moved*, so its
(already-flagged) parameter binding travels as-is; a cloned template mints fresh
parameters and copies the flag onto them. The fact that discharged the intrinsic
inside the function is the same fact that discharges it inside the loop, so
fusion leaves *whether the program compiles* unchanged.

The gate is therefore the declaration, not the op: a body containing a
call-position `%`-intrinsic is admitted **only** under `(numeric!)`. Without the
declaration there is nothing to carry, so the body declines and the HOF stays a
plain call — even where the intrinsic's operands are all literals and would prove
on their own. This is the shape the fusion most wants: a raw-intrinsic kernel over
a proven array is the numeric loop a SIMD/GPU realization tier consumes, with no
per-element closure, no dispatch, and no wrapper call between the elements and the
opcode.

Every other proof an intrinsic body may rest on is structural and survives the
splice unchanged — a literal operand, a diverging type guard inside the body, a
global's inferred type. Only the parameter floor is scoped to the function, and
only it needs carrying.

`(numeric!)` carries a second, independent assertion: that the function's body is
GPU-eligible, checked when the lambda lowers as a function
(`src/lir/lower/lambda/expr.rs`). A fused lambda never lowers as a function, so
that half of the declaration has nothing left to hold — true of every fused
`(numeric!)` lambda, intrinsic body or not. The type floor is the half fusion
carries.

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
  map/filter prefix into one loop. A `count` dissolves to a scalar tally — no
  `@array`/`freeze`, the predicate inline under one guard `if` — and composes with a
  map/filter prefix into one loop with a second guard. Each **search** dissolves to a
  scalar accumulator under one guard, with the loop condition reading the `more`
  sentinel the deciding element clears (`all?` carrying the guard whose *else* branch
  decides); over a prefix the condition is the bare range test and the sentinel gates
  the search's own stage instead, with a `filter` prefix under a `find-index` bumping
  the survivor count the answer reads. A **`take-while`** dissolves to a guarded
  push under the same sentinel — the loop condition reading it where the
  `take-while` is the chain's innermost op, a gate stage where it has a prefix —
  with the accumulator returned unfrozen and an empty base answering `()`, and it
  composes both ways (a `map` over it, and it over a `map`). A **`drop-while`**
  dissolves to the complementary shape — a `dropping` flag the rejecting element
  clears, gating the pipeline rather than the walk, so the loop condition stays the
  bare range test however the chain is arranged — with the same two array-arm facts,
  and a `find-index` over one bumping the survivor count the leading drop renumbers.
  A mutable-`@array`-base
  `map`/`filter` fuses
  with the accumulator returned **unfrozen** (no `freeze` call). A
  `(numeric!)`-declared raw-intrinsic kernel fuses — as a `map` transform, as a
  `filter` guard, as a `fold` combinator, as a composition, as a named same-unit
  function, and through the div family (whose nonzero-divisor fact is a literal,
  so it survives the splice) — with the spliced `%`-op inline in the loop. Decline pins guard the gate (user-shadowed
  callee, capturing lambda, unproven collection, an intrinsic body with **no**
  `(numeric!)` declaration, a non-reorder-safe composition — which declines and
  fuses its inner run only — a `fold`, a `count`, a `take-while`, a `drop-while`, or
  a composition
  over a
  mutable base, which declines to the innermost single op, and a `take-while` or
  `drop-while` whose
  inner stage is a `filter`, whose emptiness `len` cannot decide). Named-function pins cover a `map`/`fold`
  whose argument is a `Var` naming a same-unit `defn` (the body inlines, the
  definition persists) and the declines (a `let`-body function, a capturing local
  function, a non-lambda `Var`). A **cross-unit** pin fuses `(map dec …)` where
  `dec` is a stdlib `defn` — its body carried across the compile-unit boundary and
  spliced exactly once (the definition stays in the stdlib unit) — beside a decline
  for a stdlib fn whose body is not clone-whitelisted (`distinct`, a `letrec` body).
- **Realization (execution).** `tests/elle/dissolution-map-alloc.lisp` (the filter
  cases in `dissolution-filter-fuse.lisp`, and the mixed cases in
  `dissolution-mixed-fuse.lisp`) prove the consequence the mission names — *fewer
  allocations*. It measures
  `arena/total-allocs` (a **cumulative, monotonic** count of objects ever minted;
  `src/value/fiberheap/`) around a fused chain versus an un-fused reference
  computing the same value, and asserts the fused form mints strictly fewer, with
  the saving scaling per composition layer (one intermediate array each — for a
  mixed chain, the survivor/mapped array between the two ops; for a fold chain,
  the array the map/filter prefix would have handed the fold). A **cross-unit**
  case fuses `(map dec (map dec xs))` — a stdlib `defn` inlined across the
  compile-unit boundary — against a capturing-lambda reference, proving the
  intermediate array vanishes across that boundary too. A lone `fold`
  produces a scalar, so it saves no *result* array — its win appears once it
  composes with a prefix (`dissolution-fold-fuse.lisp`). A lone `count` produces a
  scalar too, but its stdlib arm walks with a `letrec` closure, so
  `dissolution-count-fuse.lisp` weighs the lone case as well as the prefix one. Each
  search reads that way — `dissolution-search-fuse.lisp` weighs all four lone cases
  against their un-fused twins over an **undecided** walk, where both forms visit
  every element and the saving is the walker closure and its cell. Weighing a walk
  that decides early would measure the wrong thing: the fused loop runs one full
  iteration for the deciding element where the recursive walker answers without its
  final recursive step, and that odd step costs more than the two objects fusion
  removes. The **prefixed** case is weighed against a reference that fuses the
  prefix and leaves the search un-fused (the same stdlib function under a user
  binding, which is not `is_primitive`), so the difference is the intermediate array
  plus what the search's own dissolution removes. So the **early exit** carries a
  gauge of its own, and not an allocation
  one: a predicate that errors on every element past the decision completes only if
  the walk truly stops there. (A call tally would need a mutable global, whose
  reference makes the predicate a capture and declines fusion — so the error is what
  can gauge the fused loop.) Over a prefix the same instrument gauges both halves of
  the split: a predicate that errors past the decision proves the sentinel gate
  holds it off, and a prefix stage that errors past the decision proves the walk
  did not stop. A **`take-while`** reads both ways at once
  (`dissolution-take-while-fuse.lisp`): its lone case is weighed over an undecided
  walk, as a search's is, and its early exit carries the same error instrument —
  lone, a predicate that errors past the decision proves the walk stopped; over a
  `map` prefix, a transform that errors past the decision proves it did not. A
  **`drop-while`** (`dissolution-drop-while-fuse.lisp`) weighs its lone case against
  the un-fused twin, where the saving is two walker closures and their two cells;
  its own instrument is the mirror image — a predicate that errors on an element
  past the first rejection completes only if the flag really stops the predicate,
  while the walk itself must still reach that element to push it. The
  intermediate is non-escaping and freed before the call returns, so it is
  invisible to every live/peak/steady-state axis — the leak oracle included; only
  a cumulative allocation-event count sees it, and it is deterministic (no
  GC-timing noise), so these are exact `<` relations.
- **Value + soundness.** `tests/elle/dissolution-map-fuse.lisp`,
  `dissolution-filter-fuse.lisp`, `dissolution-mixed-fuse.lisp`,
  `dissolution-fold-fuse.lisp`, `dissolution-count-fuse.lisp`,
  `dissolution-search-fuse.lisp`, `dissolution-take-while-fuse.lisp` and
  `dissolution-drop-while-fuse.lisp` (whose
  value pins carry the two array-arm facts: the mutable result, and the `()` an
  empty base answers with)
  (value-preserving, incl. the declined shapes, the reorder-gate fallback, the
  mutable-base arm — an unfrozen, in-place-mutable result — and named-function
  inlining, incl. a `let`-body function that now fuses by cloning its freshened
  inner bindings, whose un-fused cross-check oracle is a `match`-body function that
  still declines the clone). The `(numeric!)` raw-intrinsic kernel is proved
  against an un-fused oracle of the same shape — a `match`-body function that
  declines the clone and so runs the real stdlib op — so the carried floor is
  shown to preserve both the value and the compile. Soundness:
  `tests/elle/region-map-fuse-uaf.lisp` /
  `region-filter-fuse-uaf.lisp` / `region-mixed-fuse-uaf.lisp` /
  `region-fold-fuse-uaf.lisp` / `region-count-fuse-uaf.lisp` /
  `region-search-fuse-uaf.lisp` (whose `find` hands a base element out of the loop) /
  `region-take-while-fuse-uaf.lisp` (whose accumulator holds base elements the walk
  stopped short of exhausting) /
  `region-drop-while-fuse-uaf.lisp` (whose accumulator holds base elements the
  predicate never read, the leading run it did read entering nothing)
  (guardfree over heap element/base/accumulator values,
  including a mutable-base heap result mutated in place, a cloned same-unit
  named-function body, and a cross-unit stdlib `defn` (`identity`) inlined over heap
  elements).

The leak oracle is only a non-regression check here.
