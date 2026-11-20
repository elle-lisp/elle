# Grafting a call: the operands

<!-- audited: 2026-11-20 -->

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
- a `Var` naming a written binding that no closure writes and the lambda
  around the call does not capture, when no operand after it writes the
  binding. A closed body holds no `assign` and no lambda, so only a closure
  that writes the binding, or a later operand, could write it between the call
  and a read. A closure writes a binding when an `assign` to it sits in a
  lambda nested in the binding's scope, which the analyzer records on the
  binding. A closure that captures the binding and only reads it cannot change
  it, so holding a written binding is not writing it.

Inside a lambda, an operand naming a binding the lambda captures and the unit
writes is bound by a let first. The function that owns the binding can write it while the lambda's code waits
in a call or an `emit`. A call can resume the fiber where the owner is parked,
and the owner runs while the lambda's own fiber is parked. Such a binding lives
in a cell, so the `let` reads the cell once where the body would read it at
every use. The fold decides the same guards over the local, which has the
binding's type.

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

# A closure that only reads the counter cannot change it, so the counter is
# still written in place. It lives in a cell, and each op reads the cell itself.
(def read (fhir "(defn w [b] (var i 0) (def show (fn [] i))
                   (while (< i b) (assign i (+ i 1))) (show)) (w 5)"))
(assert (< 0 (occurrences read "(%lt (deref-cell i#")) "the comparison reads the cell")
(assert (< 0 (occurrences read "(%add (deref-cell i#")) "and so does the increment")

# A closure that writes the binding may do so from inside the body, so the
# operand keeps the value it had at the call.
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

# Inside a lambda, a later operand may resume the owner of a captured binding,
# and the owner writes it.
(defn pair2 [x y] [x y])
(defn first-owner [@lo]
  (assign lo 0)
  (yield (fn [d] (pair2 lo (d))))
  (assign lo 100)
  (yield nil))
(def first-parked (fiber/new (fn [] (first-owner 1)) |:yield|))
(def first-reader (fiber/resume first-parked))
(assert (= (first-reader (fn [] (fiber/resume first-parked) 2)) [0 2])
        "x is the 0 lo held before the second operand resumed the owner")

# The grafted body may resume the owner too, before it reads the parameter.
(var second-parked nil)
(defn kick [] (fiber/resume second-parked) 2)
(defn after-kick [x] (kick) x)
(defn second-owner [@lo]
  (assign lo 0)
  (yield (fn [] (after-kick lo)))
  (assign lo 100)
  (yield nil))
(assign second-parked (fiber/new (fn [] (second-owner 1)) |:yield|))
(assert (= ((fiber/resume second-parked)) 0)
        "x is what lo held at the call, not what the owner wrote")

# The guards of a grafted + then test the local, and the %add reads the local.
(def summed (fhir "(defn s [@lo] (assign lo 1) (fn [d] (+ lo (d))))
                   ((s 0) (fn [] 2))"))
(assert (= 1 (occurrences summed "(deref-cell lo#")) "the let reads the cell once")
(defn summer [@lo] (assign lo 1) (fn [d] (+ lo (d))))
(assert (= ((summer 0) (fn [] 2)) 3) "the wrapper over a captured, written binding")
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
  parameter are written in place, and so is a written local a closure captures
  and only reads, while a binding a closure writes, and one a later operand
  writes, keep the value they held at the call. Inside a lambda, a captured
  binding the unit writes is bound by a `let`, so it keeps the value it held
  at the call when a later operand or the body resumes its owner.
- [the call operands test](../../../tests/lang/call-operands.lisp), a language
  test — a callee receives the value each operand held at the call, whatever
  runs before the body reads it, and a wrapper over a captured binding the
  owner writes compiles.
- `hir::analyze::tests::bindings::*` — an `assign` inside a nested lambda
  records that a closure writes the binding, and an `assign` in the binding's
  own function does not.
- `hir::typeinfer::tests::graft::*` — a `(numeric!)` parameter keeps its floor
  in the graft, three operands keep an arithmetic call and graft a comparison
  to one spliced call, and a spliced operand keeps the call.
- [the wrapper corpus test](../../../tests/impl/wrapper-lowering.lisp) — a
  counting loop over the wrappers disassembles to the bytecode of the same
  loop over the intrinsics.
- [the loop-pages producer](../../../tests/impl/region-loop-pages.lisp) — the
  time the wrapper loop takes against the intrinsic one, which its ledger pins
  at 1.
