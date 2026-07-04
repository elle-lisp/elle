(elle/epoch 12)
# tests/elle/region-mutable-reassign-selfref.lisp
#
# RESOLVED (kept as a regression guard). Corners the second face of the
# file-letrec mutable-reassign double-free (the first face is the no-trailing-read
# case in region-toplevel-mutable-reassign.lisp): a reassigned top-level binding
# that is READ after the reassignment (in the same or a later expression). Green
# on a direct run via the container model (`top_level_reassigns`); the in-lambda
# face (the same source run as the `%file-body` whole-module thunk under
# `elle test`) is covered by region-toplevel-reassign-thunk-uaf.lisp, fixed by the
# `is_file_scope` classification.
#
# CONFIRMED ROOT CAUSE (dumped LIR + --trace=guardfree, not a guess):
#   `emit_decrefs_for` (src/lir/lower/mod.rs) releases a region with
#   `LoadLocal(slot) + DecrefValueRegion`, which decrefs `region_of(whatever
#   the slot holds NOW)`. A reassigned binding's slot has been overwritten, so:
#     - the binding's INIT region (region_to_slot maps it to the binding slot)
#       still carries a decref_point at the binding's last use; that decref
#       reloads the slot — now the CURRENT value — and frees it. With a trailing
#       read it fires at the read's Var node, BEFORE the enclosing expression
#       consumes the loaded value → use-after-free; with no read it is the
#       second decref of the already-freed current value → the regionstore
#       phantom double-free.
#   guardfree pins it exactly:
#     [guardfree] SIGSEGV — use-after-free
#         free site: DecrefValueRegion of <type> (slot operand <init-region>) @ <read>
#   Verified reassignment-specific: a non-reassigned `(def @r (pair 1 2))` read
#   in the same `(assert (= r ...))` is guardfree-CLEAN; reassigning first is
#   what faults. A reassigned value read only via a SEPARATE later statement is
#   also clean — the fault needs the read to consume the loaded value within
#   the same expression as the binding's last-use decref.
#
# INTENDED FIX (container model, derived from docs/impl/region-bindings.md "a store into a
# mutable container increfs the stored value's region; a removal decrefs it"):
#   - escape-incref the stored value (it now lives >= the binding, balancing
#     its producing temp's demise);
#   - drop-on-overwrite the PRIOR value at the assign (read old before the
#     store) — its true demise is the overwrite, not the binding's last use;
#   - SUPPRESS the init/prior regions' slot-load decref entirely (do NOT merely
#     un-extend it — un-extending moves it earlier, onto the assign, where it
#     frees the just-stored new value: strictly worse).
#   The hard part is the third point: suppressing the binding-slot decref while
#   keeping each value's own temp-slot decref. (A self-referential `(pair i
#   acc)` or a closure capture additionally routes through a capture cell /
#   closure env, overlapping the capture-RC work — same shape, separate path.)
#
# Do not "green" this by dropping the `reverse` read or shortening the loop —
# those are exactly what surfaces the dangling element.

(def @acc (list))
(each i (list 1 2 3)
  (assign acc (pair i acc)))
(assert (= (reverse acc) (list 1 2 3))
        "self-referential each accumulation into a top-level @ preserves all elements")

(println "region-mutable-reassign-selfref: OK")
