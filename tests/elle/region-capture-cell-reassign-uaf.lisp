(elle/epoch 12)
# region-capture-cell-reassign-uaf.lisp
#
# The captured-cell-reassign path, end-to-end. A top-level binding that is BOTH
#   (a) captured by a closure — forcing a compiled `MakeCaptureCell` box
#       (`needs_capture`), and
#   (b) reassigned — landing it in `captured_reassigns` (module-scope captured
#       reassign),
# is initialized through `store_captured_cell_init` with reassigned=true. Because
# a reassignment REPOINTS the cell, the init value's producer reference must be
# dropped OFF ITS OWN REGISTER at the define — NOT routed through the cell slot,
# which a later reassignment has already repointed (a slot-load + unwrap would then
# free a different, live value: the capture-cell reassign UAF).
#
# That init-drop is transform 1's DECREF side (docs/impl/region/mechanism.md
# § "Compile-time region selection (coalescing)"): when the init is a fresh local
# allocation whose region is a known static slot — a
# heap LITERAL (`'(…)`, `"…"`) materializes fresh in its own region, the
# coalescible case — the drop lowers slot-resolved (`DecrefRegion`, guarded under
# debug by the `AssertRegionMatches` equivalence oracle) instead of value-resolved
# (`DecrefValueRegion`). An init that is instead an opaque CALL result (`(pair …)`
# is a stdlib closure call, not a direct allocation) stays value-resolved — the
# caller cannot name the callee's region (the dynamic boundary). The substitution
# is RC-neutral — it resolves the SAME physical region — so it must be a no-op at
# runtime, introducing no UAF.
#
# This pins CORRECTNESS. A mis-coalesce — the init slot resolving to a
# wrong/dead physical region — frees the init's region early; the junk allocations
# below then reuse it, so the wrong-value asserts catch the stale read
# deterministically, `--trace=guardfree` detonates on the freed page, and the debug
# `AssertRegionMatches` oracle panics at the exact coalesced instruction. The
# closure-captured + reassigned shape is the canonical witness several region
# comments cite for this defect class.

# ── 1. literal init (coalesced drop) reassigned to a call result ────────────────
#       The init `'(1 2)` is a direct MaterializeConst allocation → its drop
#       coalesces; the reassign value is reached through the cell, not this drop.
(def @cap-a '(1 2))
(def read-a (fn () cap-a))
(assign cap-a (pair 3 4))
(def keep-a (list (read-a) (read-a) (read-a)))
(def junk-a (list (pair 8 8) (pair 9 9) (pair 8 8) (pair 9 9)))
(assert (= (first keep-a) (pair 3 4))
        "captured+reassigned cell reads its current value through the closure")
(assert (= cap-a (pair 3 4)) "direct read of the captured+reassigned cell")
(println "cap-reassign-1 ok")

# ── 2. string-literal init (a distinct coalescible heap kind), same shape ───────
(def @cap-b "init")
(def read-b (fn () cap-b))
(assign cap-b (concat "re" "assigned"))
(def keep-b (list (read-b) (read-b)))
(def junk-b (list (pair 1 1) (pair 2 2) (pair 3 3)))
(assert (= (first keep-b) "reassigned")
        "captured+reassigned string cell reads its current value")
(println "cap-reassign-2 ok")

# ── 3. cross-region: the reassigned value references ANOTHER top-level region ────
(def @cap-c '(0 0))
(def other (pair 7 8))
(def read-c (fn () cap-c))
(assign cap-c (pair 9 other))
(def keep-c (list (read-c) (read-c)))
(def junk-c (list (pair 5 5) (pair 6 6) (pair 5 5)))
(assert (= (first (read-c)) 9) "cross-region head preserved")
(assert (= (rest (first keep-c)) (pair 7 8))
        "cross-region referenced region preserved")
(println "cap-reassign-3 ok")

(println "region-capture-cell-reassign-uaf: OK")
