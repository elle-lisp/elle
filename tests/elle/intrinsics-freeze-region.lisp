(elle/epoch 11)
# Regression: %freeze and %thaw must allocate the result value in the
# region the lowerer's emit_alloc assigned. Previously the IntrFreeze
# / IntrThaw bytecode opcodes carried no region operand, so the new
# value landed in whatever region happened to be active (the calling
# function's body region). The DecrefRegion emitted for the assigned
# region at scope exit then targeted an empty slot and panicked with
# the phantom-region debug_assert in
# src/value/fiberheap/regionstore.rs::decref_with_cascade.
#
# Minimal pair: (def of a frozen value) followed by any use of that
# value reaches a top-level scope-exit DecrefRegion that fires the
# phantom assertion. Counterpart to tests/elle/intrinsics.lisp
# lines 382-390 which exercises the same code path inside the broader
# intrinsic suite.
(def frozen (%freeze @[1 2 3]))
(assert (%array? frozen) "%freeze of @array survives until next use")

(def thawed (%thaw [1 2 3]))
(assert (%array? thawed) "%thaw of immutable survives until next use")

(println "intrinsics-freeze-region: ok")
