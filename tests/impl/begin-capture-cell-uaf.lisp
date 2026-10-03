#!/usr/bin/env elle
(elle/epoch 12)

# audited: 2026-09-23
# A capture cell a top-level begin creates stays alive for a sibling form that binds the same name again.
# docs/impl/region/cells.md
#
# The pair:
#
#   1. A top-level `(begin (def x …) (defn f [] x) …)` whose Begin
#      pre-pass emits `MakeCaptureCell` for x at the Begin's HirId.
#   2. A sibling top-level form that binds and uses x again.
#
# The region walk's Begin arm (src/hir/region/infer/walk/walkrest.rs) records
# the cell region in `binding_regions[b]` for each pre-allocated capturable
# binding, and a Define unions into `binding_regions` rather than overwriting.
# So the cell region's `decref_point` extends to the binding's last use.
# Counterfactual: a `decref_point` at the end of form 1 frees the cell's region,
# form 2's struct reuses the slot, and the next access to x reads through a
# dangling `CaptureCell` into that struct. The unit pin is
# `begin_capture_cell_region_extends_to_binding_last_use_across_sibling_forms`
# in src/hir/region/infer/tests/looprc.rs.

(begin
  (def x 100)
  (defn add-to-x (& nums)
    (+ x (first nums)))
  (assert (= (add-to-x 42) 142) "form 1: variadic closure capture"))

(begin
  (def {:x x :y y :z z} {:x 1 :y 2 :z 3})
  (assert (= (+ x (+ y z)) 6)
          "form 2: struct destructure reusing x's binding across sibling forms"))
