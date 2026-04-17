# ── compile/run-on :jit smoke test ─────────────────────────────────────
#
# Verifies the JIT tier of compile/run-on produces correct results.

(defn add [a b] (+ a b))
(assert (= (compile/run-on :jit add 3 4) 7) "jit add 3 4")
(assert (= (compile/run-on :jit add -10 30) 20) "jit add -10 30")
(assert (= (compile/run-on :jit add 0 0) 0) "jit add 0 0")

(defn mul-add [a b] (+ (* a b) a))
(assert (= (compile/run-on :jit mul-add 3 7) 24) "jit mul-add 3 7")

(defn abs1 [x] (if (< x 0) (- 0 x) x))
(assert (= (compile/run-on :jit abs1 -7) 7) "jit abs1 -7")
(assert (= (compile/run-on :jit abs1 5)  5) "jit abs1 5")

# JIT and bytecode must agree.
(assert (= (compile/run-on :jit add 11 22)
           (compile/run-on :bytecode add 11 22))
        "jit and bytecode agree on add")

(assert (= (compile/run-on :jit mul-add 9 11)
           (compile/run-on :bytecode mul-add 9 11))
        "jit and bytecode agree on mul-add")

(println "compile/run-on :jit OK")
