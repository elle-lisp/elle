(elle/epoch 11)
## tests/elle/value-closure.lisp
## Closure signal behavior, squelch

# Basic closure creation and calling
(let [f (fn [] 42)]
  (assert (= (f) 42) "basic closure"))

# Closures capture environment
(let [x 10]
  (let [f (fn [] x)]
    (assert (= (f) 10) "closure captures")))

(println "value-closure: all tests passed")
