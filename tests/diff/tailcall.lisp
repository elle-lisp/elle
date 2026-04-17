# ── Tail-call agreement across tiers ──────────────────────────────────
#
# Covers: tail-call trampoline in invoke_closure_jit. These closures
# use tail recursion, which the :wasm tier rejects (no tail-call
# support in tiered WASM mode). Agreement is checked on :bytecode,
# :jit, and :mlir-cpu (if available).

(def diff ((import "tests/diff/harness")))

# ── GCD ──────────────────────────────────────────────────────────────

(defn gcd [a b]
  (if (= b 0) a (gcd b (rem a b))))

(diff:assert-agree gcd 12 8)
(diff:assert-agree gcd 100 75)
(diff:assert-agree gcd 17 13)
(diff:assert-agree gcd 0 5)
(diff:assert-agree gcd 1000000 3)

# ── Factorial via tail recursion ─────────────────────────────────────

(defn fact-iter [n acc]
  (if (= n 0) acc (fact-iter (- n 1) (* acc n))))

(defn fact [n] (fact-iter n 1))

(diff:assert-agree fact 0)
(diff:assert-agree fact 1)
(diff:assert-agree fact 5)
(diff:assert-agree fact 10)

# ── Iterative sum via tail recursion ─────────────────────────────────

(defn sum-iter [n acc]
  (if (= n 0) acc (sum-iter (- n 1) (+ acc n))))

(defn sum-to [n] (sum-iter n 0))

(diff:assert-agree sum-to 0)
(diff:assert-agree sum-to 1)
(diff:assert-agree sum-to 100)
(diff:assert-agree sum-to 1000)

# ── Mutual recursion: even?/odd? ─────────────────────────────────────

(defn my-even? [n]
  (if (= n 0) 1 (my-odd? (- n 1))))

(defn my-odd? [n]
  (if (= n 0) 0 (my-even? (- n 1))))

(diff:assert-agree my-even? 0)
(diff:assert-agree my-even? 1)
(diff:assert-agree my-even? 10)
(diff:assert-agree my-odd? 7)

(println "tailcall: OK")
