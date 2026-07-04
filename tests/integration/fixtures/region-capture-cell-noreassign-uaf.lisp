(elle/epoch 12)
# tests/integration/fixtures/region-capture-cell-noreassign-uaf.lisp
#
# GREEN (regression guard). Quarantined here — NOT under tests/elle/ — because a
# regression is a real use-after-free that SIGSEGVs / tag-mismatch-panics on a
# TIMING-DEPENDENT subset of runs, and `make smoke` globs tests/elle/*.lisp into
# one shared process where an abort would take the whole harness down. Exercised
# by the guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_capture_cell_noreassign_uaf`), which loops enough times to witness a
# reintroduced flaky fault reliably.
#
# WHAT IT GUARDS
#   A top-level mutable that is captured by one or more closures (so the lowerer
#   boxes it in a MakeCaptureCell) but is NEVER reassigned. The CELL's region was
#   released a step too early: under guardfree the freed cell pages were then
#   read (a torn capture-cell / captured-env deref), e.g.
#       [guardfree] SIGSEGV — use-after-free
#           free site: DecrefRegion(bytecode slot N)   (the cell's own demise)
#   The fault was per-run timing-dependent (~⅓ of runs), the BENIGN sibling of
#   the reassign defect (region-capture-cell-reassign-uaf.lisp).
#
# ROOT CAUSE (fixed) — NOT an RC imbalance; the accounting balanced. It was
# compile-order nondeterminism, two ways at once:
#
#   1. `compute_last_use`'s binding-chain override iterated a hash-ordered map
#      while reading entries it was itself overriding, so a random PREFIX of
#      each binding chain resolved per compile. acc's cell reaches the (u1)
#      call site only through u1's override — the capture-use registers at the
#      Lambda node, which IS u1's init id — so when acc happened to resolve
#      first, its cell's decref_point landed before the closure calls and the
#      cell was freed while u1 was still callable. Fixed by solving the
#      override equations to their (unique, order-independent) fixpoint in
#      src/hir/liveness/lastuse.rs.
#   2. At the shared decref_point, the emission order of the cell's
#      page-FREEING DecrefRegion vs the init's page-READING DecrefValueRegion
#      (which unwraps the cell) came from hash iteration; the freeing-first
#      permutation tears the page the unwrap reads. Fixed by the
#      dependency-safe class sort in `Lowerer::with_region_info`
#      (docs/impl/region-rules.md Rule 4: read-releases before free-releases,
#      deterministic order always).
#
# Compile-level twins of this guard live in src/lir/lower/tests.rs
# (`release_order_value_gated_before_plain_in_shared_bucket`,
# `release_order_is_deterministic_across_compiles`,
# `region_analysis_is_deterministic_across_compiles`).
#
# Do not "green" this by dropping the capturing closures — that takes the binding
# off the cell path entirely (it is no longer boxed in a MakeCaptureCell).

(def @acc (list 1 2 3))
(defn u1 []
  acc)  # each closure captures @acc -> it is boxed in a capture cell
(defn u2 []
  acc)
(defn u3 []
  acc)
(defn u4 []
  acc)
(defn u5 []
  acc)
(assert (= (first (u1)) 1) "captured non-reassigned mutable survives its reads")
(assert (= (first (u5)) 1)
        "captured non-reassigned mutable survives its reads (2)")

(println "region-capture-cell-noreassign-uaf: OK")
