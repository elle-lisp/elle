(elle/epoch 12)
# audited: 2026-09-23
# A self-referential reassignment of a top-level mutable survives when the file runs as a whole-module thunk.
# docs/impl/region/bindings.md
#
# A TOP-LEVEL mutable binding (`def @x …`) is reassigned to a value that
# references its OLD content (`(assign x (pair v x))`). The `elle test` runner
# ships a file to a worker as the body of the `%file-body` whole-module THUNK
# (`compile/whole-module-syntax`, src/pipeline/compile.rs). A direct `elle FILE`
# run is the OUTERMOST code object (`in_lambda = false`,
# region-mutable-reassign-selfref.lisp); the thunk runs `in_lambda = true`.
#
# THE TRAP. `record_top_level_reassign` (src/hir/region/infer/container.rs)
# classifies a file-letrec binding by `is_file_scope`, not by the raw
# `in_lambda` flag. Classified by `in_lambda`, the binding lands in
# `local_reassigns` inside the thunk. The fn-local path then keeps the
# assign-value release, which fires at the assign and frees the just-stored
# value while the cell still holds it. Under guardfree the free site is the
# `DecrefValueRegion` at the `(assign …)`.
#
# Driven here through `compile/whole-module-syntax` so the `%file-body` thunk
# shape runs on the MAIN thread (where --trace works), not only via the
# `elle test` worker.

(defn run-thunk [src]
  (let [forms (compile/read-forms src "<m>")
        thunk (get (get (compile/whole-module-syntax forms "<m>") 0) 1)]
    (thunk)))

# (a) the minimal shape: a single self-referential reassignment, read after.
(def src-single "(def @x (list)) (assign x (pair 1 x)) (reverse x)")
(assert (= (run-thunk src-single) (list 1))
        "thunk: single self-ref reassign of a top-level mutable, read after")

# (b) accumulate across an `each` loop into a top-level mutable, reverse and
# read after.
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
