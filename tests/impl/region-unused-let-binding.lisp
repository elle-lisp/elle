(elle/epoch 12)
# Counterfactual for the unused-let-binding Call-result leak.
#
# An *unused* let binding whose init is a heap-allocating Call (e.g.
# `(string ...)`) has its region `decref_point` at the init's own HirId
# (the binding is dead the moment it is bound). `lower_expr` emits the
# region's `DecrefValueRegion` at that point by reloading the binding's
# slot — but `lower_let` stores the init value into the slot *after*
# `lower_expr` returns. So the decref ran against the slot's stamped `nil`,
# and the actual value's region was never released: one leaked region per
# evaluation of the let.
#
# A correct runtime frees the dead binding's region each time, so region
# growth across many evaluations must stay bounded and must NOT scale with
# the iteration count.

# Non-tail: the leak is general, not specific to tail calls.
(defn discard-string (n)
  (let [s (string "x-" n)]
    42))

(defn churn-unused (iters)
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i iters)
    (discard-string i)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d100 (churn-unused 100)
      d1000 (churn-unused 1000)]
  (assert (%lt d100 20)
          (concat "unused let-binding region leak at n=100: delta="
                  (number->string d100)))
  (assert (%lt d1000 20)
          (concat "unused let-binding region leak at n=1000: delta="
                  (number->string d1000))))
