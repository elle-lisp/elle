(elle/epoch 15)
# audited: 2026-10-15
# A pair walked by a grafted helper stays live for as long as the name its walk is bound to.
# docs/impl/region/rules.md
# docs/impl/graft.md
#
# The sidecar arms guardfree and keeps the JIT off, so a stale read faults at
# its deref. The graft stays on: it is what turns each call to `step` into the
# helper's body.
#
# `step` proves its operand a pair and returns `(%rest p)`. Grafted, each call
# becomes `(let [p (begin nil (%rest arg))] …)`, a borrowing read wrapped in a
# statement and bound to the helper's parameter, and nested calls chain one
# borrow out of the next. Nothing counts the borrowed tail, so the walked
# list's region must outlive `t`, the name the last call's value is bound to.
#
# The counter-factual: with the bound names holding no region, the list is
# released once the innermost call's value is bound, and reading `t` derefs a
# freed page.

(defn step [p]
  (when (%not (%pair? p))
    (error {:error :type-error :message "step: pair expected"}))
  (%rest p))

(defn walk3 [i]
  (let [xs (%pair i (%pair i (%pair i (%pair (string "s" i) ()))))]
    (let [t (step (step (step xs)))]
      (when (%not (%pair? t))
        (error {:error :type-error :message "walk3: pair expected"}))
      (length (%first t)))))

(defn churn [n]
  (def @i 0)
  (while (< i n)
    (assert (= (walk3 i) (length (string "s" i))) "walk3 reads the walked tail")
    (assign i (+ i 1))))

# Churn region ids so a freed page is recycled onto a live region.
(churn 500)

(def r0 (arena/region-count))
(churn 500)
(def growth (- (arena/region-count) r0))
(assert (< growth 50)
        (string "grafted walk leaked " growth " regions over 500 ops"))

(println "region-borrow-chain-graft-uaf: ok")
