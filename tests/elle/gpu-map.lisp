# ── gpu/map: compiler-generated GPU compute ───────────────────────────
#
# Tests the full pipeline: Elle closure → MLIR → SPIR-V → Vulkan dispatch.
# Requires: --features mlir, vulkan plugin built, GPU available.
# Skips gracefully if any component is missing.

# ── Check prerequisites ──────────────────────────────────────────────

# mlir/compile-spirv must exist (--features mlir)
(def has-mlir?
  (not (empty? (filter (fn [p] (= (get p :name) "mlir/compile-spirv"))
                       (compile/primitives)))))
(when (not has-mlir?)
  (println "SKIP: mlir/compile-spirv not available (build with --features mlir)")
  (exit 0))

# Vulkan plugin must be loadable
(def [has-vulkan? plugin] (protect (import "plugin/vulkan")))
(when (not has-vulkan?)
  (println "SKIP: vulkan plugin not available")
  (exit 0))

# GPU must be initializable
(def [has-gpu? ctx] (protect (plugin:init)))
(when (not has-gpu?)
  (println "SKIP: no GPU available")
  (exit 0))

# ── Load gpu library ─────────────────────────────────────────────────

(def gpu ((import "std/gpu")))

# ── Test: double each element ────────────────────────────────────────

(def result (gpu:map (fn [x] (* x 2)) [1 2 3 4 5 6 7 8] :ctx ctx))
(assert (= (length result) 8) "same length")
(assert (= (result 0) 2) "1*2=2")
(assert (= (result 3) 8) "4*2=8")
(assert (= (result 7) 16) "8*2=16")

# ── Test: add constant ──────────────────────────────────────────────

(def result2 (gpu:map (fn [x] (+ x 10)) [0 5 10 15] :ctx ctx))
(assert (= (result2 0) 10) "0+10=10")
(assert (= (result2 1) 15) "5+10=15")
(assert (= (result2 3) 25) "15+10=25")

# ── Test: negate ─────────────────────────────────────────────────────

(def result3 (gpu:map (fn [x] (- 0 x)) [1 -2 3 -4] :ctx ctx))
(assert (= (result3 0) -1) "negate 1")
(assert (= (result3 1) 2) "negate -2")

# ── Test: i64 (values that overflow i32) ─────────────────────────────

(def big (+ 1 (bit/shl 1 40)))
(def result4 (gpu:map (fn [x] (* x 2)) [1 big] :ctx ctx))
(assert (= (result4 0) 2) "i64: 1*2=2")
(assert (= (result4 1) (* big 2)) "i64: big*2 no truncation")

(println "all gpu/map tests passed")
