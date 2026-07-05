(elle/epoch 12)
# tests/integration/fixtures/region-traits-capture-adopt-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because a REGRESSION faults on the ownership
# forest's default path: behaviourally `(get (traits a) :type)` reads `nil` instead of
# `:my-type` (a wrong answer over a freed table), and under `--trace=guardfree` it
# SIGSEGVs. `make smoke` globs tests/elle/*.lisp into one shared process, so a fault there
# would take the whole harness down; it is exercised by the guardfree subprocess pin in
# tests/integration/elle_scripts.rs (`region_traits_capture_adopt_uaf`), which faults
# deterministically if the forest stops recording the trait-table embed containment.
#
# NOT the same bug as region-traits-table-uaf.lisp. That one was a RUNTIME RC gap
# (`clone_with_traits` stored the table into the `traits` side-field and the alloc-scan
# `find_object_cross_refs` did not enumerate it, so RC never counted the edge) — fixed, and
# flag-independent. THIS one is a COMPILE-TIME OWNERSHIP invariant: with the alloc-scan now
# counting the edge, per-region RC alone is clean, but the FOREST must ALSO see the
# containment or it frees the table one demise early regardless of the count.
#
# WHAT IT EXERCISES
#   A closure `make` CAPTURES a top-level struct `shared-tbl` and, in its body, attaches it
#   as a trait table with `with-traits`. The returned traited value `a` outlives `make`'s
#   activation and its `traits` side-field still references `shared-tbl`.
#
# THE INVARIANT (what keeps this green)
#   `with-traits` declares `RegionEffect::Fresh` AND `embeds: &[1]`: the table is EMBEDDED
#   into the fresh result and alloc-scan counted (Rule 5), exactly like `%pair` embedding
#   its car/cdr — an embedding constructor is `Fresh`, not `Stores`. The ownership forest
#   learns containment from `cross_region_refs` (intrinsic stores), capture edges, the
#   funnel-recovered `containment_edges` (mutable-container `%put`), AND — via the embed
#   declaration — a `Fresh` native's `result ⊇ arg` embed (`call_embeds`, recorded by the
#   walk's `Fresh` arm into `containment_edges`), the compile-time analog of the runtime
#   `find_object_cross_refs`/`traits()` alloc-scan. So the forest sees `result ⊇ shared-tbl`;
#   the result escapes `make`, so the captured `shared-tbl` is referenced from OUTSIDE make's
#   subtree — external uniqueness refuses to capture-adopt it, and it stays Shared, reclaimed
#   by per-region RC under `a`'s live `traits` reference.
#
#   Drop the embed declaration and the forest never sees `result ⊇ shared-tbl`: it judges the
#   captured `shared-tbl` externally unique to `make`, capture-adopts it into `make`'s subtree,
#   and make's subtree drop frees it while `a`'s `traits` field still points at it — a
#   use-after-free (context `UpdateCapture` under guardfree), and a wrong answer once the page
#   recycles on plain runs.

(def shared-tbl {:type :my-type})
(def make (fn (data) (with-traits [data] shared-tbl)))
(def a (make 1))
(assert (= (get (traits a) :type) :my-type)
        "a captured trait table must survive its capturing closure's subtree drop")
(println "ok")
