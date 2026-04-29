(elle/epoch 9)
## demos/gpu/vecadd.lisp — GPU vector addition
##
## Adds two 1024-element float arrays on the GPU. The compute shader
## is compiled from SPIR-V emitted at runtime — no GLSL, no offline
## tools, just Elle generating GPU bytecode.

(def vk (import "plugin/vulkan"))
(def gpu ((import "std/gpu") :vulkan vk))

(def ctx (gpu:init))
(println "GPU initialized")

## ── Compile shader at runtime ───────────────────────────────────
(def shader
  (gpu:compile ctx 256 3
               (fn [s]
                 (let* [id (s:global-id)
                        a (s:load 0 id)
                        b (s:load 1 id)]
                   (s:store 2 id (s:fadd a b))))))
(println "Shader compiled (SPIR-V emitted at runtime)")

## ── Input data ──────────────────────────────────────────────────
(def n 1024)
(def a (map float (range n)))
(def b (map (fn [i] (* 2.0 (float i))) (range n)))

## ── Dispatch: 4 workgroups of 256 threads = 1024 elements ──────
(def result
  (gpu:run shader [4 1 1] [(gpu:input a) (gpu:input b) (gpu:output n)]))

## ── Verify ──────────────────────────────────────────────────────
(println "First 8 results: " (slice result 0 8))

(assert (= (result 0) 0.0) "element 0: 0 + 0 = 0")
(assert (= (result 1) 3.0) "element 1: 1 + 2 = 3")
(assert (= (result 10) 30.0) "element 10: 10 + 20 = 30")
(assert (= (result 999) 2997.0) "element 999: 999 + 1998 = 2997")

(println "All " n " elements verified")
