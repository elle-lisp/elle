(elle/epoch 13)
# audited: 2026-09-29
# A match guard that answers differently when the phi select runs it again keeps the pre-match value.
# docs/impl/hir.md
#
# Functionalization lowers a match that assigns in its arms to a second match
# that selects each assigned binding's version, and that second match runs the
# guards again. An impure guard that answers differently the second time falls
# to a synthetic catch-all that keeps the pre-match value, rather than raising
# :match-error. An implementation that runs each guard once takes the arm,
# assigns 1, and is just as correct.

(def phi-box @[true])
(defn phi-pred ()
  (get phi-box 0))
(let [@a 0]
  (match 5
    x when
    (phi-pred) (do
                 (put phi-box 0 false)
                 (assign a 1)))
  (assert (= a 0) "match phi: divergent impure guard falls back to pre-value"))

(println "branch-phi-guard-reeval: ok")
