(elle/epoch 12)
# Counterfactual: a TOP-LEVEL mutable binding (`def @x …`) reassigned to a value
# that references its OLD content (`(assign x (pair v x))`) is freed too early
# when the file's forms run as the body of the `%file-body` whole-module THUNK —
# the shape the `elle test` runner ships to a worker (compile/whole-module-syntax,
# src/pipeline/compile.rs). A direct `elle FILE` run is the OUTERMOST code object
# (`in_lambda = false`) and is correct (region-mutable-reassign-selfref.lisp); the
# same source wrapped in `(fn () (%file-body …))` runs `in_lambda = true`.
#
# Root cause (dumped LIR + --trace=rc/guardfree, not a guess): in the solver
# (`record_top_level_reassign`, src/hir/regions.rs) a reassigned top-level mutable
# was classified by the raw `in_lambda` flag, so inside the thunk it landed in
# `local_reassigns` (fn-local) instead of `top_level_reassigns` (module-scope).
# The file-letrec lifts each statement into a dead `__file_expr_N` wrapper; the
# fn-local path keeps the assign-value decref, which the wrapper routes through
# the binding slot and fires at the assign — freeing the just-stored value while
# the cell still holds it:
#   [guardfree] free site: DecrefValueRegion of <list> @ <the (assign …)>
#   (plain build: arena.rs tag/object mismatch — capture-cell read as a freed slot)
#
# The fix marks file-letrec bindings `is_file_scope` so the solver classifies them
# module-scope regardless of the synthetic thunk wrapper — the final value is freed
# by the file-letrec scope-region teardown, identical to a direct run.
#
# Driven here through `compile/whole-module-syntax` so the failing `%file-body`
# thunk shape is exercised on the MAIN thread (where --trace works), not only via
# the `elle test` worker. RED before the fix; GREEN after.

(defn run-thunk [src]
  (let [forms (compile/read-forms src "<m>")
        thunk (get (get (compile/whole-module-syntax forms "<m>") 0) 1)]
    (thunk)))

# (a) the minimal shape: a single self-referential reassignment, read after.
(def src-single "(def @x (list)) (assign x (pair 1 x)) (reverse x)")
(assert (= (run-thunk src-single) (list 1))
        "thunk: single self-ref reassign of a top-level mutable, read after")

# (b) the advanced.lisp shape: accumulate across a loop into a top-level mutable,
# reverse and read after — the exact `decision tree match in loop` form.
(def src-each
  (concat "(def @acc (list)) "
          "(each i (list 1 2 3) (assign acc (pair i acc))) " "(reverse acc)"))
(assert (= (run-thunk src-each) (list 1 2 3))
        "thunk: self-referential each-accumulation into a top-level mutable")

# (c) a `while` loop variant (no `each` macro), same accumulation shape.
(def src-while
  (concat "(def @acc (list)) (def @i 0) "
          "(while (%lt i 3) (assign acc (pair i acc)) (assign i (%add i 1))) "
          "(reverse acc)"))
(assert (= (run-thunk src-while) (list 0 1 2))
        "thunk: while-loop self-referential accumulation into a top-level mutable")

(println "region-toplevel-reassign-thunk-uaf: ok")
