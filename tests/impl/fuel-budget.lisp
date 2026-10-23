(elle/epoch 15)
# audited: 2026-10-22
# A fiber's budget holds any non-negative integer, and a fiber with no budget reads nil however long it runs.
# docs/runtime.md
#
# The budget is one 64-bit word, and a fiber with no budget holds 2^63 + 2^62
# there, which no run counts down to the largest integer
# (docs/impl/jit-fuel.md).
#
# The counter-factual: a budget kept in 32 bits reads 2^32 back as 0, and a
# fiber given it pauses at its first charge.

(def two-32 4294967296)
(def largest 9223372036854775807)

(defn spin [i n]
  (if (%int? i)
    (if (%int? n)
      (if (%lt i n) (spin (%add i 1) n) i)
      n)
    i))

(let [f (fiber/new (fn [] 1) |:fuel|)]
  (assert (nil? (fiber/fuel f)) "a new fiber has no budget"))

(let [f (fiber/new (fn [] (spin 0 1000)) |:fuel|)]
  (fiber/set-fuel f two-32)
  (assert (= (fiber/fuel f) two-32) "a budget past 32 bits reads back whole")
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "a budget past 32 bits does not pause")
  (assert (< (- two-32 1100) (fiber/fuel f) two-32)
          "the run spends its budget from the whole count"))

(let [f (fiber/new (fn [] (spin 0 1000)) |:fuel|)]
  (fiber/set-fuel f largest)
  (assert (= (fiber/fuel f) largest) "the largest integer reads back")
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "the largest budget runs")
  (assert (< (- largest 1100) (fiber/fuel f) largest)
          "the largest budget counts down"))

(let [f (fiber/new (fn [] 1) |:fuel|)
      [ok? err] (protect (fiber/set-fuel f -1))]
  (assert (not ok?) "a negative budget is refused")
  (assert (= (get err :error) :type-error) "a negative budget is a type error")
  (assert (nil? (fiber/fuel f)) "a refused budget leaves the fiber without one"))

(let [f (fiber/new (fn [] (spin 0 100000)) |:fuel|)]
  (fiber/set-fuel f 10)
  (fiber/clear-fuel f)
  (assert (nil? (fiber/fuel f)) "clear-fuel removes the budget")
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "a fiber without a budget never pauses")
  (assert (nil? (fiber/fuel f)) "a long run leaves a fiber without a budget"))

(println "fuel-budget: ok")
