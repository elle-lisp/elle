# ── GPU tier agreement ────────────────────────────────────────────────
#
# Mirrors arithmetic.lisp cases through the :gpu tier via gpu:map.
# Requires: --features mlir, vulkan plugin built, GPU available.
# Skips gracefully if any component is missing.

# ── Check prerequisites ──────────────────────────────────────────────

(def has-mlir?
  (not (empty? (filter (fn [p] (= (get p :name) "mlir/compile-spirv"))
                       (compile/primitives)))))
(when (not has-mlir?)
  (println "SKIP gpu-diff: mlir/compile-spirv not available")
  (exit 0))

(def [has-vulkan? _] (protect (import "plugin/vulkan")))
(when (not has-vulkan?)
  (println "SKIP gpu-diff: vulkan plugin not available")
  (exit 0))

# plugin:init is only defined after the vulkan import above succeeds.
# This eval defers compilation past the guard.
(def [has-gpu? _] (protect (eval '(plugin:init))))
(when (not has-gpu?)
  (println "SKIP gpu-diff: no GPU available")
  (exit 0))

# ── Load harness and register GPU tier ───────────────────────────────

(def diff ((import "tests/diff/harness")))
(def gpu  ((import "std/gpu")))
(diff:with-gpu gpu)

# ── Arithmetic ───────────────────────────────────────────────────────

(defn add [a b] (+ a b))
(defn sub [a b] (- a b))
(defn mul [a b] (* a b))
(defn neg [x] (- 0 x))

(diff:assert-agree add 3 4)
(diff:assert-agree add -10 30)
(diff:assert-agree add 0 0)

(diff:assert-agree sub 10 3)
(diff:assert-agree sub -10 3)

(diff:assert-agree mul 3 7)
(diff:assert-agree mul -4 5)
(diff:assert-agree mul 0 100)

(diff:assert-agree neg 42)
(diff:assert-agree neg -7)
(diff:assert-agree neg 0)

# ── Branching ────────────────────────────────────────────────────────

(defn abs1 [x] (if (< x 0) (- 0 x) x))

(diff:assert-agree abs1 -7)
(diff:assert-agree abs1 5)
(diff:assert-agree abs1 0)

# ── Unary ────────────────────────────────────────────────────────────

(defn double [x] (* x 2))
(diff:assert-agree double 21)
(diff:assert-agree double -5)
(diff:assert-agree double 0)

(println "gpu-diff: OK")
