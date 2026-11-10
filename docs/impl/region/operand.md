# What a tail-call operand hands the call

<!-- audited: 2026-11-10 -->

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
the reading passes a `let` or `letrec` into its body alone, an `if` into its
arms, a `cond` into its bodies and its else, a `match` into its arm bodies, a
`begin` into its tail, and an `and` or `or` into every operand, since any of
them may be the value. A `block` is read whole, because a `break` anywhere
inside it carries the block's value. A region that only an evaluation position
names owes the frame its release, and the relocation carries it ahead of the
call.

Read over every child instead, a value a `let` binds and both arms hand to a
native is exempt as if the callee had received it. Its release stays in the
dead block, and each call strands it. The everyday shape is a worker spawn: the
closure to run is bound once, each arm hands it to a different spawn, and the
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

Pinned by `a_let_bound_value_the_arms_hand_a_native_is_released_before_the_tail_call`
and `a_let_bound_value_an_arm_hands_the_call_stays_behind_it` in
[lir::lower::tests::release::operand](../../../src/lir/lower/tests/release/operand.rs),
by [region-tail-operand-let.lisp](../../../tests/impl/region-tail-operand-let.lisp)
(the reclamation, with the moved value as the control) and by
[region-tail-operand-let-uaf.lisp](../../../tests/impl/region-tail-operand-let-uaf.lisp)
(the soundness complement: a value the arm embeds or borrows out of the bound
value survives the moved release).
