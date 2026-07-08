(elle/epoch 12)
# A top-level compile-time literal belongs to the program's lifetime: it is held
# in a reclaimable program-extent region that the termination sweep frees. It is
# an ordinary allocation, exactly like a literal inside `eval`
# (docs/impl/region/model.md § "Constants lower as ordinary allocations") — the
# file body is just compile-time happening once, the same machinery the repeated
# `eval` in region-eval-leak.lisp exercises.
#
# WITNESS: `(arena/region-of "<literal>")` for a top-level string literal is a
# real reclaimable region (id >= 2), not an immediate (id 0).

(def lit "toplevel-literal-xyz")
(def r (arena/region-of lit))
(println "region-of top-level literal =" r)

(assert (%ge r 2)
        (concat "top-level compile-time literal must live in a real reclaimable "
                "region (got id " (number->string r) ")"))

(println "region-toplevel-literal: ok")
