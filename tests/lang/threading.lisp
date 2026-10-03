(elle/epoch 14)
# audited: 2026-09-30
# -> threads a value in as each step's first argument, and ->> as its last.
# docs/functions.md
#
# The counter-factual: every step below is `-`, which does not commute. A ->
# that threaded last, or a ->> that threaded first, gives a different answer.
# A test written with `+` passes either way.

(assert (= (-> 42) 42) "-> of a value alone is the value")
(assert (= (->> 42) 42) "->> of a value alone is the value")

(assert (= (-> 10
               (- 3)) 7) "-> makes the value the first argument")
(assert (= (->> 10
                (- 3)) -7) "->> makes the value the last argument")

(assert (= (-> 10
               (- 3)
               (- 2)) 5) "-> threads each result into the next step")
(assert (= (->> 10
                (- 3)
                (- 2)) 9) "->> threads each result into the next step")

(assert (= (-> -5
               abs) 5) "-> calls a bare symbol step with the value")

(defn distance-to-ten [x]
  (->> x
       (- 10)))
(assert (= (distance-to-ten 3) 7) "->> expands around a function's parameter")
