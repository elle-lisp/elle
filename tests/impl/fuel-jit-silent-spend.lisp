(elle/epoch 15)
# audited: 2026-10-12
# A compiled silent function spends a fiber's fuel and never pauses; the pause comes after it.
# docs/signals/inference.md
#
# The sidecar makes the JIT eager and compiles on the VM thread, so `spin` runs
# as compiled code from its first call after the warm-up. `spin` is silent:
# `numeric!` proves its operands, and nothing in it can signal.
#
# The counter-factual: a silent function that charges nothing leaves the budget
# where its caller's call left it, so the call after it charges and the fiber
# finishes without a pause. A silent function that pauses inside itself parks a
# frame its caller has no site to park behind.

(defn spin [n]
  (numeric!)
  (def @i 0)
  (while (%lt i n) (assign i (%add i 1)))
  i)

(spin 3)
(jit/rejections)
(assert (jit? spin) "the silent loop is compiled")
(assert (silent? spin) "the loop is silent")

## A body that is silent itself: the loop spends the budget, and nothing pauses.
(let [f (fiber/new (fn [] (spin 1000)) |:fuel|)]
  (fiber/set-fuel f 5)
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "nothing pauses a silent body")
  (assert (= (fiber/value f) 1000) "the loop ran to its end")
  (assert (= (fiber/fuel f) 0) "the compiled loop spent the budget"))

## A body that is not silent: a call through a mutable binding is a charge site
## after the loop, and it finds the budget spent.
(def @after identity)
(let [f (fiber/new (fn [] (after (spin 1000))) |:fuel|)]
  (fiber/set-fuel f 5)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "the call after the loop pauses")
  (assert (nil? (fiber/value f)) "the pause carries a nil payload")
  (assert (= (fiber/fuel f) 0) "the loop spent what the body's call left")
  (fiber/set-fuel f 5)
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "the refueled fiber finishes")
  (assert (= (fiber/value f) 1000) "with the loop's value"))

(println "fuel-jit-silent-spend: ok")
