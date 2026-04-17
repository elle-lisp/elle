# ── Mixed-type local slot agreement ────────────────────────────────
#
# Covers the case where a single local variable receives different
# scalar types in different branches. The MLIR lowering must either
# handle this correctly or reject the closure (falling through to
# bytecode/JIT which handles it natively).
#
# This is a regression test for a correctness bug: slot_types in
# lower.rs was a single map overwritten at each StoreLocal. When
# different branches stored different types to the same slot, the
# LoadLocal picked the type from whichever StoreLocal was lowered
# last — not from the runtime control-flow path.

(def diff ((import "tests/diff/harness")))

# ── Int/float branch: same var, different types per branch ───────

(defn mixed-branch [x]
  (var s 0)
  (if (> x 0)
    (assign s 1.5)
    (assign s 2))
  s)

(diff:assert-agree mixed-branch 1)
(diff:assert-agree mixed-branch 0)
(diff:assert-agree mixed-branch -1)

# ── Float/int branch (reversed order) ───────────────────────────

(defn mixed-branch-rev [x]
  (var s 0.0)
  (if (> x 0)
    (assign s 2)
    (assign s 1.5))
  s)

(diff:assert-agree mixed-branch-rev 1)
(diff:assert-agree mixed-branch-rev 0)
(diff:assert-agree mixed-branch-rev -1)

# ── Sequentially reassigned type ────────────────────────────────

(defn reassign-type [x]
  (var s 0)
  (assign s 1.5)
  (assign s (+ s x))
  s)

(diff:assert-agree reassign-type 1.0)
(diff:assert-agree reassign-type 0.0)

(println "mixed-type locals: OK")
