## lib/gpu.lisp — GPU compute library
##
## Wraps the vulkan plugin and SPIR-V emitter.
##
## Usage:
##   (def gpu (import "std/gpu"))
##   (def ctx (gpu:init))
##
##   ## compile shader at runtime — no offline tools needed
##   (def shader (gpu:compile ctx 256 3 (fn [s]
##     (let [id (s:global-id)
##           a  (s:load 0 id)
##           b  (s:load 1 id)]
##       (s:store 2 id (s:fadd a b))))))
##
##   (def result (gpu:run shader [4 1 1]
##                  [(gpu:input a) (gpu:input b) (gpu:output 1024)]))

(def plugin (import "plugin/vulkan"))
(def spv    (import "std/spirv"))

## ── Context ────────────────────────────────────────────────────

(defn gpu-init []
  "Initialize GPU context."
  (plugin:init))

## ── Shader compilation ─────────────────────────────────────────

(defn gpu-compile [ctx local-size-x num-buffers body-fn]
  "Compile a compute shader from Elle code. No GLSL, no offline tools.
   local-size-x: workgroup size (threads per workgroup).
   num-buffers: number of f32 storage buffer bindings.
   body-fn: (fn [s] ...) receives shader builder context s."
  (let [[bytecode (spv:compute local-size-x num-buffers body-fn plugin:f32-bits)]]
    (plugin:shader ctx bytecode num-buffers)))

(defn gpu-load-shader [ctx path num-buffers]
  "Load a pre-compiled SPIR-V shader from a file path."
  (plugin:shader ctx path num-buffers))

## ── Buffer specs ───────────────────────────────────────────────

(defn gpu-input [data]
  "Mark an array as input-only (uploaded to GPU, not read back)."
  {:data data :usage :input})

(defn gpu-output [n]
  "Declare an output-only buffer of n f32 elements."
  {:size (* n 4) :usage :output})

(defn gpu-inout [data]
  "Mark an array as input+output (uploaded to GPU, read back after compute)."
  {:data data :usage :inout})

## ── Dispatch ───────────────────────────────────────────────────

(defn gpu-run [shader workgroups buffers]
  "Dispatch a compute shader and return decoded f32 result arrays.
   workgroups: [x y z] dispatch dimensions.
   buffers: array of specs from gpu-input, gpu-output, gpu-inout.
   Fiber suspends on GPU fence fd — no thread pool thread consumed."
  (let* [[handle (plugin:dispatch shader
                   (workgroups 0) (workgroups 1) (workgroups 2)
                   buffers)]
         [_ (plugin:wait handle)]]
    (plugin:decode (plugin:collect handle) :f32)))

## ── Export ─────────────────────────────────────────────────────
{:init        gpu-init
 :compile     gpu-compile
 :load-shader gpu-load-shader
 :input       gpu-input
 :output      gpu-output
 :inout       gpu-inout
 :run         gpu-run}
