(elle/epoch 12)
# tests/integration/fixtures/region-fold-closure-arg-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because it SIGSEGVs under
# --trace=guardfree, and `make smoke` globs tests/elle/*.lisp into one shared
# process where a segfault would take the whole harness down. It is exercised by
# the guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_fold_closure_arg_uaf`), which is #[ignore]'d RED until the over-free is
# fixed.
#
# WHAT IT REPRODUCES — the closure-arg OVER-FREE (the over-free twin of the F5
# arg-retain gap).
#   A fold-shaped helper that holds its combiner `f` by THREADING it as a
#   recursive argument (`go-threaded`) instead of CAPTURING it in a letrec lets
#   `f`'s region reach refcount 0 mid-drive: its DecrefValueRegion frees the
#   closure and a later UpdateCapture derefs the freed page —
#
#     [guardfree] SIGSEGV — use-after-free
#     free site: DecrefValueRegion of closure (runtime region N)   <- the `f` arg
#     context:   UpdateCapture
#
#   A `def @`-cell accumulator (`while` loop) is the same shape with the same
#   trace; the threaded-arg form is used here because it faults in plain user
#   scope. The bug is NOT pre-prelude-specific and NOT specific to `f`=`+`; it is
#   a general closure-lifetime gap for the threaded-arg / cell-held shape.
#
#   It is STATE-DEPENDENT: it only faults once region ids recycle onto the freed
#   one, so a short loop — or one that COMPARES the fold result — looks clean
#   (either changes the allocation sequence). The discard-the-result drive below
#   reaches the collision deterministically (~8000 reps; per-call `->array`
#   churns region ids). Do NOT "verify" a fix of this with a small run.
#
# THE CONTRAST — the SAME drive over the SAFE form (combiner CAPTURED in a letrec,
# exactly src/core.lisp `fold`) is guardfree-CLEAN, so this isolates the
# threaded-arg / cell-held closure lifetime, not folding in general.
#
# WHEN FIXED — the region solver keeps a threaded/cell-held closure's region live
# across its whole use — this exits 0 under guardfree; un-#[ignore] the pin then.
# See src/core.lisp `fold` (the comment there) and assessment.md Stage 1 § the
# soundness note.

(defn go-threaded [f arr n i acc]
  (if (%lt i n)
    (go-threaded f arr n (%add i 1) (f acc (get arr i)))
    acc))
(defn fold-threaded [f init coll]
  (let [arr (->array coll)
        n (length arr)]
    (go-threaded f arr n 0 init)))
(defn drive [thunk reps]
  (def @c 0)
  (while (%lt c reps)
    (thunk)
    (assign c (%add c 1))))

# `+` is a shared stdlib closure; threading it per fold call over-decrefs its
# region until it frees, then the next use faults. The result is discarded on
# purpose — comparing it changes the allocation sequence and hides the fault
# (this is a UAF guard, not a value test).
(drive (fn [] (fold-threaded + 0 [1 2 3])) 8000)
(println "region-fold-closure-arg-uaf: ok")
