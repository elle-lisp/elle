(elle/epoch 13)
# audited: 2026-09-29
# A captured top-level mutable nothing reassigns keeps its cell alive across every closure that reads it.
# docs/impl/region/rules.md
#
# The sidecar arms guardfree, which faults at the torn cell read every time. A
# plain run faults on a timing-dependent third of runs.
#
# WHAT IT GUARDS
#   A top-level mutable that is captured by several closures (so the lowerer
#   boxes it in a MakeCaptureCell) but is never reassigned. Two orders decide
#   whether the cell outlives the closures that read it, and both must be the
#   same on every compile:
#
#   1. `compute_last_use` solves the binding-chain override equations to their
#      fixpoint (src/hir/liveness/lastuse.rs). acc's cell reaches the (u1)
#      call site only through u1's override, so a solver that read a
#      hash-ordered prefix of the chain put the cell's decref_point before the
#      closure calls on some compiles.
#   2. At a shared decref_point, `order_releases` emits the init's page-reading
#      DecrefValueRegion (which unwraps the cell) before the cell's page-freeing
#      DecrefRegion (Rule 4). The other order tears the page the unwrap reads.
#
# Compile-level twins of this guard live in src/lir/lower/tests/release/order.rs
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
