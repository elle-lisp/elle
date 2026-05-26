(elle/epoch 11)
## tests/elle/prim-math.lisp
## Mathematical operations: sqrt, log, pow, trig, constants

(assert (= (sqrt 16) 4.0) "sqrt int")
(assert (= (sqrt 16.0) 4.0) "sqrt float")

(let [[ok? _] (protect ((fn [] (sqrt "hello"))))]
  (assert (not ok?) "sqrt type error"))

# log
(assert (< (abs (- (log (math/e)) 1.0)) 1e-10) "log(e) = 1")
(assert (< (abs (- (log 8 2) 3.0)) 1e-10) "log base 2 of 8 = 3")

# pow
(assert (= (pow 2 8) 256) "pow int positive")
(assert (= (pow 2 -1) 0.5) "pow int negative exponent")
(assert (< (abs (- (pow 4.0 0.5) 2.0)) 1e-10) "pow float")

# trig
(assert (= (sin 0) 0.0) "sin(0) = 0")
(assert (= (cos 0) 1.0) "cos(0) = 1")

# constants
(assert (float? (math/pi)) "pi is float")
(assert (float? (math/e)) "e is float")
(assert (= (math/inf) (/ 1.0 0.0)) "inf")

(println "prim-math: all tests passed")
