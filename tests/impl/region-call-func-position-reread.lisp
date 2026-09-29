(elle/epoch 13)
# audited: 2026-09-29
# A binding read in a call's argument and again in its function position is released after the second read.
# docs/impl/region/rules.md
#
# `lower_call` evaluates a call's ARGUMENTS first and its FUNC expression
# last, on both the plain and the splice path. The analysis's structural
# execution order (`compute_order` over `Hir::for_each_child`) must agree.
#
# The counter-factual: an order that puts the func-position read first
# releases the binding at the arg-position read. The binding's call-result
# region gets its `DecrefValueRegion` and nil slot stamp right after the
# FIRST get, and the func-position get reads the stamped nil —
# "get: expected collection …, got nil".
#
# The shape needs one binding read twice inside one nested call, once in arg
# position and once in func position, as in `(z:unzstd (z:zstd ""))`. Reading
# z through two separate top-level forms does not reach it, and neither do two
# distinct bindings.

(def z
  ((fn []
     (defn f [x]
       1)
     (defn g [x]
       2)
     {:f f :g g})))

(def d ((get z :g) ((get z :f) "")))
(assert (= d 2) "func-position re-read after arg-position read")

# The let-bound variant of the same shape.
(let [w ((fn []
           (defn h [x]
             3)
           (defn k [x]
             4)
           {:h h :k k}))]
  (assert (= ((get w :k) ((get w :h) "")) 4) "let-bound func-position re-read"))

(println "region-call-func-position-reread: ok")
