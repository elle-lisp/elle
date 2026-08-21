#!/usr/bin/env elle
(elle/epoch 12)

# tests/elle/begin-capture-cell-uaf.lisp
#
# Family E regression — capture-cell UAF across sibling top-level forms.
#
# The minimal pair (extracted from tests/elle/destructuring.lisp forms
# 70 + 187 via the eval-prefix bisection harness in notes.md):
#
#   1. A top-level `(begin (def x …) (defn f [] x) …)` whose Begin
#      pre-pass emits `MakeCaptureCell` for x at the Begin's HirId.
#   2. A sibling top-level form that re-assigns and uses x.
#
# Pre-fix, the cell's region's `free_at` was the Begin's own last use
# (the end of form 1). The compiler-emitted `DecrefRegion` at that
# free_at freed the cell's region, the slab slot was reused for the
# struct allocated by form 2's destructure, and the next access to x
# read through a dangling `CaptureCell` Value (tag=0x22) into a
# slot now holding an `LStruct` — canonical use-after-free caught at
# `as_capture_cell` in `handle_update_capture`.
#
# Fix: `src/hir/regions.rs` HirKind::Begin records the cell region in
# `binding_regions[b]` for each pre-allocated capturable binding so the
# post-pass `free_at` extension via `compute_last_use` covers it; and
# `HirKind::Define` now unions into `binding_regions` instead of
# overwriting (same overwrite-vs-union class as the Destructure fix
# in commit 204e5ebb). Counterfactual (unit test):
# `src/hir/regions.rs::tests::
#  begin_capture_cell_region_extends_to_binding_last_use_across_sibling_forms`.

(begin
  (def x 100)
  (defn add-to-x (& nums)
    (+ x (first nums)))
  (assert (= (add-to-x 42) 142) "form 1: variadic closure capture"))

(begin
  (def {:x x :y y :z z} {:x 1 :y 2 :z 3})
  (assert (= (+ x (+ y z)) 6)
          "form 2: struct destructure reusing x's binding across sibling forms"))
