# Type inference: what a guard's fact survives

<!-- audited: 2026-11-18 -->

Where a guard's fact about a binding holds, and the write, loop turn, call or closure that ends it.

[Type inference](../typeinfer.md) owns the ascent that computes the types. This
document owns the facts a guard adds on top of them: the type facts inference
applies, and the nonzero facts the [operand contracts](../../intrinsics.md)
carry. The fold deletes a branch whose test a fact decides, so a fact that
outlives the value it describes deletes code that runs.

## A fact holds in the branch it governs

A guard's fact holds inside the branch it governs: `(nil? x)` makes `x` nil in
the then-branch, and `(%int? n)` makes `n` an int. A guard's fact meets the
binding's type, and a `(type-of x)` arm overrides it. When the branch ends, the
binding takes back the type it had before the fact, unless the branch wrote the
binding. Then the binding takes the join of that type and what the write left,
because the write outlives the branch. So the code after a loop reads what the
loop body wrote:

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

A fact holds only while the binding keeps the value the guard tested. A write
in the branch is one way the value changes, and the pass sees it where it
stands. The sections below are the ways the pass cannot see in walk order, and
what the pass does at each one.

## A loop's next turn

A loop body runs again over what its last turn wrote. So the pass walks the
body, joins each binding the body writes with the type it had at the head, and
walks the body again from that join. It stops when a walk leaves the head as it
found it. A fact from outside the loop then governs the first turn alone:

```lisp
(assert (= (let [@found nil]
             (while (nil? found)
               (each m in [1 2 5]
                 (when (nil? found) (assign found m))))
             found)
           1)
        "the second turn reads the 1 the first turn wrote")
```

The lattice has height four, so each binding moves at most three times, and the
walks end. A binding the body introduces is bound again on every turn, so it
carries nothing across the back edge and does not take part in the join.

## A call and a yield

A call can run a closure, and a closure can write a binding of the function
that made it. So after a call, a binding a closure may write loses every fact
in force about it. It takes the join of its type and every type those facts
hid:

```lisp
(assert (= (let [@x nil]
             (def set-x (fn [] (assign x 1)))
             (match (type-of x)
               :nil (begin (set-x) (match (type-of x) :nil :nil _ :int))
               _ :other))
           :int)
        "the call wrote x after the arm tested it")
```

A `yield` ends the same facts, because the fiber that resumes this one runs
first, and it can call such a closure:

```lisp
(assert (= (let [@x nil]
             (def f (fiber/new
                      (fn []
                        (match (type-of x)
                          :nil (begin (yield 1)
                                      (match (type-of x) :nil :nil _ :int))
                          _ :other))
                      |:yield|))
             (fiber/resume f)
             (assign x 1)
             (fiber/resume f))
           :int)
        "the parent wrote x while the fiber was parked")
```

The analyzer marks the binding a closure may write
([the operands](../graft/operands.md) asks the same question). A binding no
closure writes keeps its facts across a call, because only its own function
can write it.

## A closure's body

A lambda's body runs when the lambda is called, which may be after a write or
outside the branch the lambda stands in. So inside a lambda, a binding the unit
writes anywhere reads with no fact that is in force where the lambda stands. A
binding nothing writes keeps its facts, because its value never changes:

```lisp
(assert (= (let [@x nil @hits 0]
             (when (nil? x)
               (def count-nil (fn [] (when (nil? x) (assign hits (+ hits 1)))))
               (count-nil)
               (assign x 1)
               (count-nil))
             hits)
           1)
        "the second call runs after the write")
(defn adder [x] (when (%int? x) (fn [] (%add x 1))))
(assert (= ((adder 5)) 6) "the guard still proves the x nothing writes")
```

A lambda whose body writes such a binding leaves the join of the binding's type
before the lambda and what the body wrote.

## A written parameter

A parameter the function writes enters with a value no call site proves
([type inference](../typeinfer.md)), and an entry the pass holds no type for
reads as unknown. A write joins with that unknown, so the parameter stays
unknown, before the write and after it:

```lisp
(defn fresh? [@x]
  "Whether x was nil before the function wrote it."
  (def was (nil? x))
  (assign x 1)
  was)
(assert (fresh? nil) "the read before the write sees the caller's nil")
```

A write that narrowed the parameter would hold over the whole function on the
next pass, because each pass starts from the environment the last one left.

## Nonzero facts

The div family needs a divisor the walk proves nonzero, and the walk carries
that proof as a fact under the same rules. A write ends it. A loop body starts
with no fact about a binding the body writes. A lambda body starts with no
fact. A call or a yield ends the fact about a binding a closure may write:

```lisp
(defn compiles? [src]
  "Whether src compiles through the file front end."
  (first (protect (compile/whole-module src "<doc>"))))

(assert (not (compiles? "(var d 1)
                         (defn zero! [] (assign d 0))
                         (defn q [] (if (%ne d 0) (begin (zero!) (%div 10 d)) 0))
                         (q)"))
        "the call may have set d to zero")
(assert (compiles? "(defn q [d] (if (%ne d 0) (%div 10 d) 0)) (q 2)")
        "with no call between, the guard proves d")
```

## Pinning tests

- `hir::typeinfer::tests::branch::a_write_in_a_guarded_branch_outlives_the_branch`
  and `…::a_write_in_a_type_of_arm_outlives_the_arm` — a write outlives the
  branch that narrowed its binding.
- `hir::typeinfer::tests::facts::*` — each way a fact ends: a loop turn, a
  call, a yield, a closure body and a written parameter, for type facts and for
  nonzero facts. Their controls keep the proofs a fact still gives: a counter
  a closure writes, an int loop counter, and a guard on a binding nothing
  writes.
- [guard-facts.lisp](../../../tests/lang/guard-facts.lisp) — the language
  suite's peer: each program computes what it computes with no fold.
