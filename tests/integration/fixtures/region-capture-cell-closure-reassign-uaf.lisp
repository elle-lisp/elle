(elle/epoch 12)
# tests/integration/fixtures/region-capture-cell-closure-reassign-uaf.lisp
#
# The capture-cell reassign hazard reached through a CLOSURE. A binding defined
# outside any lambda gets a compiled `MakeCaptureCell` in its own slot, so the
# lowerer must not route the init value's release through that slot: the release
# is a `LoadLocal slot` + `DecrefValueRegion`, and `result_region_of` unwraps a
# capture cell to whatever content it holds AT THE RELEASE. A reassignment
# repoints the cell, so the release then frees a different, live value — the
# capture-cell reassign UAF (docs/impl/region/bindings.md § "Captured reassigned
# cells").
#
# The sibling fixture region-capture-cell-reassign-uaf.lisp writes its
# reassignment at file scope. Here the `assign` sits INSIDE a closure the
# defining scope encloses, which is the whole point: the routing decision is a
# fact about the BINDING (some `assign` repoints its cell), not about the scope
# the `assign` happens to sit in. Reading it off the assign site classifies this
# binding as fn-local, leaves the cell-slot routing in place, and over-releases
# the reassigned value by one — the release the `begin`'s frame exit fires against
# the cell frees the very list the `begin` hands back.
#
# Deliberately ONE face and nothing else. Whether a one-short region reaches zero
# while the program still runs depends on what else the file allocates, so extra
# faces here would mask this one rather than add coverage. The shape-level
# coverage (a heap init; the write two closures deep) is compile-level and
# deterministic: `lir::lower::tests::release`'s
# `*_closure_reassign_leaves_no_cell_slot_release`, which assert that no slot
# holding a compiled capture cell ever carries a value-routed release. The
# value-level face — every element of a handed-back list reading back correctly —
# is `unittests::jit::jit_tests::test_jit_before_and_after_threshold`.

(def kept
  (begin
    (var results (list))
    (defn collect (n)
      (if (= n 0)
        results
        (begin
          (assign results (pair n results))
          (collect (- n 1)))))
    (collect 5)))

# Junk allocations reclaim the page an over-release freed, so a stale read is a
# wrong value and not merely a lucky intact one.
(def junk (list (pair 8 8) (pair 9 9) (pair 8 8) (pair 9 9)))

(assert (= (first kept) 1)
        "the closure-reassigned cell's value survives the frame exit")
(assert (= (length kept) 5) "and every element the closure pushed with it")
(println "region-capture-cell-closure-reassign-uaf: OK")
