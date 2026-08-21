(elle/epoch 12)
# Counterfactual for the call-evaluation-order release mistarget.
#
# `lower_call` evaluates a call's ARGUMENTS first and its FUNC expression
# last (both the plain and splice paths). The analysis side's structural
# execution order (`compute_order` over `Hir::for_each_child`) must agree,
# or a binding whose last read sits in func position is released at the
# arg-position read: the binding's call-result value region gets its
# `DecrefValueRegion` + nil slot-stamp emitted right after the FIRST get,
# and the func-position get then reads the stamped nil —
# "get: expected collection …, got nil".
#
# This is the minimized tests/elle/compress.lisp `(z:unzstd (z:zstd ""))`
# failure: the same module binding read twice within one nested call, once
# in arg position (executed first) and once in func position (executed
# last). Reading z through two separate top-level forms masks it, as does
# using two distinct module bindings.

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
