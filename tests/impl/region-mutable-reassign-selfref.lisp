(elle/epoch 13)
# audited: 2026-09-29
# A reassigned top-level binding read after the reassignment keeps every element of its current value.
# docs/impl/region/bindings.md
#
# The sidecar arms guardfree, so a read of a freed element faults.
#
# A reassigned binding is a 1-slot container: drop-on-overwrite releases each
# displaced value, and frame teardown releases the last one. No release is
# routed through the binding's slot at its last use.
# region-toplevel-mutable-reassign.lisp holds the face with no read after the
# reassignment, and region-toplevel-reassign-thunk-uaf.lisp the same source
# run inside a function.
#
# The counter-factual: `emit_decrefs_for` (src/lir/lower/regiondecref.rs)
# releases a region with `LoadLocal(slot) + DecrefValueRegion`, which decrefs
# whatever the slot holds NOW. A decref_point for the binding's INIT region at
# its last use therefore reloads the slot, now the CURRENT value, and frees it
# at the read's Var node, before the enclosing expression consumes the loaded
# value. Under --trace=guardfree that faults with
# `free site: DecrefValueRegion of <type> (slot operand <init-region>) @ <read>`.
# The fault needs the reassignment, and it needs the read to consume the
# loaded value within the same expression as the last-use decref. So the
# `reverse` read and the loop are both part of the shape.

(def @acc (list))
(each i (list 1 2 3)
  (assign acc (pair i acc)))
(assert (= (reverse acc) (list 1 2 3))
        "self-referential each accumulation into a top-level @ preserves all elements")

(println "region-mutable-reassign-selfref: OK")
