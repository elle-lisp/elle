# Type inference: the ascent, and what a call proves

<!-- audited: 2026-10-21 -->

Where the types come from: an ascent from below whose limit is the least
fixpoint, and what each kind of call contributes to it.

[The intrinsics reference](../intrinsics.md) says what each `%`-intrinsic contract needs.
This file says where the types that discharge it are computed, and what happens
when the computation does not settle. The pass is
[type inference](../../src/hir/typeinfer.rs).

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

A call the graft replaced with the callee's body still counts as a call site.
The graft records the operand it bound to each parameter, and the join reads
that record as it reads a call's operand ([grafting a call](graft.md)).

A pass re-derives; it never accumulates. Each parameter's type is **replaced**
at pass end by that pass's complete join over the call sites, and each lambda's
body type is replaced by what its body computed on that pass. A join that only
grows can never come back down, so one early reading of an unfinished estimate
would hold every later pass at Top.

This is Kleene iteration: every pass reads strictly more than the last, the
estimate rises, and the limit of the ascent is the least fixpoint.

## What a call contributes

A call contributes to its callee's parameters first. Each operand joins into
the parameter in its position. A parameter the call gives no operand, an `&opt`
one, receives nil. The rest parameter receives the list of the operands past the
fixed ones, which is the empty list or a pair. A spliced operand spreads into
positions nobody can count, so every parameter from its position on receives
unknown.

```lisp
(defn rejected? [src]
  "Whether the file front end rejects src."
  (not (first (protect (compile/whole-module src "<doc>")))))

(assert (rejected? "(defn f [a & more] (%bit-and more 1)) (f 1 2)")
        "more is the list of the extra operands, not the int among them")
(assert (rejected? "(defn f [a &opt b] (%bit-and b 1)) (f 1 2) (f 1)")
        "the second call leaves b nil")
(assert (rejected? "(defn f [a b] (%bit-and b 1)) (f ;[1 \"s\"] 2)")
        "b is the string the splice spreads, not the 2 written after it")
```

A row below that names a primitive or a stdlib function reads the binding the
runtime supplies under that name. A parameter or a local with the same name is
an unknown function, and its call's type is Top:

```lisp
(assert (rejected? "(defn h [length s] (%bit-and (length s) 1))
                    (h (fn [s] \"x\") \"abc\")")
        "a parameter named length is not the primitive")
```

| Callee | The call's type |
|---|---|
| a lambda binding this unit writes | Top (§ "A written binding's result is Top") |
| a lambda binding this unit defines and never writes | that lambda's body type, as the previous pass left it |
| `thaw` or `freeze`, or its `%` twin | the argument's mutable or immutable twin container type |
| a registered primitive | its declared `RetType`, read from the primitive tables |
| the stdlib `push` or `put` | the first argument's type, which it returns |
| a function the graft replaced with its body | no call is left: the body is typed where it stands ([grafting a call](graft.md)) |
| a stdlib arithmetic wrapper that stays a call, such as `(+ a b c)` | Number — the wrapper raises on everything else |
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

The proof the second pass adds is what the backends spend. An `%add` of two
proven ints carries `:int` into LIR, which is `AddInt` on the bytecode tier and
a tag-check-free fast path on the JIT ([LIR](lir.md)).

The wrapper spelling settles the same way. Every wrapper call in `fib-w` is
grafted, so `(- n 1)` is the body of `-`, whose `%sub` over an int `n` and a
literal is an int. The recursive call passes an int and `n` stays one. The fold
then deletes every guard over the proven operands, and each wrapper call ends
as its op ([grafting a call](graft.md)):

```lisp
(defn fib-w [n]
  (if (< n 2) n (+ (fib-w (- n 1)) (fib-w (- n 2)))))
(assert (= (fib-w 10) 55) "the dissolved wrappers compute the recursion")
```

The example compiles `fib-w` again from source to read its LIR. Handing
`fib-w` itself to an inspector would use it as a value, and a binding used as
a value proves nothing about its parameters (§ "What still does not prove"):

```lisp
(def fib-w-lir
  (get (compile/dumps "(defn fib-w [n]
                         (if (< n 2) n (+ (fib-w (- n 1)) (fib-w (- n 2)))))
                       (fib-w 10)" "<doc>") :lir))
(assert (any? (fn [line] (and (string/contains? line " + ")
                              (string/contains? line ":int")))
              (string/split fib-w-lir "\n"))
        "the dissolved + adds two proven ints")
```

## What a branch and a block leave

A guard's fact holds inside the branch it governs. When the branch ends, the
binding takes back the type it had before the fact, unless the branch wrote the
binding. Then the binding takes the join of that type and what the write left,
because the write outlives the branch. A `(type-of x)` arm's override ends the
same way. So the code after a loop reads what the loop body wrote:

```lisp
(defn walk [& xs]
  "Whether a loop of at most ten turns walks xs to its end."
  (def @cur xs)
  (def @n 0)
  (while (and (pair? cur) (< n 10))
    (assign n (+ n 1))
    (assign cur (rest cur)))
  (if (pair? cur) :stopped :walked))
(assert (= (walk 1 2 3) :walked) "cur is the empty list the body wrote")
```

A `block` evaluates to its tail, or to the value of a `break` that names it. So
its type joins the tail's type with the type of every such `break` value. A
`break` leaves no value where it stands, so its own type is Bottom:

```lisp
(defn pick [x]
  "Which kind of value the block hands back."
  (let [v (block :b (when (> x 0) (break :b "s")) 1)]
    (if (int? v) :int :other)))
(assert (= (pick 5) :other) "v joins the string the break carries")
```

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
prove-or-reject rejects that site.

## Bottom is not a proof

Widening treats an unsettled entry, and a settled Bottom needs the same answer
for the same reason. `subtype(⊥, b)` holds for every `b`, so a Bottom operand
discharges every contract row that asks a subtype question. Those rows are
Numbers, DivFamily, Ints, Ordered, and the `%get` index, and the silent opcode
then lowers over a value of any type.

Two different facts wear that one lattice element:

- **No value reaches here.** A function this unit defines and never calls has
  parameters no call site contributes to, so they keep the Kleene start.
- **Nothing has contributed yet.** A `(numeric!)` parameter carries the start
  until a pass walks a call site that refines it.

Neither says what type a value arriving at the site has. So the pass hands out
no Bottom: at the ascent's limit, every Bottom left in the node map settles to
Top, which is above the fixpoint and proves nothing.

That one rule covers every consumer, because they all read that one map. The
consumers are the operand contracts
([the contract check](../../src/hir/typeinfer/contract.rs)), the signal
narrowing ([narrowing](../../src/hir/narrow.rs)), the fold
([grafting a call](graft.md)), and the LIR operand proof
([intrinsic lowering](../../src/lir/lower/expr/intrinsic.rs)). The last asks
with equality, so a Bottom proves nothing there. The first three ask with
`subtype`, where a Bottom would prove every row and decide every type test.

A definition is therefore checked where it is written. `f` below is a compile
error whether or not a later form calls it, and whether or not any form follows
it:

```lisp
(defn compiles? [src]
  "Whether src compiles through the file front end."
  (first (protect (compile/whole-module src "<doc>"))))

(assert (not (compiles? "(defn f [x] (%mul x x))")))
(assert (not (compiles? "(defn f [x] (%mul x x)) 1")))
(assert (compiles? "(defn f [x] (%mul x x)) (f 3)")
        "a call site with an int proves x")
```

### A fact refines the start

A guard and a `(numeric!)` declaration are both proofs about a binding that owe
nothing to a call site. Both reach the environment by meeting with the type the
ascent has accumulated, and `meet(⊥, fact)` is ⊥, so a plain meet with the
start would erase them.

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
the fact are disjoint. The two producers end differently:

- `meet(String, Number)` is ⊥ for a caller passing a string to a
  declared-numeric parameter. The declaration types the binding whatever it
  receives, so the floor gives Number there rather than ⊥. That holds where the
  function is entered and where a graft binds the parameter with a `let`. A
  site that needs more than a Number, such as an int, rejects.
- `meet(String, Int)` is ⊥ inside the `(%int? x)` branch of a function only
  ever called with a string. No value reaches the site the fact governs. The
  fold decides that test false and deletes the branch before the contract is
  checked, so the site is gone rather than rejected
  ([grafting a call](graft.md)).

```lisp
(assert (not (compiles? "(defn sq [x] (numeric!) (%bit-and x 1)) (sq \"str\")"))
        "a string caller leaves x a Number, which proves no int")
(assert (compiles? "(defn sq [x] (numeric!) (%mul x x)) (sq \"str\")")
        "the declaration proves the grafted %mul, as it proves the call's")
(assert (compiles? "(defn g [b] (if (%int? b) (%bit-and b 1) 0)) (g \"str\")")
        "the branch no value reaches is deleted")
```

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
the first branch it meets. `(if c (f 0) 1)` joins it with Int and hands the
site an Int proof. `settle` never sees it, because the join has already
replaced it (§ "Bottom is not a proof").

A call to a written binding is a compile error on either side of the write.
The pass has no flow, so it cannot order the write against a call:

```lisp
(assert (not (compiles? "(var f (fn [x] 1))
                         (%bit-and (f 0) 1)
                         (assign f (fn [x] \"s\"))")))
(assert (not (compiles? "(var f (fn [x] 1))
                         (assign f (fn [x] \"s\"))
                         (%bit-and (f 0) 1)")))
(assert (compiles? "(def f (fn [x] 1)) (%bit-and (f 0) 1)")
        "the unwritten binding proves its result")
```

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
- **A destructured binding.** A file-level `(def [lo hi] …)` binds `lo` and
  `hi` to a `nil` placeholder, and a `destructure` form writes them
  afterwards. The pass does not follow the value into the pattern, so each
  binding the pattern writes is unknown, never the placeholder's Nil. A Nil
  there would let the fold decide `(+ lo hi)`'s number guard certain to raise.

```lisp
(def [lo hi] [1 2])
(assert (= (+ lo hi) 3) "lo and hi are what the pattern wrote")
```

## Pinning tests

- `hir::typeinfer::tests::recursion::self_recursive_call_result_is_the_body_type`
  — the `fib` ascent: the `%add` over two self-calls is Int, not Bottom.
- `…::a_self_recursive_result_proves_a_callee_parameter` — the same result
  carried one call further, into the parameters of a helper that adds it.
- `…::a_float_base_case_does_not_prove_an_int_recursive_result` — the
  over-narrowing guard, and the soundness half. A self-call read as Bottom on
  every pass would type the `%add` of a float base case Int, and let a bitwise
  op read a float's payload.
- `…::a_three_member_recursion_proves_through_the_cycle` and
  `…::a_cycle_whose_members_disagree_proves_nothing` — mutual recursion, which
  converges by the same iteration and must keep doing so.
- `…::a_chain_deeper_than_the_budget_proves_nothing` — the widening: eleven
  functions in reverse walk order outrun the ten passes, and the site that
  reads the unsettled entry is rejected rather than compiled.
- `hir::typeinfer::tests::bottom::*` — Bottom is not a proof. The module holds
  one rejection per contract row that asks a subtype question. A guard and a
  `(numeric!)` declaration each refine the Kleene start, and each meets a fact
  that contradicts it. The postcondition: the map the pass hands out carries no
  Bottom.
- `hir::typeinfer::tests::mutation::*` — a written lambda binding proves nothing
  about its result. The module covers the write after the call and the write
  before it, the write reached from inside another function, and a write that
  stores the same type. It covers the loop that rewrites its own callee between
  two turns, and the branch a Bottom would have joined away. Two controls
  prove: the unwritten binding, and the SSA-renamed function-local write.
- `hir::typeinfer::tests::destructure::*` — a read of a destructured binding,
  from a `def` and from a `var`, is never typed by its `nil` placeholder.
- `hir::typeinfer::tests::branch::*` — a write inside a guarded branch or a
  `(type-of x)` arm outlives the branch, and a block's type joins the values
  of the `break`s that name it.
- `hir::typeinfer::tests::graft::*` and `hir::typeinfer::tests::fold::*` —
  what the graft replaces and what the fold decides, pinned in
  [grafting a call](graft.md).
- [typed-int-ops.lisp](../../tests/impl/typed-int-ops.lisp) — the implementation-suite peer: a
  self-recursive integer `fib` emits `AddInt` and computes with it, in the
  intrinsic spelling and in the wrapper spelling. A float base case is refused
  at compile time, and a reassigned local lambda binding is refused where its
  unreassigned twin emits `AddInt`.
