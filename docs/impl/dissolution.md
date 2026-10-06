# Dissolution — HOF loop fusion

<!-- audited: 2026-10-06 -->

The pass that turns a higher-order call over a proven array into a loop with the
function's body spliced in.

Dissolution is the third leg of the region system ([memory.md](memory.md)). A
closure is a first-class *value* but the *unit of nothing* at run time. Guided
by the escape and ownership facts that legs 1 and 2 infer, the compiler
realizes a higher-order call in its most efficient form.

`(map f xs)` over an owned, non-escaping `xs` exposes no observable closure and
no observable intermediate collection. The compiler is therefore free to
realize it as a plain loop with `f`'s body spliced in, a JIT'd group, CPU SIMD,
or a device dispatch. This document specifies the first realization:
**HOF-chain loop fusion** on the VM substrate
([src/hir/typeinfer/fuse.rs](../../src/hir/typeinfer/fuse.rs)).

The pass covers six array-producing higher-order ops: `map`, `map-indexed`,
`filter`, `take-while`, `drop-while` and `mapcat`. It also covers the
scalar-producing terminals: the left-fold `fold`/`reduce`, the predicate tally
`count`, and the four short-circuiting searches `any?`/`all?`/`find`/`find-index`.

Three companions hold the rest: [the pipeline stages](dissolution/stages.md) other
than `map`, [the scalar terminals](dissolution/terminals.md), and [whose body the
loop splices](dissolution/inline.md) when the function is a named one.

## What the pass does

Take a call `(map f xs)` where `xs` is a statically proven immutable array and
`f` is a single-parameter lambda written at the call site. The pass rewrites the
call to the index-walk loop that `map`'s own array arm runs
([src/stdlib.lisp](../../src/stdlib.lisp), the `(array? coll)` arm). The
difference is that **`f`'s body is spliced directly into the loop body**,
instead of called through a closure value:

```lisp
# (map (fn [x] BODY) [ … ]) becomes this surface HIR:
#
#   (let [coll [ … ]]
#     (let [len (length coll)]
#       (let [acc (@array)]
#         (define i 0)
#         (while (< i len)
#           (push acc (let [x (get coll i)] BODY))
#           (assign i (%add i 1)))
#         (freeze acc))))
#
# The examples in this document read the HIR the compiler emits after
# functionalization, where the `while` is a `loop`.

(def {:fused fused :occurrences occurrences}
  (import-file "dissolution/hir.lisp"))

(def map-hir (fused "(map (fn [x] (* x 10)) [1 2 3])"))
(assert (= 0 (occurrences map-hir "map#")) "the map call is gone")
(assert (= 1 (occurrences map-hir "(loop ")))
(assert (= 1 (occurrences map-hir "%push-array-mut")) "BODY is pushed inline")
(assert (= 1 (occurrences map-hir "freeze#")))
(assert (= (map (fn [x] (* x 10)) [1 2 3]) [10 20 30]))
```

The closure `f` is gone. No closure value is allocated, and no indirect call
through one happens per element: `BODY` runs inline against a let-bound element.
The `map` dispatch is gone too, a cross-unit stdlib call whose `cond` selects
the array arm.

The emitted form is *surface* HIR, the same shape `map`'s body has before
functionalization. So every downstream pass (functionalize, ANF, region
inference, the lowerer) consumes it exactly as it consumes `map`'s own body. The
`while` becomes a `loop`/`recur`, `push` monomorphizes to `%push-array-mut` on
the proven `@array` accumulator, and the result is one frozen array.

The pass sets a signal on each synthesized helper call
(`get`/`push`/`freeze`/`<`/`length`/`@array`) and on the synthesized `if`/`let`
scaffolding. That signal is the original call's, a sound upper bound, because
that call's signal already subsumes every op in the stdlib op's body. So the
bottom-up signal re-propagation ([src/hir/narrow.rs](../../src/hir/narrow.rs))
never under-reports the fused form's effects. The spliced lambda bodies keep
their own signals.

## Composition — the intermediate collection dissolves

A chain `(map g (map f xs))` fuses to a **single** loop. The pass peels the
chain down to its base array `xs` and collects the per-element transforms
`[f, g]` in application order. It emits one accumulator loop whose element
expression nests the transforms, innermost first:

```lisp
# (push acc (let [gp (let [fp (get coll i)] F-BODY)] G-BODY))

(def map-map-hir (fused "(map (fn [y] (+ y 1)) (map (fn [x] (* x 10)) [1 2 3]))"))
(assert (= 0 (occurrences map-map-hir "map#")))
(assert (= 1 (occurrences map-map-hir "(@array#")) "one accumulator, no intermediate")
(assert (= 1 (occurrences map-map-hir "(loop ")))
(assert (= (map (fn [y] (+ y 1)) (map (fn [x] (* x 10)) [1 2 3])) [11 21 31]))
```

The inner `map` would have allocated and frozen an intermediate array, and the
outer `map` would have walked it. That array **never exists**: there is one
loop, one accumulator, and no intermediate. A `map`-tower reduces to this
shape, and the depth of the chain becomes the nesting depth of one element
expression, not a stack of allocations.

## When it is legal — the gate

Fusion preserves the program's value. For a single op (`map` or `filter`), it
also preserves the exact per-element evaluation order: the loop visits each
element left to right and applies `f`/`p` as the stdlib op does. The gate:

- **The callee is a canonical stdlib HOF.** A pipeline op is `map`,
  `map-indexed`, `filter`, `take-while`, `drop-while` or `mapcat`. The optional
  outermost terminal op is `fold`, `reduce`, `count`, or one of the four
  short-circuiting searches `any?`/`all?`/`find`/`find-index`.
  - The pass recognizes the callee by name, when its binding is `is_primitive`.
    `bind_primitives` binds every stdlib and core export so, and marks the
    canonical core-env override `is_primitive` too, so `fold`/`reduce` reach
    the gate as `map`/`filter` do.
  - A user redefinition shadows with a non-primitive binding, so the pass never
    rewrites one.
  - A `count` or a search call has a `filter`'s two-argument shape. The pass
    therefore recognizes the terminal before the pipeline walk starts, and never
    reads either as a stage.
- **`xs` is a proven immutable array.** The base is one of these:
  - an array literal (`[ … ]`, which analyzes to a call to the `array`
    primitive, `RetType::Array`), or any `RetType::Array` primitive call at the
    call site;
  - a `Var` alias whose initializer is such an array, followed to a fixpoint
    through immutable, unmutated, singly-bound `let`/`def` bindings;
  - another fusable same-HOF chain over such a base.

  The alias proof is the **same** one that dead-arm pruning reads at this
  stage: the binding→concrete-`type-of`-keyword map that `prune.rs` builds
  (`classify_init`/`resolve`). A base whose keyword resolves to `array` is a
  proven immutable array. Reusing that map is deliberate. An over-broad
  classification there deletes a live match arm (a UAF), so the map already
  carries the soundness bar that fusion needs, where an over-broad base is a
  miscompile.

  A base whose keyword resolves to `array` selects the frozen-result arm. A
  mutable `@array` base (keyword `@array`, or a `RetType::MutableArray` producer
  call) selects the unfrozen-result arm, under the tighter gate of "The
  mutable-array arm" below.
- **The function has the op's fixed arity.** That is one parameter for a
  `map`/`filter`/`take-while`/`drop-while`/`mapcat`/`count`/search (the
  element). It is two for a `fold` (the accumulator and the element) and two
  for a `map-indexed` (the position and the element).
  - The function has no rest parameter and no parameter that the body
    reassigns.
  - Its body is free of nested lambdas, and of call-position `%`-intrinsics
    unless the function declares `(numeric!)`. [Inline](dissolution/inline.md)
    carries that declaration into the loop.
  - The fixed parameter count is what makes the loop's element bind 1:1, and
    for a fold the accumulator too.

  The function is one of three forms:
  - a **lambda literal** written directly as the call's argument. The rewrite
    consumes it, moving it out of the call, so no other use can observe the
    change, and its parameter is retyped to a loop-local in place. It may
    **capture**: its free variables are in scope at the splice. "Captures"
    below says what that costs the composition gate.
  - a **`Var` referencing a same-compile-unit function** whose initializer is
    such a lambda (a top-level `(defn f …)` or a `let`/`def`-bound `(fn …)`),
    inlined by cloning;
  - a **stdlib `defn`** such as `inc`, whose body a registry carries across the
    compile-unit boundary.

  [Inline](dissolution/inline.md) owns the two named forms. Each is a
  **non-capturing** template: its body names the scope it was defined in, not
  the one it splices into.
- **A `mapcat`'s function body proves an array result.** Only that op reads
  what its function returns as a *collection*, and only the indexed walk is
  linear ([stages](dissolution/stages.md)). The pass reads the body with the
  same `classify_base` proof it reads the base with. So the body qualifies as a
  call-site array producer or a `Var` alias of one. A named function's fragment
  runs that proof in its defining unit and records the answer. Every other op is
  indifferent to what its function returns.

A **composition** is a chain of length ≥ 2, homogeneous *or* mixed. For one,
the pass also requires each lambda body to be free of **sequencing effects**: no
yield, I/O, emit, FFI, or halt (`reorder_safe`). Composition interleaves the
per-element work (`f x0; g …; f x1; g …`, or `p x0; q …; p x1; q …`), where the
stdlib runs all of the first op and then all of the second. So it reorders that
work.

The *value* is unchanged either way. Each stage still runs on exactly the
elements it would have: the outer op on the inner's outputs, left to right. A
`filter` runs on its predecessor's survivors or mapped values, and a `map` on
its predecessor's survivors. Only the *interleaving* of the two lambdas' calls
differs, and the gate makes it unobservable.

This is why a mixed chain is gated as a homogeneous one is. A mixed chain always
has length ≥ 2, so it always carries the reorder requirement. A
non-reorder-safe stage declines the whole composition, for example a variadic
comparison like `>`, which routes through `apply`. The chain then falls back to
fusing only its inner reorder-safe run.

A **capture** is a second cross-element channel, and it carries no signal at
all. A composition therefore also requires every lambda to be non-capturing
(§ "Captures"). Otherwise, reordering is observable only through a sequencing
effect, which is a non-capturing lambda's only cross-element channel. A body
with none reorders unobservably.

`SIG_ERROR` is deliberately permitted. Reordering errors changes only *which*
of several errors surfaces, and each still surfaces as an error. A dissolvable
numeric kernel over proven data does not error, and refusing it would forbid
every arithmetic tower, the shape this fusion exists to collapse. A single op
never reorders and carries no such requirement.

The **terminal counts as an op** in the chain length:

- A lone `fold` (`(fold f init xs)`, length 1) threads its accumulator strictly
  in element order, exactly as the stdlib fold does. It never reorders and needs
  no gate, even with a sequencing-effectful body.
- A lone `count` visits each element left to right and applies its predicate as
  the stdlib op does, so it reads the same way. A lone search reads the same way
  again, up to the element that decides it.
- A terminal *with* an inner `map`/`filter` prefix has length ≥ 2. It carries
  the reorder requirement over every lambda, the terminal's and the prefix's.
  The terminal's per-element work interleaves with the prefix transforms
  (`g x0; f …; g x1; f …`), the same reorder a mixed chain makes.
- A non-reorder-safe stage declines the whole composition. The pass then falls
  back to fusing the inner reorder-safe run (the prefix), and leaves the
  terminal a plain call over the fused loop.

## The mutable-array arm

`map`/`filter` are **type-preserving**. Over an immutable array they return a
frozen array, but over a **mutable** `@array` they return the accumulator
*unfrozen*. The stdlib arm is literally `(if (mutable? coll) acc (freeze acc))`
([src/stdlib.lisp](../../src/stdlib.lisp)), and fusion mirrors it.

The base's proof may resolve to the `@array` keyword: a `@[ … ]` literal, a
`RetType::MutableArray` producer call (`thaw`, …), or a `Var` alias of one. The
base is then **statically** known mutable. `freeze` never mutates in place: it
copies to a *new* immutable value and leaves its input mutable, so a
proven-`@array` binding is mutable at every use. The fused loop then emits the
accumulator **unfrozen** instead of `(freeze acc)`, and the loop body is
otherwise identical to the immutable arm.

A mutable base fuses under a **strictly tighter gate: a single `map`, `filter`
or `mapcat` only**, with no terminal and no composition. The fused loop walks
the base *live*: it reads `(get coll i)` each iteration against a `len`
captured once. That matches the stdlib op **exactly** for those three. Each
array arm captures `len` once (`mapcat`'s through the `each` macro's own indexed
arm) and reads `coll` live.

So the value is preserved even when the lambda mutates the base through an
alias, a global or a capture of the base's own binding (§ "Captures"). A
`mapcat`'s inner walk reads its function's result the same way, so it matches
there too. The three excluded shapes break the match:

- **`fold`** first snapshots its input: `trait/elements` converts it with
  `->array`, which copies a mutable array. It then walks the copy. A fused fold
  would walk the live base, so a mutating combinator would observe a
  divergence. A `fold` over a mutable base stays a plain call.
- **`count`** re-reads `(length coll)` on every iteration, where the fused loop
  captures `len` once. A predicate that pushes to or pops from the base would
  observe a divergence, so a `count` over a mutable base stays a plain call.
  Each **search** array arm re-reads it the same way, and so do the arms of
  **`map-indexed`**, **`take-while`** and **`drop-while`**. All decline for the
  same reason.
- **A composition** (`(map g (filter p @xs))`, …) runs each stdlib op to
  completion over a *fresh* array before the next begins. A later op's lambda
  that mutates the original base can then no longer affect the result. The
  single fused loop interleaves the ops against the live base, where such a
  mutation *would* affect later reads. So a composition over a mutable base
  declines. The pre-order recursion still fuses its innermost single
  `map`/`filter`/`mapcat`, sound in isolation, and leaves the outer ops as plain
  calls over that fused loop.

For an **immutable** base none of the hazards exists, because the base cannot be
mutated. The terminals and compositions fuse over it exactly as before.

## Captures — a literal's free variables are in scope at the splice

A call-site lambda literal is **moved** out of the call, and its body is spliced
where the call was. Every free variable that body reads is therefore bound by an
enclosing scope of the splice, which is what writing the lambda there means.

The spliced `Var` names the same binding the closure would have captured:
`CaptureInfo::binding` is the enclosing binding itself, never a per-closure slot. So
the enclosing function resolves that name exactly as it resolves any other name it
holds. A `Local` capture is one of that function's own slots. A transitive `Capture`
is one the enclosing lambda already carries in its capture list, because a nested
lambda's captures propagate outward when it is analyzed. A capturing literal
therefore splices with no rename and no machinery, and `(map (fn [x] (* x k)) xs)`
dissolves as `(map (fn [x] (* x 2)) xs)` does.

A capture marks its binding celled. The spliced read unwraps that cell exactly as
every other read of the binding does, so a mutable capture is read live per element,
as the closure read it.

The one capture kind that declines is the **self-reference**
(`CaptureKind::Recursive`). It names the executing closure rather than a binding the
enclosing frame holds, and fusion removes the closure it would name.

The two **template** paths cannot have this fact, which is why the refusal belongs to
the clone rather than to the splice. A named function's body names the scope it was
*defined* in, and the call site need not sit inside that scope. So `fn_fragment`
refuses a body that names an enclosing runtime local, and the cross-unit
collector admits only free variables that are genuine globals
([inline](dissolution/inline.md)).

### A capture is a second cross-element channel

What a capture costs is the **composition** gate. The reorder argument (§ "When it is
legal — the gate") rests on one premise: a non-capturing lambda's only cross-element
channel is a sequencing effect. Interleaving `f x0; g …; f x1; g …` is unobservable
when each body reads only its parameters and globals. A captured binding is a second
such channel, and it raises no signal to gate it with: a mutable local two bodies
share, or a mutable value an immutable local names. A chain that interleaves two
lambdas cannot admit one.

A capturing lambda therefore fuses **only in a chain of one op**. That chain carries
no reorder requirement at all: a lone op applies its function to each element left to
right, exactly as the stdlib op does, so whatever state the body reaches behaves
identically. A capture anywhere in a longer chain declines that chain whole, and the
pre-order recursion still fuses its inner run.

## Where it runs

The pass runs in `regularize` ([src/hir/regularize.rs](../../src/hir/regularize.rs)),
after dead-arm pruning and **before** `functionalize`, on surface HIR. Running
before functionalize lets the pass emit ordinary `while`/`push`/`freeze` HIR. The
same machinery that lowers `map`'s own body then lowers the loop, so the pass
never constructs a `loop`/`recur` or a capture cell by hand.

The proven-immutable-array fact it needs is the same one dead-arm pruning reads
at this stage (`typeinfer/prune.rs`, `classify_init`): a literal's constructor,
or a primitive call's declared `RetType`.

## Why a call-site rewrite, not a stdlib edit

Dissolution is one mechanism keyed on proven-owned-non-escaping inputs, not a
per-function hand-rewrite. The production `zip` was hand-fused once for an RSS
win, and that is exactly the manual gap-bridging this pass exists to replace. The
pass fires structurally on *any* `(map f xs)` that matches the gate: it
enumerates no user function and hand-writes no composition.

It mirrors the container-dispatch monomorphization
([src/hir/typeinfer/monomorphize.rs](../../src/hir/typeinfer/monomorphize.rs)).
Both recognize a proven-type call across the compile-unit boundary and collapse
it to the direct form the proof selects. The general dispatch, and everything it
strands, then ceases to exist.

## The gauge

Dissolution is a **realization** goal, not a leak goal. It is proven at the
codegen and execution levels, and the leak oracle is only a non-regression check.
Three instruments pin it:

- **Structure.** The unit tests under
  [src/hir/typeinfer/fuse/tests](../../src/hir/typeinfer/fuse/tests), one module
  per op, compile a form and assert on the HIR the pass emits: the callee gone,
  the body inline, one accumulator, and every decline the gate owes.
- **Realization.** Each `dissolution-*.lisp` file in
  [tests/impl](../../tests/impl/overview.md) weighs a fused form against an
  un-fused reference that computes the same value, by `arena/total-allocs`. The
  count is cumulative, because the intermediate a fusion removes is non-escaping
  and freed before the call returns. No live, peak or steady-state gauge sees
  it, the leak oracle included. The count is deterministic, so each file asserts
  an exact `<`.
- **Soundness.** Each `region-*-fuse-uaf.lisp` file runs a fused loop over heap
  values under `--trace=guardfree`.

Each file says what its shapes pin, and why it weighs a shape the way it does.
