(elle/epoch 13)
# audited: 2026-09-29
# %freeze and %thaw allocate their result in the region the lowerer assigned it.
# docs/impl/region/mechanism.md
#
# The lowerer assigns each result a region and emits a DecrefRegion for that
# region at scope exit, and the IntrFreeze and IntrThaw opcodes carry the
# region as an operand. A result allocated in whatever region was active
# instead leaves the scope-exit DecrefRegion naming an empty slot. A debug
# build then panics at the phantom-region assert in decref_reaches_zero
# (src/value/fiberheap/regionstore/refcount.rs), and a release build counts
# the release in arena/over-frees.
#
# A top-level def of the result, followed by any use of it, reaches that
# scope-exit DecrefRegion. tests/lang/intrinsics.lisp asserts what the two
# intrinsics return.
(def frozen (%freeze @[1 2 3]))
(assert (%array? frozen) "%freeze of @array survives until next use")

(def thawed (%thaw [1 2 3]))
(assert (%array? thawed) "%thaw of immutable survives until next use")

(println "intrinsics-freeze-region: ok")
