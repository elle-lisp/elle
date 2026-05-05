(elle/epoch 10)

# Type inference rewriting correctness test.
# Verifies that stdlib arithmetic/comparison calls are rewritten to
# %-intrinsics when argument types are proven ⊑ Number.

(defn fib [n]
  (if (< n 2)
    n
    (+ (fib (- n 1)) (fib (- n 2)))))

# Correctness
(assert (= (fib 20) 6765) "fib(20) should be 6765")
(assert (= (fib 0) 0) "fib(0) should be 0")
(assert (= (fib 1) 1) "fib(1) should be 1")

# Arithmetic rewrite correctness
(defn add-two [a b]
  (+ a b))
(assert (= (add-two 3 4) 7) "add-two with ints")
(assert (= (add-two 1.5 2.5) 4.0) "add-two with floats")
(assert (= (add-two 1 2.0) 3.0) "add-two with mixed int/float")

(defn sub-one [n]
  (- n 1))
(assert (= (sub-one 10) 9) "sub-one")

(defn double [n]
  (* n 2))
(assert (= (double 21) 42) "double")

# Comparison rewrite correctness
(defn less [a b]
  (< a b))
(assert (less 1 2) "less 1 2")
(assert (not (less 2 1)) "not less 2 1")
(assert (less 1.0 2.0) "less floats")

# Type error preservation: non-number args to + should still error
(let [[ok? _] (protect ((fn [] (+ 1 "a"))))]
  (assert (not ok?) "type error on + with string"))
