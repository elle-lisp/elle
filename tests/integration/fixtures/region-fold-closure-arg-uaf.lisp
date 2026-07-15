(elle/epoch 12)
# tests/integration/fixtures/region-fold-closure-arg-uaf.lisp
#
# The fold-shaped, deep-churn witness of the CONST tail-call-arg borrow — the
# hole where a compile-time-constant heap value (here the stdlib closure `+`,
# read as `LoadConst` from `immutable_values`, never captured) is passed as a
# TAIL-CALL argument and pure-moved into an owned-param callee. The frame owns
# no reference to a constant, so the callee's release drained `+`'s region rc
# by one per drive iteration to a premature free; the next read of the page
# faulted:
#
#     [guardfree] SIGSEGV — use-after-free
#     free site: DecrefValueRegion of closure (runtime region N)
#     context:   UpdateCapture
#
# GREEN since `arg_leaf_is_borrowed` (src/lir/lower/control.rs) treats a
# constant HEAP value as borrowed and hands the callee one fresh owning
# reference. The minimal shapes and the balance guard live in
# tests/elle/region-const-tail-move-borrow-uaf.lisp; this fixture is kept as
# the deep-churn regression witness (exercised by the guardfree subprocess pin
# `region_fold_closure_arg_uaf` in tests/integration/elle_scripts.rs).
#
# DIAGNOSIS HISTORY — kept because the mis-framing cost real work. This was
# long filed as a closure-LIFETIME gap: "a combiner THREADED as a recursive
# argument (or held in a `def @` cell) is over-freed mid-fold", and
# src/core.lisp `fold` was shaped around it (the letrec-capture form). The
# recursion was never the mechanism — a ZERO-iteration `go-threaded` drains
# exactly the same 1/call — and the letrec-capture form was "clean" only
# because its drive passed a FRESH lambda instead of a stdlib constant. The
# hole was the driver thunk's own tail call `(fold-threaded + 0 [1 2 3])`
# moving the constant `+`. Decompose before attributing: vary ONE ingredient
# at a time (here, iteration count zero was the decisive control).
#
# It is STATE-DEPENDENT: the fault fires only once region ids recycle onto the
# freed one, so a short loop — or one that COMPARES the fold result — looks
# clean (either changes the allocation sequence). The discard-the-result drive
# below reaches the collision deterministically (~8000 reps; per-call
# `->array` churns region ids). Do NOT "verify" a change here with a small run.

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
