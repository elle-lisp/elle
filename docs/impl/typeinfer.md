# Type inference: the ascent, and what a call proves

<!-- audited: 2026-09-08 -->

Where the types come from: an ascent from below whose limit is the least
fixpoint, and what each kind of call contributes to it.

[intrinsics.md](../intrinsics.md) says what each `%`-intrinsic contract needs.
This file says where the types that discharge it are computed, and what happens
when the computation does not settle. The pass is `src/hir/typeinfer/`.

## The ascent

One pass walks the whole tree, records a type for every node and every binding,
and leaves that environment behind. The next pass walks the same tree over the
environment the last one left. The passes stop when one of them changes
nothing.

The start is Bottom, not Top. Every parameter a complete enumeration of call
sites can prove — the binding is used only as a callee, the parameter is not
mutated — is seeded at Bottom, so a recursion that passes its own argument
through contributes nothing on the way up instead of pinning the parameter at
Top. A parameter whose callers are not all visible is left absent, and an
absent entry reads as Top.

A pass re-derives, it never accumulates. Each parameter's type is **replaced**
at pass end by that pass's complete join over the call sites, and each lambda's
body type is replaced by what its body computed on that pass. A join that only
grows can never come back down, so one early reading of an unfinished estimate
would hold every later pass at Top.

This is Kleene iteration: every pass reads strictly more than the last, the
estimate rises, and the limit of the ascent is the least fixpoint.

## What a call contributes

| Callee | The call's type |
|---|---|
| a lambda binding this unit defines | that lambda's body type, as the previous pass left it |
| a registered primitive | its declared `RetType`, read from the primitive tables |
| a stdlib arithmetic wrapper (`+`, `abs`, `min`, …) | Number — the wrapper raises on everything else |
| anything else | Top |

**A self-recursive call is a call.** It reads the body-type map that every
other call reads, and takes no exception of its own. On the first pass the
callee's body has not been walked yet, so the map holds no entry for it and an
absent entry is Bottom — which is what the recursive contribution to a
return-type join is worth, the base cases. Every later pass reads the estimate
the pass before it computed, exactly as a mutual recursion does through the
same map.

`fib` settles on the second pass:

```lisp
(defn fib [n]
  (if (%lt n 2) n (%add (fib (%sub n 1)) (fib (%sub n 2)))))
```

Pass 1 reads Bottom for both self-calls, so the `%add` is Bottom and the body
is `Int ⊔ ⊥ = Int`. Pass 2 reads Int for both, so the `%add` is Int and the
body is Int again. Nothing moves, and the ascent is over.

The proof the second pass adds is what the backends spend: an `%add` of two
proven ints carries `:int` into LIR, which is `AddInt` on the bytecode tier and
a tag-check-free fast path on the JIT ([lir.md](lir.md)).

## An exhausted budget widens

The budget is ten passes, and nothing guarantees the ascent fits in it.
Information travels one call at a time in walk order, so a caller walked before
its callee learns the callee's type one pass late. Eleven such functions in a
row outrun the budget.

An estimate that is still moving sits **below** the least fixpoint, and below
is the dangerous side. An entry still at Bottom discharges every contract that
asks a subtype question, so a program whose ascent was cut short compiles
`(%bit-and (g1) 1)` for a `g1` that returns a string.

So an exhausted budget widens. Every entry that moved on the last pass is
pinned at Top, and the ascent runs again over the pinned environment. Top is
above the fixpoint, so no site reads a proof out of an estimate that never
settled. A pinned entry cannot move again, so each round pins at least one more
entry and the widening terminates.

Widening costs precision, and only for a program that outran the budget: a
`%`-intrinsic whose operand a widened entry feeds no longer proves, and
prove-or-reject rejects that site. The whole corpus converges in five passes or
fewer, so no program in it pays.

## What still does not prove

- **A mutated binding.** An `assign` gives a binding flow that a per-pass
  recomputation cannot see, so a mutated parameter never receives call-site
  proofs and keeps whatever a guard proves of it. The loop-accumulator spelling
  of a numeric kernel is unproven for this reason, where the recursive spelling
  of the same kernel proves.
- **A binding used as a value.** One use outside callee position — stored,
  passed to a higher-order function, returned, exported — means callers this
  unit cannot enumerate, so the call-site join over the visible ones proves
  nothing about the parameters.

## Pinning tests

- `hir::typeinfer::tests::recursion::self_recursive_call_result_is_the_body_type`
  — the `fib` ascent: the `%add` over two self-calls is Int, not Bottom.
- `…::a_self_recursive_result_proves_a_callee_parameter` — the same result
  carried one call further, into the parameters of a helper that adds it.
- `…::a_float_base_case_does_not_prove_an_int_recursive_result` — the
  over-narrowing guard, and the soundness half: while a self-call was pinned at
  Bottom, a body whose base case is a float typed its `%add` Int and let a
  bitwise op read a float's payload.
- `…::a_three_member_recursion_proves_through_the_cycle` and
  `…::a_cycle_whose_members_disagree_proves_nothing` — mutual recursion, which
  converges by the same iteration and must keep doing so.
- `…::a_chain_deeper_than_the_budget_proves_nothing` — the widening: eleven
  functions in reverse walk order outrun the ten passes, and the site that
  reads the unsettled entry is rejected rather than compiled.
