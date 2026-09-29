(elle/epoch 12)
# audited: 2026-09-29
# A closure captures its environment, and squelch narrows the signals it may raise.
# docs/functions.md
#

# Basic closure creation and calling
(let [f (fn [] 42)]
  (assert (= (f) 42) "basic closure"))

# Closures capture environment
(let [x 10]
  (let [f (fn [] x)]
    (assert (= (f) 10) "closure captures")))

(println "value-closure: all tests passed")
