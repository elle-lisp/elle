(elle/epoch 14)
# audited: 2026-10-06
# A query the VM answers with a value from outside the call's own region strands nothing.
# docs/impl/region/ctx.md
#
# A native that asks the VM a question (`jit?`, `git`, `fiber/self`) builds the
# question as a pair in the call's own result region, and the VM's answer is
# the call's result. A fresh answer lives in that region too, so the caller's
# release of the result frees the question with it. An immediate answer, or one
# that lives elsewhere — `jit?`'s boolean — leaves the region's birth
# reference with no consumer, so the dispatch releases it.
#
# The counter-factual: without that release every `(jit? f)` strands one region
# holding a pair that names `f`, and this loop counts one region per call.

(defn f [x]
  x)

(defn churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (< i n)
    (jit? f)
    (assign i (+ i 1)))
  (- (arena/region-count) before))

# The answer must still be right: a probe that stopped reaching the dispatch
# would measure 0 for the wrong reason.
(assert (boolean? (jit? f)) "jit? answers a boolean")

(let [d100 (churn 100)
      d1000 (churn 1000)]
  (assert (< d100 20) (string "a query's question leaks at n=100: delta=" d100))
  (assert (< d1000 20)
          (string "a query's question leaks at n=1000: delta=" d1000)))
