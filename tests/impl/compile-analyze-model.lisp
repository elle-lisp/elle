(elle/epoch 13)
# audited: 2026-09-29
# compile/analyze reports this compiler's JIT eligibility and the cell kind of a captured mutable binding.
# docs/compile-time.md
#
# Neither fact is a language claim. Another implementation may compile a
# function that errors, and may capture a mutable binding by any means it
# likes.

# add calls stdlib +, which may error on a non-numeric argument. Any signal
# is a potential suspension, so add is not eligible for this JIT.
(def a (compile/analyze "(defn add [a b] (+ a b))"))
(assert (not (get (compile/signal a :add) :jit-eligible))
        "add is not jit-eligible (may error)")

# Both functions carry SIG_ERROR (from * and +), so neither is eligible.
(def a2
  (compile/analyze "
(defn pure-fn [x] (* x x))
(defn caller [x] (pure-fn (+ x 1)))
"))
(assert (not (get (compile/signal a2 :pure-fn) :jit-eligible))
        "pure-fn may error → not jit-eligible")
(assert (not (get (compile/signal a2 :caller) :jit-eligible))
        "caller may error → not jit-eligible")

# A captured, assigned binding lives in an lbox cell.
(def a3
  (compile/analyze "
(defn make-counter [start]
  (var n start)
  (defn next [] (assign n (+ n 1)) n)
  next)
"))
(def caps3 (compile/captures a3 :next))
(assert (= (get (first caps3) :name) "n") "next captures n")
(assert (= (get (first caps3) :kind) :lbox) "n captured as lbox (mutable)")

(println "compile-analyze-model: all tests passed")
