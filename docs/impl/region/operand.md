# What a tail-call operand hands the call

<!-- audited: 2026-11-13 -->

The regions of the value an operand produces, never the regions its evaluation used on the way.

[The relocation](relocate.md) owns the release a frame-replacing tail call
leaves dead and the move that carries one ahead of the call. This document owns
the reading that says which regions the call itself names through an operand,
so that their releases stay behind the call as the ownership move.

## The value, not the syntax

**What an operand names is its VALUE, not its syntax.** The reading descends
the value-transparent wrappers: a `Let`/`Letrec` body, a `Begin`/`Block` tail,
a branch arm, an `And`/`Or`, a `DerefCell`, a `Return`. It stops where the value
is produced, and records that node's own region, because that region *is* the
value handed over. It does **not** descend a `Call`'s callee or arguments, nor a
`Lambda`'s captures. A region reached only in there is one the operand's own
evaluation used and finished with before the tail call was made. Exempting it
leaves a release the frame still owes emitted where control never arrives.

`(f (g x))` hands the callee `g`'s **result**, and `g`'s own closure region is
not reachable from the call at all. What the produced value does still hold, it
holds by a **counted** (or owning) edge in each case. A call's result carries
exactly one minted reference ([the return mint](mechanism.md)). A closure's env
took the funnel's count when it was built ([the frame-held admission](held.md)).
So the frame's own release remains the only reference it drops.

An inline `%`-opcode that allocates (`%pair`, `%freeze`, `%thaw`) is such a
node too. Its result lives in the region it mints, and that result holds each
operand by the counted edge the allocation takes. Every other inline `%`-opcode
mints no region. Its heap result (`%first`/`%rest`/`%get`) is an uncounted
borrow living *in* its operand's region, so the operand is the value-producing
leaf and the descent continues through it. A name bound to such a result hands
the call the borrow it carries ([borrow.md](borrow.md)), so the reading takes
that borrow's containers along with the name's own regions. The closure-cycle
merge's by-move boundary reads the same question the same way, for the same
reason ([cycletail.md](cycletail.md)).

The difference shows in a recursion that threads an accumulator. In
`(step (%add i 1) n (%pair i acc))`, the call receives the new pair, and the pair
holds `acc` by its own count. Descending the `%pair` would exempt `acc`'s release
too, and nothing takes that release over, so each call would strand the
accumulator it displaced. The release reaches the recursive arm at all only
because `acc` is frame-held there: escape reads the pair's counted hold as no
containment ([escape analysis](../escape.md)).

```lisp
(def step (fn [i n acc] (if (%lt i n) (step (%add i 1) n (%pair i acc)) acc)))
(defn stranded-by [f]
  "How many regions 200 runs of f leave live."
  (def r0 (arena/region-count))
  (def @j 0)
  (while (%lt j 200) (f) (assign j (%add j 1)))
  (- (arena/region-count) r0))
(assert (< (stranded-by (fn [] (length (step 0 4 ())))) 50)
        "each displaced accumulator is released before the tail call")
```

## Evaluation is not the value

A `let`'s bindings and a branch's condition run before the operand's value
exists, and nothing of them reaches the callee except through that value. So
the reading passes each form into the positions that may be its value:

| Form | Read into |
|------|-----------|
| `let`, `letrec` | the body alone |
| `if` | both arms |
| `cond` | each body, and the else |
| `match` | each arm body |
| `begin` | the tail |
| `and`, `or` | every operand |
| `block` | the whole block, because a `break` anywhere inside carries its value |

A region that only an evaluation position names owes the frame its release, and
the relocation carries it ahead of the call.

Read over every child instead, a value a `let` binds and both arms hand to a
native is exempt as if the callee had received it. Its release stays in the
dead block, and each call strands it. The everyday shape is a worker spawn. The
closure to run is bound once and each arm hands it to a different spawn. The
handle the arm answers goes to a `join` that replaces the frame.

```lisp
(defn sink [h]
  "A callee whose body no graft replaces, so the call to it replaces the frame."
  (block :keep (length h)))
(defn spawn-and-count [x c]
  (sink (let [a (array x x)]
          (if c (list (length a)) (list (length a) (length a))))))
(assert (= (spawn-and-count 1 true) 1) "the arm's own value reaches the callee")
```

The pins:

- `a_let_bound_value_the_arms_hand_a_native_is_released_before_the_tail_call`
  and `a_let_bound_value_an_arm_hands_the_call_stays_behind_it` in
  [lir::lower::tests::release::operand](../../../src/lir/lower/tests/release/operand.rs).
- [region-tail-operand-let.lisp](../../../tests/impl/region-tail-operand-let.lisp),
  the reclamation, with the moved value as the control.
- [region-tail-operand-let-uaf.lisp](../../../tests/impl/region-tail-operand-let-uaf.lisp),
  the soundness complement. A value the arm embeds or borrows out of the bound
  value survives the moved release.

## A branch hands the call one arm's value

The reading takes the union over a branch's arms, so a region that any arm
names is exempt from the frame's release. That is right on the path through an
arm whose value names the region. On a sibling arm the call receives a
different value, and the frame still owes the release.

The everyday shape is the list walker of stdlib `filter`, which keeps an element
or passes the accumulator on:
`(go (rest c) (if (p (first c)) (pair (first c) acc) acc))`. The `acc` arm
hands the call `acc`, so its release stays behind the call as the ownership
move. The pair arm hands the call a pair that holds `acc` by its own count.
With the release left behind the call on that path too, each kept element
strands the accumulator it displaced.

So the end of each arm of a branch operand is a relocation point of its own
([the relocation point](replicate.md)). It admits only a region that the call
exempts through a sibling arm of the same operand and through nothing else. A
region that the callee, another operand or the arm's own value names keeps its
exemption there. The release is replicated into the arm after the arm's value
is built, and it nil-stamps the slot it read. So on that path the copy behind
the call loads `nil` and does nothing.

The arm runs before the operands after it and before the callee. A release
there must not free a value that this later evaluation reads, through any
binding, and the stamp protects only the slot the release read. So a region
that a later operand or the callee names anywhere, in value position or not, is
not admitted.

An arm admits a region that the operand allocates only where every allocation
of it in the operand is in the arm's prefix. The prefix is the evaluation that
runs on each path to the arm, in each form between the operand and the arm: a
`let` init, a `begin` expression but the last, an `if` condition, the `cond`
tests up to the arm's clause, a `match` scrutinee, and the `and` or `or`
operands ahead of the arm. On each path to the arm
the prefix wrote the region's slot, so the release frees what the frame holds.
A region allocated anywhere else in the operand is not admitted. On a path that
did not allocate it, the slot reads `nil` ([rules.md](rules.md)), so a release
there would do nothing.

In `(go (rest c) (let [t (pair x acc)] (if p t acc)))` the `let` binds the pair
on both paths, and only the `t` arm hands it to the call. So the `acc` arm
releases it.

The count argument is the frame-held admission ([held.md](held.md)), as for
every replica.

A short-circuit `and` or `or` is a branch too. Its value is the operand it
stopped at, so each operand is an arm. An operand but the last stops it on one
edge of its test, and runs the next operand on the other. The lowering sends
the stopping edge through a block of its own, and the end of that block is the
arm's end. An arm inside an operand but the last is no arm of the tail operand,
because on the path past it the test can still run the next operand.

```lisp
(defn keep-positive [c acc]
  (if (empty? c)
    (length acc)
    (keep-positive (rest c) (if (< 0 (first c)) (pair (first c) acc) acc))))
(assert (< (stranded-by (fn [] (keep-positive (list 1 -2 3 4) ()))) 50)
        "each kept element releases the accumulator it displaced")
(defn keep-bound [c acc]
  (if (empty? c)
    (length acc)
    (keep-bound (rest c)
                (let [t (pair (first c) acc)] (if (< 0 (first c)) t acc)))))
(assert (< (stranded-by (fn [] (keep-bound (list 1 -2 3 -4) ()))) 50)
        "each dropped element releases the pair the let bound")
(defn keep-or [c acc]
  (if (empty? c)
    (length acc)
    (keep-or (rest c) (or (and (< 0 (first c)) (pair (first c) acc)) acc))))
(assert (< (stranded-by (fn [] (keep-or (list 1 -2 3 4) ()))) 50)
        "each element an or keeps releases the accumulator it displaced")
```

The pins:

- `a_branch_operand_arm_releases_what_only_its_sibling_hands_the_call`,
  `a_branch_operand_arm_releases_nothing_the_operand_allocates` and
  `a_branch_operand_arm_releases_what_its_prefix_binds` in
  [lir::lower::tests::release::armpoint](../../../src/lir/lower/tests/release/armpoint.rs).
- `a_short_circuit_operand_arm_releases_on_its_stopping_edge` in
  [lir::lower::tests::release::shortcircuit](../../../src/lir/lower/tests/release/shortcircuit.rs).
- [region-tail-operand-arm.lisp](../../../tests/impl/region-tail-operand-arm.lisp),
  the reclamation, with the arm that hands the call the accumulator as the
  control. A pair that the prefix binds is reclaimed too.
- [region-tail-operand-or.lisp](../../../tests/impl/region-tail-operand-or.lisp),
  the reclamation of an accumulator that an `and` or `or` displaces.
- [region-tail-operand-arm-uaf.lisp](../../../tests/impl/region-tail-operand-arm-uaf.lisp),
  the soundness complement. The accumulator survives on every arm, and a later
  operand that reads it holds the release back. A pair that the prefix binds
  survives where the call or a later operand reads it. An accumulator that a
  later `or` operand hands over survives an arm inside an earlier operand.
- [region-tail-operand-arm-uaf-syncjit.lisp](../../../tests/impl/region-tail-operand-arm-uaf-syncjit.lisp),
  the same witnesses with each function compiled on its first call, so the self
  tail call is a loop from the first trip.
