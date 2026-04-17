# ── Local-slot agreement across tiers ─────────────────────────────────
#
# Covers: LirInstr::{StoreLocal,LoadLocal}, let bindings, var/assign,
# cross-block local stores (the memref-vs-SSA boundary in MLIR).

(def diff ((import "std/differential")))

# Single-block let.
(defn poly1 [x]
  (let [[s (* x x)]]
    (+ s 1)))
(diff:assert-agree poly1 5)
(diff:assert-agree poly1 -3)
(diff:assert-agree poly1 0)

# Sequential let* dependency.
(defn pyth [a b]
  (let* [[s1 (* a a)]
         [s2 (* b b)]
         [t  (+ s1 s2)]]
    t))
(diff:assert-agree pyth 3 4)
(diff:assert-agree pyth 5 12)
(diff:assert-agree pyth 0 0)

# Local mutated across branches — exercises memref.alloca / cross-block
# store on the MLIR side.
(defn clamp [x lo hi]
  (var v x)
  (when (< v lo) (assign v lo))
  (when (> v hi) (assign v hi))
  v)
(diff:assert-agree clamp 5 0 10)
(diff:assert-agree clamp -5 0 10)
(diff:assert-agree clamp 100 0 10)
(diff:assert-agree clamp 7 0 10)

(println "locals: OK")
