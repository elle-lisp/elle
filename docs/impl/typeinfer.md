# Type inference: the ascent, and what a call proves

<!-- audited: 2026-09-09 -->

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

The start is Bottom, not Top. A parameter is seeded at Bottom when a complete
enumeration of call sites can prove it: the binding is used only as a callee,
and the parameter is not mutated. A recursion that passes its own argument
through then contributes nothing on the way up, rather than pinning the
parameter at Top. A parameter whose callers are not all visible is left absent,
and an absent entry reads as Top.

A pass re-derives; it never accumulates. Each parameter's type is **replaced**
at pass end by that pass's complete join over the call sites, and each lambda's
body type is replaced by what its body computed on that pass. A join that only
grows can never come back down, so one early reading of an unfinished estimate
would hold every later pass at Top.

This is Kleene iteration: every pass reads strictly more than the last, the
estimate rises, and the limit of the ascent is the least fixpoint.

## What a call contributes

| Callee | The call's type |
|---|---|
| a lambda binding this unit writes | Top (§ "A written binding's result is Top") |
| a lambda binding this unit defines and never writes | that lambda's body type, as the previous pass left it |
| a registered primitive | its declared `RetType`, read from the primitive tables |
| a stdlib arithmetic wrapper (`+`, `abs`, `min`, …) | Number — the wrapper raises on everything else |
| anything else | Top |

**A self-recursive call is a call.** It reads the body-type map that every
other call reads, and takes no exception of its own. On the first pass the
callee's body has not been walked yet, so the map holds no entry for it, and an
absent entry is Bottom. Bottom is what the recursive contribution to a
return-type join is worth: the base cases. Every later pass reads the estimate
the pass before it computed, exactly as a mutual recursion does through the same
map.

`fib` settles on the second pass:

```lisp
(defn fib [n]
  (if (%lt n 2) n (%add (fib (%sub n 1)) (fib (%sub n 2)))))
(assert (= (fib 10) 55) "the recursive results prove the addition")
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
prove-or-reject rejects that site. The whole corpus converges in six passes or
fewer — the deepest is `demos/nqueens` at six — so no program in it widens.

## Bottom is not a proof

Widening treats an unsettled entry, and a settled Bottom needs the same answer
for the same reason. `subtype(⊥, b)` holds for every `b`, so a Bottom operand
discharges every contract row that asks a subtype question — Numbers,
DivFamily, Ints, Ordered, and the `%get` index — and the silent opcode lowers
over a value of any type.

Two different facts wear that one lattice element:

- **No value reaches here.** A function this unit defines and never calls has
  parameters no call site contributes to, so they keep the Kleene start.
- **Nothing has contributed yet.** A `(numeric!)` parameter carries the start
  until a pass walks a call site that refines it.

Neither says what type a value arriving at the site has. So the pass hands out
no Bottom: at the ascent's limit, every Bottom left in the node map settles to
Top, which is above the fixpoint and proves nothing.

That one rule covers every consumer, because they all read that one map — the
operand contracts (`contract.rs`), the signal narrowing (`narrow.rs`), the
wrapper monomorphization (`monomorphize.rs`), and the LIR operand proof
(`src/lir/lower/expr/intrinsic.rs`). The last two ask with equality and never
read a proof out of Bottom. The first two ask with `subtype`, and did.

A definition is therefore checked where it is written:

```text
(defn f [x] (%mul x x))
```

That is a compile error whether or not a later form calls `f`. Bottom used to
exempt it, and only in the spelling with a form after it: the same definition
alone in a file is the file's result, so `f` reads in value position, its
parameters read as Top, and the site was rejected already.

### A fact refines the start

A guard and a `(numeric!)` declaration are both proofs about a binding that owe
nothing to a call site. Both reach the environment by meeting with the type the
ascent has accumulated, and `meet(⊥, fact)` is ⊥, so the start erases them.
While Bottom discharged every row that erasure cost nothing, and nothing found
it.

A fact meeting the start **is** the fact. Nothing has contributed to the
binding, so the guard or the declaration is the whole of what is known:

```lisp
(defn g [b]
  (when (%not (%int? b)) (error :not-int))
  (%mul 2 b))
(assert (= (g 5) 10) "the guard proves b, and this unit calls g nowhere else")
```

`b` is an int in everything after the guard, whether or not this unit calls
`g`. `(numeric!)` reads the same way: it floors **on** the start rather than
meeting with it, and a floor that returns something below itself is not a
floor.

A Bottom the meet **produces** is the opposite fact — the accumulated type and
the fact are disjoint, so no value reaches the site the fact governs.
`meet(String, Number)` is ⊥ for a caller passing a string to a declared-numeric
parameter, and `meet(String, Int)` is ⊥ inside the `(%int? x)` branch of a
function only ever called with a string. Both are left alone, so those sites
reject rather than compute.

## A written binding's result is Top

A binding whose initializer is a lambda records that lambda's body type, and
every call to the binding reads it as a proof. An `assign` puts a different
lambda in the binding, and no pass can say which one a given call reaches: the
write can sit inside a function an earlier call runs.

So the binder records **Top** for a binding this unit writes anywhere, whatever
its initializer computes. The rule is asked once, where the body type is
recorded, so no route to the write defeats it. Functionalization rewrites a
write to a mutated binding into `MakeCell`/`SetCell`, whose arm records no body
type at all; the `Assign` arm records one for a binding that rewrite left alone.

Which binding a call names is functionalization's answer rather than the
source's. A straight-line write to a function-local binding is SSA-renamed:
each version has one initializer, and the call names the version the write
made. No version is written there, so that spelling keeps its proof and answers
from the lambda the program last put in the name.

What renaming cannot reach is a binding a **cell** holds — file scope, a loop,
a capture, a write from inside another function — and that is the shape this
rule covers. A version a branch merges is neither: its initializer is the
merge, so nothing records a body type for it and a call already reads Top.

Recording Top is what the join needs, and recording nothing is not enough. An
absent entry reads as Bottom, and a Bottom no later pass raises joins away at
the first branch it meets — `(if c (f 0) 1)` joins it with Int and hands the
site an Int proof. `settle` never sees it, because the join has already
replaced it (§ "Bottom is not a proof").

```text
(var f (fn [x] 1))
(%bit-and (f 0) 1)
(assign f (fn [x] "s"))
(%bit-and (f 0) 1)
```

That is a compile error at both call sites, the one above the write included.
The pass has no flow, so it cannot order the write against a call.

## What still does not prove

- **A mutated binding.** An `assign` gives a binding flow that a per-pass
  recomputation cannot see, so a mutated parameter never receives call-site
  proofs and keeps whatever a guard proves of it. The loop-accumulator spelling
  of a numeric kernel is unproven for this reason, where the recursive spelling
  of the same kernel proves. A mutated lambda binding's result answers the same
  way, and for the same reason (§ "A written binding's result is Top").
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
- `hir::typeinfer::tests::bottom::*` — Bottom is not a proof: one rejection per
  contract row that asks a subtype question, a guard and a `(numeric!)`
  declaration each refining the Kleene start and each meeting a fact that
  contradicts it, and the postcondition that the map the pass hands out carries
  no Bottom.
- `hir::typeinfer::tests::mutation::*` — a written lambda binding proves nothing
  about its result: the write after the call and the write before it, the write
  reached from inside another function, a write that stores the same type, the
  loop that rewrites its own callee between two turns, the branch a Bottom would
  have joined away, and two controls — the unwritten binding, and the
  SSA-renamed function-local write that still proves.
- `tests/elle/typed-int-ops.lisp` — the corpus peer, on every tier: a
  self-recursive integer `fib` emits `AddInt` and computes with it, a float
  base case is refused at compile time, and a reassigned local lambda binding
  is refused where its unreassigned twin emits `AddInt`.
