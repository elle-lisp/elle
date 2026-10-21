# Grafting a call: the operands

<!-- audited: 2026-10-20 -->

Which operand replaces its parameter in the grafted body, which is bound by a `let` first, and how a rest list is decided.

[Grafting a call](../graft.md) owns the graft itself: which calls graft, where a
body comes from, and what the fold then deletes. This document owns how the
operands reach the body.

```lisp
(defn fhir [src]
  "The functionalized HIR of src, as text."
  (get (compile/dumps src "<doc>") :fhir))

(defn occurrences [text part]
  "How many times part occurs in text."
  (- (length (string/split text part)) 1))
```

## Operands and parameters

The graft binds each parameter to its operand in the order the operands are
written, so the evaluation order of the call is kept. An operand replaces the
parameter directly in the body when it holds one value from the call to the
last read the body makes of it:

- an immediate literal;
- a `Var` naming a binding nothing writes. The test is that nothing writes it,
  not that it is immutable, because a `match` pattern's variable is never
  marked immutable;
- a `Var` naming a written binding that no closure captures, when no operand
  after it writes the binding. A closed body holds no `assign` and no lambda,
  so only a closure that captured the binding, or a later operand, could write
  it between the call and a read.

Every other operand is bound by a `let` to a fresh local first. The direct
replacement matters for the fold: a guard in the grafted body then tests the
caller's own binding, and its fact narrows that binding. It matters for a loop
too: a counter written in place reaches the op as itself, where a `let` costs
a store and a load on every iteration.

```lisp
# A loop counter is a written binding no closure captures, so each op reads it
# as itself, and the loop compiles to what its intrinsic spelling compiles to.
(def counted (fhir "(defn w [b] (var i 0) (while (< i b) (assign i (+ i 1))) i) (w 5)"))
(assert (< 0 (occurrences counted "(%lt i#")) "the comparison reads the counter itself")
(assert (< 0 (occurrences counted "(%add i#")) "and so does the increment")

# A closure that captured the binding may write it from inside the body, so
# the operand keeps the value it had at the call.
(var i 0)
(defn bump [] (assign i (%add i 1)))
(defn after-bump [x] (bump) x)
(assert (= (after-bump i) 0) "x is what i held at the call, not what bump left")

# A later operand may write it too.
(defn add2 [x y] (%add x y))
(defn later []
  (var k 1)
  (add2 k (begin (assign k 10) 1)))
(assert (= (later) 2) "k is read before the second operand writes it")
```

A parameter a `(numeric!)` declaration floors is always bound by a `let`. The
floor is a fact about the parameter's binding, and the `let` carries that
binding into the caller, so the body's `%`-ops prove against the floor as they
did inside the function. Replacing the parameter with an unproven operand would
drop the floor and reject a program that compiles today.

## The rest list and the known arity

A rest parameter receives the operands past the fixed ones. The count is known
at the call, so a body that dispatches on the rest list decides the dispatch at
graft time. The body must be a `match` on the rest parameter. Its arms are tried
in order against the known shape of the list:

- `()` matches no extra operands;
- a list pattern of plain variables or `_`, as `(b)` or `(a b)`, matches that
  many, and a `& r` tail matches that many or more;
- `_` or a bare variable matches any count.

The first arm that matches is the body the graft uses, and its pattern variables
bind to the operands under the rule above. A tail or a bare variable binds to a
`(list …)` of the operands it covers. An arm with a literal element, a nested
pattern or a guard cannot be decided at graft time, so reaching one declines the
graft. A body that is not such a `match` binds the rest parameter to the
`(list …)` of the extra operands, `()` when there are none.

Each arm closes on its own. An arm that does not close, such as the general arm
of `+` with its `letrec` walker, leaves the calls of its arity as calls, while
the other arities still graft.

## Pinning tests

- `hir::typeinfer::tests::operand::*` — a written local and a mutated
  parameter no closure captures are written in place, while a captured
  binding, and one a later operand writes, keep the value they held at the
  call.
- `hir::typeinfer::tests::graft::*` — a `(numeric!)` parameter keeps its floor
  in the graft, three operands keep an arithmetic call and graft a comparison
  to one spliced call, and a spliced operand keeps the call.
- [the wrapper corpus test](../../../tests/impl/wrapper-lowering.lisp) — a
  counting loop over the wrappers disassembles to the bytecode of the same
  loop over the intrinsics.
- [the loop-pages producer](../../../tests/impl/region-loop-pages.lisp) — the
  time the wrapper loop takes against the intrinsic one, which its ledger pins
  at 1.
