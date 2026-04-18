(elle/epoch 8)
# GPU eligibility tests
#
# Tests the fn/gpu-eligible? predicate which checks signal and structural
# properties of closures for GPU compilation candidacy.

# ── Eligible: pure arithmetic ──────────────────────────────────
(assert (fn/gpu-eligible? (fn [a b] (+ a b))) "add is gpu-eligible")
(assert (fn/gpu-eligible? (fn [x] (* x x))) "square is gpu-eligible")
(assert (fn/gpu-eligible? (fn [x] (- 0 x))) "negate is gpu-eligible")
(assert (fn/gpu-eligible? (fn [a b] (< a b))) "compare is gpu-eligible")
# bit/and is not an intrinsic (goes through Call), so not gpu-eligible
(assert (not (fn/gpu-eligible? (fn [x] (bit/and x 0xFF)))) "bit/and call is not gpu-eligible")
(assert (fn/gpu-eligible? (fn [] 42)) "constant is gpu-eligible")

# ── Eligible: control flow ─────────────────────────────────────
(assert (fn/gpu-eligible? (fn [x] (if (> x 0) x (- 0 x))))
  "if-then-else is gpu-eligible")

# ── Not eligible: calls other functions ────────────────────────
(assert (not (fn/gpu-eligible? (fn [x] (println x))))
  "println is not gpu-eligible (yields)")
(assert (not (fn/gpu-eligible? (fn [x] (map inc x))))
  "map is not gpu-eligible (polymorphic)")

# ── Eligible: immutable capture is constant-propagated ─────────
(def outer 10)
(assert (fn/gpu-eligible? (fn [x] (+ x outer)))
  "immutable capture is gpu-eligible (constant-propagated)")

# ── Not eligible: variadic ─────────────────────────────────────
(assert (not (fn/gpu-eligible? (fn [x & rest] x)))
  "variadic is not gpu-eligible")

# ── Not eligible: mutable captures ─────────────────────────────
(assert (not (fn/gpu-eligible?
  (let [@counter 0]
    (fn [] (assign counter (+ counter 1)) counter))))
  "mutable capture is not gpu-eligible")

# ── Not eligible: error signaling ──────────────────────────────
(assert (not (fn/gpu-eligible? (fn [x] (error "boom"))))
  "error is not gpu-eligible")

# ── Not eligible: I/O ──────────────────────────────────────────
(assert (not (fn/gpu-eligible? (fn [x] (eprintln x))))
  "I/O is not gpu-eligible")

# ── Non-closures return false ──────────────────────────────────
(assert (not (fn/gpu-eligible? 42)) "integer is not gpu-eligible")
(assert (not (fn/gpu-eligible? "hello")) "string is not gpu-eligible")
(assert (not (fn/gpu-eligible? nil)) "nil is not gpu-eligible")

# ── Signal analysis fix: fn/errors? ──────────────────────────
# These tests depend on compute_inferred_signal preserving
# SIG_ERROR on non-suspending functions (the bug was discarding it).
(assert (fn/errors? (fn [x] (error "boom")))
  "error-only function must report errors")
(assert (fn/errors? (fn [a b] (+ a b)))
  "arithmetic function has SIG_ERROR (type errors)")
(assert (not (fn/errors? (fn [x] x)))
  "identity function does not error")

(println "All GPU eligibility tests passed")
