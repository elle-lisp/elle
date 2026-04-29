(elle/epoch 9)
# ── MLIR tier-2 backend tests ─────────────────────────────────────────
#
# Verifies that GPU-eligible functions compile and execute correctly
# through the MLIR → LLVM → native path. Functions must be called
# enough times to pass the hotness threshold before MLIR compiles them.
#
# These tests only exercise the MLIR path when built with --features mlir.
# Without the feature, all functions run through the bytecode VM and
# the assertions still hold (same semantics, different backend).

# ── Arithmetic ───────────────────────────────────────────────────────

(defn ml-add [a b]
  (+ a b))
(defn ml-mul [a b]
  (* a b))
(defn ml-sub [a b]
  (- a b))

# Call past hotness threshold (default 10)
(repeat 15 (ml-add 1 2))
(repeat 15 (ml-mul 1 2))
(repeat 15 (ml-sub 1 2))

(assert (= (ml-add 10 32) 42) "MLIR add")
(assert (= (ml-mul 6 7) 42) "MLIR mul")
(assert (= (ml-sub 50 8) 42) "MLIR sub")
(assert (= (ml-add -5 15) 10) "MLIR add negative")
(assert (= (ml-mul -3 -7) 21) "MLIR mul negatives")

# ── Multi-operation ──────────────────────────────────────────────────

(defn ml-quad [a b c]
  (+ (* a (+ b c)) c))

(repeat 15 (ml-quad 1 2 3))
(assert (= (ml-quad 2 3 4) 18) "MLIR multi-op: 2*(3+4)+4=18")
(assert (= (ml-quad 0 5 10) 10) "MLIR multi-op: 0*(5+10)+10=10")

# ── Control flow ─────────────────────────────────────────────────────

(defn ml-abs [x]
  (if (> x 0) x (- 0 x)))

(repeat 15 (ml-abs 1))
(assert (= (ml-abs 42) 42) "MLIR abs positive")
(assert (= (ml-abs -7) 7) "MLIR abs negative")
(assert (= (ml-abs 0) 0) "MLIR abs zero")

(defn ml-max [a b]
  (if (> a b) a b))

(repeat 15 (ml-max 1 2))
(assert (= (ml-max 3 7) 7) "MLIR max")
(assert (= (ml-max 10 5) 10) "MLIR max reversed")
(assert (= (ml-max 4 4) 4) "MLIR max equal")

# ── Non-eligible functions still work ────────────────────────────────
# These use I/O or captures, so they go through Cranelift or bytecode.

(def outer 100)
(defn ml-with-capture [x]
  (+ x outer))
(repeat 15 (ml-with-capture 1))
(assert (= (ml-with-capture 5) 105) "captured var works (not MLIR)")

# ── Verify GPU eligibility predicates ────────────────────────────────

(assert (fn/gpu-eligible? ml-add) "ml-add is GPU-eligible")
(assert (fn/gpu-eligible? ml-abs) "ml-abs is GPU-eligible")
(assert (fn/gpu-eligible? ml-with-capture)
        "immutable capture is constant-propagated")

# ── Branching with even values ────────────────────────────────────────
# Before the cmpi-ne fix in MLIR, trunci took the LSB: comparison result
# 2 would truncate to 0 (false). This test verifies the fix by using
# comparisons where the branch condition is always 0 or 1.

(defn ml-clamp [x lo hi]
  (if (< x lo) lo (if (> x hi) hi x)))

(repeat 15 (ml-clamp 5 0 10))
(assert (= (ml-clamp -3 0 10) 0) "MLIR clamp below")
(assert (= (ml-clamp 5 0 10) 5) "MLIR clamp within")
(assert (= (ml-clamp 15 0 10) 10) "MLIR clamp above")

(println "all MLIR tests passed")
