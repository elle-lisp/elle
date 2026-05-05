(elle/epoch 10)
# Test tiered WASM compilation.
# Run with: ELLE_WASM_TIER=1 elle tests/elle/wasm-tier.lisp

# ── fib: recursive closure, should be WASM-compiled after threshold ──

(defn fib [n]
  (if (< n 2)
    n
    (+ (fib (- n 1)) (fib (- n 2)))))

(let [result (fib 20)]
  (if (= result 6765)
    (println "wasm-tier fib: PASS")
    (println "wasm-tier fib: FAIL expected 6765 got " result)))

# ── higher-order: map uses a lambda, should stay on bytecode VM ──

(defn my-sum [xs]
  (if (empty? xs)
    0
    (+ (first xs) (my-sum (rest xs)))))

(let [result (my-sum (list 1 2 3 4 5))]
  (if (= result 15)
    (println "wasm-tier sum: PASS")
    (println "wasm-tier sum: FAIL expected 15 got " result)))

# ── struct access: keyword constants through const pool ──

(let [s {:x 42 :y 99}]
  (if (= (get s :x) 42)
    (println "wasm-tier struct: PASS")
    (println "wasm-tier struct: FAIL")))
