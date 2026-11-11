(elle/epoch 15)
# audited: 2026-11-11
# sqrt, log, pow, trig, the math/ rounding names and the constants answer as documented.
# docs/stdlib.md
#

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

# rounding under its math/ names
(assert (= math/floor floor) "math/floor is floor")
(assert (= math/ceil ceil) "math/ceil is ceil")
(assert (= math/round round) "math/round is round")
(assert (= (math/floor 3.7) 3) "math/floor 3.7")
(assert (= (math/floor -3.2) -4) "math/floor -3.2")
(assert (= (math/ceil 3.2) 4) "math/ceil 3.2")
(assert (= (math/round 2.5) 3) "math/round 2.5")
(assert (= (math/round -2.5) -3) "math/round -2.5 goes away from zero")
(assert (integer? (math/round 2.5)) "math/round gives an integer")
(assert (= (math/floor 7) 7) "math/floor passes an integer through")
(assert (= (doc math/round) (doc round)) "math/round shows round's docstring")

(let [[ok? _] (protect ((fn [] (math/round "hello"))))]
  (assert (not ok?) "math/round type error"))

# constants
(assert (float? (math/pi)) "pi is float")
(assert (float? (math/e)) "e is float")
(assert (= (math/inf) (/ 1.0 0.0)) "inf")

(println "prim-math: all tests passed")
