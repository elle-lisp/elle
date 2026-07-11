(elle/epoch 12)
# tests/integration/fixtures/region-compose-closure-acc-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because a regression SIGSEGVs under
# --trace=guardfree, and `make smoke` globs tests/elle/*.lisp into one shared
# process where a segfault takes the whole harness down. Exercised by the
# guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_compose_closure_acc_uaf`).
#
# WHAT IT PINS — a heap value the tail call of a self-tail-recursive HOF
# transfers forward stays live across the whole recursion; stdlib `compose`/`comp`
# compose correctly.
#
#   src/core.lisp `fold` holds its combiner `f` by CAPTURE (a letrec `go` closing
#   over f/arr/n) and THREADS the accumulator `acc` as a recursive argument:
#   `(go (%add i 1) (f acc (get arr i)))`. `compose`/`comp` fold `identity` with a
#   closure-returning reducer
#   (`(fold (fn (composed f) (fn (& args) (composed (f ;args)))) identity fns)`),
#   so the accumulator is the running composed closure — the reducer captures the
#   prior `acc` into its result, which the tail call then moves forward as the
#   next `acc`. The value must survive that move.
#
#   The hazard the region walk must avoid: its callee inline
#   (`try_inline_call`, whose sole job is to surface a callee body's cross-region
#   EDGES at the call site) binds the callee's params to the CALLER's arg regions
#   and re-walks the body. A `Return` reached inside that re-walk names the arg
#   region, so recording it in `return_sites` would pin the transferred arg's
#   `decref_point` to the callee's base-case (sibling) arm. Under self-tail-call
#   frame reuse the branch-union release then loads that stale slot and frees the
#   reducer result the tail call already handed to the next accumulator — a UAF at
#   the next deref of the composed closure.
#
#   The interprocedural return facet is escape.rs's authority — a param-index
#   summary, not a stateful re-walk — so the region walk records return-frontier
#   `decref_point` extensions only on the STRUCTURAL walk (`inline_depth == 0`),
#   the same gate the Letrec/Let cell mint already uses. The callee's own
#   structural analysis records its returns correctly; the inline duplicate is
#   skipped, and the transferred accumulator is released exactly once (by the
#   base case's `acc` decref), never twice.
#
#   It faults SINGLE-SHOT and deterministically when the gate is absent (the freed
#   region's generation is bumped and the next use reads gen-stale), so no
#   id-recycling drive is needed — `(compose g)` over one function is enough.
#   Reached only when the HOF is called from >= 2 sites in a unit: the second
#   caller's inline re-walk is what pushes the polluting return-site. A distinct
#   surface — a combiner threaded as a recursive ARG reaching rc 0 mid-drive — is
#   pinned separately by region-fold-closure-arg-uaf.

# The user-visible surface: compose/comp are everyday combinators.
(assert (= ((compose (fn [x] (+ x 1))) 5) 6) "compose: single fn")
(assert (= ((compose (fn [x] (* x 2)) (fn [x] (+ x 1))) 3) 8) "compose: two fns")
(assert (= ((comp (fn [x] (+ x 1)) (fn [x] (* x 2))) 3) 7) "comp: alias")

# The isolated mechanism: a single-step fold whose reducer captures the closure
# accumulator into its result. Faults on the first call of the result.
(def c (fold (fn (acc x) (fn () acc)) (fn () 42) [1]))
(assert (= ((c)) 42) "fold: closure accumulator survives the fold")

(println "region-compose-closure-acc-uaf: ok")
