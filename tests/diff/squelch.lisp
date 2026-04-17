# ── Squelch enforcement across tiers ──────────────────────────────────
#
# Squelch is a runtime transform: (squelch f :signal) returns a new
# closure that converts :signal emissions into :signal-violation errors.
# Enforcement must be consistent across tiers — if bytecode catches
# a squelched signal, JIT must too.
#
# This is a known weak link: compile/run-on bypasses call_inner where
# squelch enforcement lives. These tests verify the fix.

# ── Squelch on a silent closure (no signal emitted) ──────────────────
#
# Squelching a closure that doesn't emit the squelched signal should
# have no effect — the closure runs normally on every tier.

(def diff ((import "tests/diff/harness")))

(defn add [a b] (+ a b))
(def squelched-add (squelch add :yield))

# A squelched but silent closure should agree across tiers.
(diff:assert-agree squelched-add 3 7)
(diff:assert-agree squelched-add -10 20)
(diff:assert-agree squelched-add 0 0)

# ── Squelch mask preserved through tiers ─────────────────────────────
#
# The squelch mask is on the closure, not the code. All tiers should
# see the same squelch_mask when dispatching.

(defn mul [a b] (* a b))
(def squelched-mul (squelch mul |:yield :io|))

(diff:assert-agree squelched-mul 3 4)
(diff:assert-agree squelched-mul -5 6)

# ── Squelch on a tail-recursive closure ──────────────────────────────
#
# Tail calls accumulate squelch masks in the trampoline. A squelched
# tail-recursive closure should work on all tiers that support tail
# calls (bytecode, jit).

(defn sum-iter [n acc]
  (if (= n 0) acc (sum-iter (- n 1) (+ acc n))))
(def squelched-sum (squelch sum-iter :yield))

(diff:assert-agree squelched-sum 0 0)
(diff:assert-agree squelched-sum 10 0)
(diff:assert-agree squelched-sum 100 0)

# ── Squelch enforcement on compile/run-on ────────────────────────────
#
# The critical test: a closure that DOES emit a squelched signal.
# compile/run-on should enforce the squelch mask and produce a
# :signal-violation error, not let the signal through.
#
# Note: yielding closures are rejected by :wasm and :mlir-cpu (not
# GPU-eligible), so we test :bytecode and :jit only.

(defn yielder [x] (yield x) (+ x 1))
(def squelched-yielder (squelch yielder :yield))

# Bytecode tier: squelch must convert :yield to :signal-violation.
(def [bc-ok? bc-err] (protect (compile/run-on :bytecode squelched-yielder 42)))
(assert (not bc-ok?)
        (string "squelched yielder on :bytecode must error, got: " bc-err))
(assert (and (struct? bc-err) (= (get bc-err :error) :signal-violation))
        (string "squelched yielder on :bytecode must be :signal-violation, got: " bc-err))

# JIT tier: same enforcement.
(def [jit-ok? jit-err] (protect (compile/run-on :jit squelched-yielder 42)))
(assert (not jit-ok?)
        (string "squelched yielder on :jit must error, got: " jit-err))
(assert (and (struct? jit-err) (= (get jit-err :error) :signal-violation))
        (string "squelched yielder on :jit must be :signal-violation, got: " jit-err))

(println "squelch: OK")
