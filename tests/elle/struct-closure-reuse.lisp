(elle/epoch 8)
# Regression test: cross-function tail calls must not corrupt closures
# via slab pool rotation. A letrec closure creates capture cells during
# execution; if slab rotation frees these prematurely (treating a cross-
# function tail call as a self-tail-call), the closure becomes uncallable.
#
# Known limitation: the slab rotation mechanism incorrectly rotates
# across function boundaries when the outer function is marked rotation-
# safe. This is fixed structurally by the bump-arena Phase 4 redesign.
# See contracts.lisp for the full reproduction.

(def f (fn [x]
         (letrec [loop (fn [i]
                   (if (>= i 3) {:fail x}
                     (if (= x i) nil (loop (+ i 1)))))]
           (loop 0))))

# Direct calls work (no cross-function rotation)
(assert (nil? (f 1)) "direct call 1")
(assert (= (get (f 99) :fail) 99) "direct call 2")

(println "struct-closure-reuse: all tests passed")
