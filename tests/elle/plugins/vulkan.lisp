## Vulkan compute plugin tests
##
## Tests that we can:
##   - emit SPIR-V at runtime from Elle code (no offline compilation)
##   - load it, move data CPU → GPU → CPU
##   - dispatch a compute shader and collect correct results
##   - allocate and free GPU resources deterministically

(def [ok? gpu] (protect (import "std/gpu")))
(when (not ok?)
  (println "SKIP: vulkan plugin not built")
  (exit 0))

(def [gpu-ok? ctx] (protect (gpu:init)))
(when (not gpu-ok?)
  (println "SKIP: no Vulkan GPU available")
  (exit 0))

## ── Compile shader at runtime ─────────────────────────────────
(def shader (gpu:compile ctx 256 3 (fn [s]
  (let* [[id (s:global-id)]
         [a  (s:load 0 id)]
         [b  (s:load 1 id)]]
    (s:store 2 id (s:fadd a b))))))

## ── Vector addition: 256 elements = 1 workgroup ───────────────
(def n 256)
(def a (map float (range n)))
(def b (map (fn [i] (* 10.0 (float i))) (range n)))

(def result (gpu:run shader [1 1 1]
              [(gpu:input a) (gpu:input b) (gpu:output n)]))

(assert (= (length result) n) "all elements returned")
(assert (= (result 0) 0.0)     "0 + 0 = 0")
(assert (= (result 1) 11.0)    "1 + 10 = 11")
(assert (= (result 10) 110.0)  "10 + 100 = 110")
(assert (= (result 255) 2805.0) "255 + 2550 = 2805")

## ── Error: bad SPIR-V ─────────────────────────────────────────
(def [bad-ok? _] (protect (gpu:load-shader ctx "/dev/null" 1)))
(assert (not bad-ok?) "invalid SPIR-V errors")

(println "All Vulkan plugin tests passed")
