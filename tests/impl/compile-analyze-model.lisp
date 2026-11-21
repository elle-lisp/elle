(elle/epoch 18)
# audited: 2026-11-20
# compile/analyze reports the cell kind this compiler gives a captured mutable binding.
# docs/compile-time.md
#
# The kind is no language claim. Another implementation may capture a mutable
# binding by any means it likes.

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
