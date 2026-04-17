# ── Constant and zero-arg closure agreement ──────────────────────────
#
# Exercises corner cases: zero-arg LIR, unused params, constant returns.
# These are trivial closures that could diverge between tiers if the
# lowering mishandles zero-arg calling conventions or dead parameters.

(def diff ((import "tests/diff/harness")))

# ── Constant return, no args ─────────────────────────────────────────

(defn always-42 [] 42)
(diff:assert-agree always-42)

(defn always-zero [] 0)
(diff:assert-agree always-zero)

(defn always-neg [] -1)
(diff:assert-agree always-neg)

# ── Ignores argument, returns constant ───────────────────────────────

(defn ignore-arg [x] 0)
(diff:assert-agree ignore-arg 99)
(diff:assert-agree ignore-arg -1)
(diff:assert-agree ignore-arg 0)

(defn ignore-two [x y] 7)
(diff:assert-agree ignore-two 1 2)
(diff:assert-agree ignore-two 0 0)

# ── Identity ─────────────────────────────────────────────────────────

(defn identity-fn [x] x)
(diff:assert-agree identity-fn 0)
(diff:assert-agree identity-fn 42)
(diff:assert-agree identity-fn -100)

# ── Select first / second ───────────────────────────────────────────

(defn fst [a b] a)
(diff:assert-agree fst 1 2)
(diff:assert-agree fst 0 99)

(defn snd [a b] b)
(diff:assert-agree snd 1 2)
(diff:assert-agree snd 99 0)

(println "const: OK")
