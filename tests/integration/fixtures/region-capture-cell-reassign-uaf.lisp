(elle/epoch 12)
# tests/integration/fixtures/region-capture-cell-reassign-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because BEFORE the fix it was a real
# double-free that ABORTED/SIGSEGV'd on roughly half of *plain* runs, and `make
# smoke` globs tests/elle/*.lisp into one shared process where an abort would
# take the whole harness down. It is exercised by the guardfree subprocess pin in
# tests/integration/elle_scripts.rs (`region_capture_cell_reassign_uaf`), which
# faults deterministically if the defect ever returns.
#
# WHAT IT REPRODUCES
#   The elle corpus surfaces a timing-dependent phantom-region / double-free
#   panic in a spawned `sys/spawn` worker. The heavy worker re-runs `init_stdlib`
#   (a SECOND compile in the process), and the process-global static-region
#   counter (src/lir/lower/mod.rs NEXT_STATIC_REGION) has advanced, so the second
#   compile mints physical region ids in a shifted range. A latent double-free in
#   the region solver — present on EVERY stdlib load but benign on the first
#   because the freed page survives untouched — then corrupts the worker's region
#   store: either the regionstore.rs:210 DecrefRegion phantom assert, or a garbage
#   region id fed to `incref -> ensure -> Vec::resize_with` -> a multi-hundred-GB
#   allocation (OOM/abort). Confirmed counter-INDEPENDENT: arming guardfree before
#   the very first load faults identically (region/slot ids exactly half the
#   second-load values).
#
# ROOT CAUSE (--trace=guardfree, not a guess)
#   A top-level mutable that is BOTH captured by a closure (so the lowerer boxes
#   it in a MakeCaptureCell) AND reassigned: its slot holds the CELL, not the
#   value. The init value's release was ROUTED through that slot — the lowerer
#   recorded `region_to_slot[init_region] = acc.slot`, so at the init region's
#   decref_point the lowerer emitted `LoadLocal acc.slot + DecrefValueRegion`.
#   But `DecrefValueRegion` resolves its target with `result_region_of`, which
#   UNWRAPS a capture cell to its CURRENT content. By the time that decref fires
#   (the binding's last use, after the loop), the reassignments have repointed
#   the cell at a LATER, still-live value — so the decref freed that live value
#   (UAF) and the displaced original leaked. guardfree pins it:
#       [guardfree] SIGSEGV — use-after-free
#           free site: DecrefValueRegion of capture-cell (runtime region N)
#           context: UpdateCapture        (StoreCaptureCell, vm/capture.rs)
#
#   The init value's release fires at the WRONG region because the cell content
#   it unwraps to is no longer the value the static decref named. A captured
#   binding that is NEVER reassigned does not hit this — the cell content is
#   stable, so the unwrap always names the right value (that path is unchanged).
#
# FIX (landed)
#   The region analysis flags top-level captured AND reassigned bindings
#   (`RegionInfo::captured_reassigned_bindings`). For those the lowerer
#   (`Lowerer::store_captured_cell_init`) drops the init value's alloc reference
#   off its OWN value register right after `StoreCaptureCell` — timing-independent
#   and unambiguous — and SKIPS `record_region_slot` for the init so no decref is
#   routed through the (repointed) cell slot. The cell's membership reference for
#   the init (raised by `StoreCaptureCell`/`handle_update_capture`) is released by
#   the first reassignment's drop-on-overwrite; the final value by the cell's free
#   cascade. Each reassigned value's own producer-temp decref drops its alloc
#   reference at its production, off its own temp — never via the cell slot.
#
# Do not "green" a regression by dropping the capturing closure (`use`) — that
# takes it off the cell path (see tests/elle/region-mutable-reassign-selfref.lisp)
# — or by shortening the loop, which is what makes the dangling free deterministic
# here.

(def @acc (list))
(defn use []
  acc)  # capture @acc -> it is boxed in a capture cell
(each i (list 1 2 3)
  (assign acc (pair i acc)))  # each iteration: StoreCaptureCell (UpdateCapture)
(assert (= (reverse acc) (list 1 2 3))
        "captured reassigned mutable keeps every chained value")

(println "region-capture-cell-reassign-uaf: OK")
