# The return mint

<!-- audited: 2026-11-20 -->

A function hands its caller one reference to its result, minted at a HIR `Return` node or at a native tail call's fall-through.

A call hands its caller one owning reference to the result's runtime region,
and the caller's single `DecrefValueRegion` at the result's `decref_point`
consumes it ([the mechanism](mechanism.md)). So the callee mints **once** per
returned value. Two lowering sites can supply the mint, and which one applies
is decided by whether the result is *named*:

- **the `Return` mint** (`lower_return`) — the named path. ANF binds the tail
  value to a synthetic slot, so the frame holds its own reference. The mint
  raises RC. The binding's `decref_point`, extended past the mint by
  `return_sites`, drops the frame's reference and leaves net one for the
  caller.
- **the `TailCall` fall-through retain** (`lower_call`'s tail arm) — the
  anonymous path. A *native* tail call pushes no bytecode frame, so on normal
  completion the dispatch loop runs the post-`TailCall` block before the
  enclosing lambda returns. In a **propagating** tail position (a `let`/`lambda`
  body, which ANF deliberately leaves unnamed) there is no binding, hence no
  `decref_point` to balance a `Return` mint — the fall-through retain *is* the
  mint.

## A `Return` node covers a call it does not wrap

`Return` is a HIR node, not a form of the language.
[return_incref.rs](../../../src/hir/return_incref.rs) puts one around each
value that leaves the function and is not a tail call. The two mints cover the
same value whenever a marked tail call sits outside a tail position of the tree
that walk reads. `mark_tail_calls` marks the call before functionalization, and
the walk runs after ANF, so the two can disagree. The call keeps its mark, and
its value still leaves the function through a `Return` node:

| Shape | What takes the call out of tail position |
|-------|------------------------------------------|
| ANF's wrap `(let [t (f …)] t)`, whose `t` the walk wraps | a binding's init is not a tail position |
| a tail `if`/`cond`/`match` whose arm assigns a mutable binding | functionalization binds the branch's value, then merges that binding's versions ahead of the `Return` node |
| the last operand of a tail `and`/`or` | the `Return` node wraps the whole form, so no operand is a tail position |

Emitting both retains the result twice against one release: a leak of one
region per call, growing per loop iteration. So the fall-through retain
**stands down** for every marked tail call that the walk reaches outside a tail
position (`return_minted_calls`). The named path's mint-then-release accounting
then carries the convention alone. A frame-replacing *closure* tail call
reaches neither instruction (the callee emits its own `Return` mint), so the
rule is uniform over callee kinds.

Two narrower sites suppress the fall-through retain for the same "exactly one
reference" reason. One is a `-mut` pass-through store/remove funnel whose
dispatch wrapper released the container owned-param reference here
(`container_release_sites`). The other is a moves-out ∩ `PassThrough` native
whose in-body escape retain is already the caller's reference
(`moves_out_release_sites`).

## Pinning tests

- [region-native-tail-compound-leak.lisp](../../../tests/impl/region-native-tail-compound-leak.lisp)
  reads the per-shape region-count deltas: bare, `let`-body, `begin`-nested and
  `if`-nested, over Fresh, Funnel and pass-through natives.
- [region-native-tail-moved-leak.lisp](../../../tests/impl/region-native-tail-moved-leak.lisp)
  reads the two shapes functionalization and `and`/`or` make.
- [region-native-tail-return-uaf.lisp](../../../tests/impl/region-native-tail-return-uaf.lisp)
  and [region-hof-tail-return-uaf.lisp](../../../tests/impl/region-hof-tail-return-uaf.lisp)
  are the soundness complement: the anonymous path must keep its retain.
- [region-native-tail-moved-uaf.lisp](../../../tests/impl/region-native-tail-moved-uaf.lisp)
  is the complement for a covered call: its result must survive its caller's
  borrow on the `Return` node's mint alone.
