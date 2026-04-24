(elle/epoch 9)
# ── gpu-select: git + disgit + N-ary gpu:map ──────────────────────────
#
# Tests the full pipeline:
#   (git f) → cache SPIR-V on template
#   (fn/git? f) → true
#   (disgit f) → SPIR-V bytes
#   (gpu:map f a b c) → N-ary dispatch on GPU
#
# Requires: --features mlir, vulkan plugin built, GPU available.
# Skips gracefully if any component is missing.

# ── Check prerequisites ──────────────────────────────────────────────

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

# ── Test: git + fn/git? + disgit ─────────────────────────────────────

(defn select [flag a b]
  (silence)
  (muffle :error)
  (if flag a b))

(assert (not (fn/git? select)) "select is not GIT'd initially")

(git select)

(assert (fn/git? select) "select is GIT'd after (git select)")

(def spirv-bytes (disgit select))
(assert (bytes? spirv-bytes) "disgit returns bytes")
(assert (> (length spirv-bytes) 0) "SPIR-V bytes are non-empty")

# git is idempotent
(git select)
(assert (fn/git? select) "still GIT'd after second git call")

# ── Test: N-ary gpu:map with select ──────────────────────────────────

(def result (gpu:map select [1 0 1 0] [10 20 30 40] [100 200 300 400] :ctx ctx))
(assert (= (length result) 4) "gpu:map select: same length")
(assert (= (result 0) 10) "flag=1 → a: (select 1 10 100) = 10")
(assert (= (result 1) 200) "flag=0 → b: (select 0 20 200) = 200")
(assert (= (result 2) 30) "flag=1 → a: (select 1 30 300) = 30")
(assert (= (result 3) 400) "flag=0 → b: (select 0 40 400) = 400")

# ── Test: truthiness on GPU ──────────────────────────────────────────
# value 2 must be truthy (cmpi ne, not trunci LSB)

(defn gpu-truthy [x]
  (silence)
  (muffle :error)
  (if x 1 0))

(git gpu-truthy)
(def result2 (gpu:map gpu-truthy [0 1 2 3 -1 256] :ctx ctx))
(assert (= (result2 0) 0) "GPU truthiness: 0 is false")
(assert (= (result2 1) 1) "GPU truthiness: 1 is true")
(assert (= (result2 2) 1) "GPU truthiness: 2 is true (not trunci)")
(assert (= (result2 3) 1) "GPU truthiness: 3 is true")
(assert (= (result2 4) 1) "GPU truthiness: -1 is true")
(assert (= (result2 5) 1) "GPU truthiness: 256 is true (LSB=0)")

# ── Test: 2-ary gpu:map (add) ────────────────────────────────────────

(defn gpu-add [a b]
  (silence)
  (muffle :error)
  (+ a b))

(git gpu-add)
(def result3 (gpu:map gpu-add [1 2 3 4] [10 20 30 40] :ctx ctx))
(assert (= (result3 0) 11) "gpu-add: 1+10=11")
(assert (= (result3 1) 22) "gpu-add: 2+20=22")
(assert (= (result3 2) 33) "gpu-add: 3+30=33")
(assert (= (result3 3) 44) "gpu-add: 4+40=44")

(println "all gpu-select tests passed")
