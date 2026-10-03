(elle/epoch 13)
# audited: 2026-09-29
# A captured top-level mutable an assign repoints releases its init off its own register, not through the cell.
# docs/impl/region/cells.md
#
# The sidecar arms guardfree, which faults at the stale free every time. A
# plain run double-frees on about half of runs, and a second stdlib compile in
# a `sys/spawn` worker turns the same free into a phantom-region assert.
#
# THE RULE
#   A top-level mutable that is BOTH captured by a closure (so its slot holds a
#   MakeCaptureCell) AND reassigned must not route its init's release through
#   that slot. `DecrefValueRegion` resolves its target with `result_region_of`,
#   which unwraps the cell to its CURRENT content, and by the binding's last use
#   the reassignments have repointed the cell at a later, live value.
#   `RegionInfo::captured_reassigned_bindings` names such a binding, and
#   `Lowerer::store_captured_cell_init` drops the init's reference off its own
#   register right after `StoreCaptureCell` instead. The cell's membership
#   reference for the init goes at the first overwrite, and the final value's
#   at the cell's free cascade.
#
# THE COUNTER-FACTUAL
#   Routed through the slot, the release frees the live value and leaks the
#   displaced original:
#       [guardfree] SIGSEGV — use-after-free
#           free site: DecrefValueRegion of capture-cell (runtime region N)
#           context: UpdateCapture
#
# Do not "green" a regression by dropping the capturing closure (`use`) — that
# takes it off the cell path (see tests/impl/region-mutable-reassign-selfref.lisp)
# — or by shortening the loop, which is what makes the dangling free deterministic
# here.

(def @acc (list))
(defn use []
  acc)  # capture @acc -> it is boxed in a capture cell
(each i (list 1 2 3)
  (assign acc (pair i acc)))  # each iteration: StoreCaptureCell (UpdateCapture)
(assert (= (reverse acc) (list 1 2 3))
        "captured reassigned mutable keeps every chained value")

(println "region-capture-cell-reassign-loop-uaf: OK")
