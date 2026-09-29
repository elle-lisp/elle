(elle/epoch 12)
# Regression: a closure in call-argument position inside a loop body
# must not have its region's free_at extended to the loop boundary.
#
# With an empty iterator the body never runs, so the bytecode
# MakeClosure never executes; but an unconditional DecrefRegion emitted
# at the loop exit then targets a region that was never
# alloc_in_region'd -> phantom-region panic
# (src/value/fiberheap/regionstore.rs).
#
# Root cause: liveness decided "is this binding bound inside or outside
# the loop?" by comparing HirId *magnitude* (binding_form_id > loop_id
# => outside). The ANF lift appends synthetic `let` bindings with fresh
# HirIds drawn from the end of the global counter, so a binding bound
# INSIDE the loop body gets an id LARGER than the loop -> misclassified
# as "outside" -> region free_at extended to the loop -> phantom.
# Fixed by computing an explicit structural post-order index and
# comparing that, not the HirId.

(defn use-it [f]
  (f))

# Empty iterator: the closure alloc never fires. This is the phantom
# trigger -- it panicked before the fix.
(each c in @[]
  (use-it (fn () 1)))

# Non-empty iterator: the closure alloc fires every iteration and the
# closure actually runs. Confirms the value path stays correct.
(def @results @[])
(each c in @[10 20 30]
  (use-it (fn () (push results c))))
(assert (= (freeze results) [10 20 30])
        "closure-arg in loop body runs per iteration")

(println "loop-closure-arg-phantom: PASS")
