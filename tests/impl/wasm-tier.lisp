(elle/epoch 13)
# audited: 2026-09-29
# The tiered WASM backend returns the right values for a recursive closure, a list walk and a struct read.
# docs/impl/wasm.md
#
# The file tests the tiered backend only when the process runs under it, as
# in `elle --wasm=11 tests/impl/wasm-tier.lisp` on a wasm build. Anywhere else
# it gates itself, because every assertion below is also a language claim that
# the language suite already checks on the build's own tier.

(def _tiered
  (let [policy (if (has-key? (vm/config) :wasm) (vm/config :wasm) nil)]
    (if (= policy :lazy)
      true
      (error (struct :error :gated :reason "not running under --wasm=N")))))

# A recursive closure, called often enough to pass the tier's threshold.
(defn fib [n]
  (if (< n 2)
    n
    (+ (fib (- n 1)) (fib (- n 2)))))

(assert (= (fib 20) 6765) "wasm-tier: fib")

(defn my-sum [xs]
  (if (empty? xs)
    0
    (+ (first xs) (my-sum (rest xs)))))

(assert (= (my-sum (list 1 2 3 4 5)) 15) "wasm-tier: list sum")

# Keyword constants reach the module through its constant pool.
(let [s {:x 42 :y 99}]
  (assert (= (get s :x) 42) "wasm-tier: struct read"))

(println "wasm-tier: ok")
