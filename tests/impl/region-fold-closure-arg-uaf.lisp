(elle/epoch 13)
# audited: 2026-09-29
# A stdlib closure passed as a tail-call argument, deep in a fold driven
# thousands of times, keeps its region alive.
#
# docs/impl/region/rules.md
#
# `+` is a compile-time constant: the lowerer loads it with `LoadConst` and
# never captures it, so the frame owns no reference to it. A tail call is a
# move into an owned-param callee, so the caller must hand that callee a fresh
# reference of its own (`arg_leaf_is_borrowed`, src/lir/lower/control.rs).
# Moving the constant instead lets each callee release drain `+`'s region by
# one per iteration, and a later read faults under `--trace=guardfree`.
# region-const-tail-move-borrow-uaf.lisp holds the minimal shapes; this file is
# the deep-churn witness.
#
# The counter-factual that looked right: the recursion is not the mechanism. A
# fold that runs zero iterations drains the region just as fast, because the
# move is the driver thunk's own tail call `(fold-threaded + 0 [1 2 3])`.
#
# The trap: the fault fires only once a region id recycles onto the freed one.
# A short loop does not get there, and neither does a loop that compares the
# fold's result, since the comparison changes the allocation sequence. The
# discard-the-result drive below reaches the collision deterministically at
# about 8000 repetitions, with `->array` churning region ids on every call. A
# small run proves nothing here.

(defn go-threaded [f arr n i acc]
  (if (%lt i n)
    (go-threaded f arr n (%add i 1) (f acc (get arr i)))
    acc))
(defn fold-threaded [f init coll]
  (let [arr (->array coll)
        n (length arr)]
    (go-threaded f arr n 0 init)))
(defn drive [thunk reps]
  (def @c 0)
  (while (%lt c reps)
    (thunk)
    (assign c (%add c 1))))

# `+` is a compile-time constant (a stdlib-export closure); the thunk tail-
# passes it into `fold-threaded` per iteration. The result is discarded on
# purpose — comparing it changes the allocation sequence and hides the fault
# (this is a UAF guard, not a value test).
(drive (fn [] (fold-threaded + 0 [1 2 3])) 8000)
(println "region-fold-closure-arg-uaf: ok")
