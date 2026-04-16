## lib/gpu.lisp — GPU compute library
##
## Wraps the vulkan plugin and SPIR-V emitter.
##
## Usage:
##   (def gpu ((import "std/gpu")))
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

(fn []

(def plugin (import "plugin/vulkan"))
(def spv    ((import "std/spirv")))

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

## ── Compiler-generated GPU map ────────────────────────────────

## Split a rest-args list at the first keyword: return [inputs kw-struct].
## Keyword args follow (kw val kw val ...) after the positional args.
(defn split-kwargs [args]
  (var xs args)
  (var inputs @[])
  (while (and (not (empty? xs)) (not (keyword? (first xs))))
    (push inputs (first xs))
    (assign xs (rest xs)))
  (var kw @{})
  (let [[pairs (->array xs)]]
    (var i 0)
    (while (< i (length pairs))
      (put kw (pairs i) (pairs (+ i 1)))
      (assign i (+ i 2))))
  [inputs kw])

(defn gpu-map [f & rest-args]
  "Map a GPU-eligible function over N input arrays using compiler-generated SPIR-V.
   f: a GPU-eligible closure (pure arithmetic, no I/O or captures).
   rest-args: one or more input arrays of integers (must match fn arity),
              optionally followed by keyword args.
   Returns: array of results.

   Requires elle built with --features mlir.

   If f has been GIT'd (via (git f)), the cached SPIR-V is reused;
   otherwise SPIR-V is compiled on the fly via mlir/compile-spirv.

   Optional keyword args:
     :ctx       — Vulkan context (created if not given)
     :dtype     — :i64 (default), :i32, :u32, or :f32
     :wg-size   — workgroup size (default 256)

   Examples:
     (gpu:map (fn [x] (* x x)) [1 2 3 4])
     (gpu:map (fn [a b] (+ a b)) [1 2 3] [4 5 6])
     (gpu:map select [1 0 1 0] [10 20 30 40] [100 200 300 400])"
  (let* [[parts      (split-kwargs rest-args)]
         [inputs     (parts 0)]
         [opts       (parts 1)]
         [ctx        (or (get opts :ctx) (plugin:init))]
         [dtype      (or (get opts :dtype) :i64)]
         [wg-size    (or (get opts :wg-size) 256)]]
    (assert (= (length inputs) (fn/arity f))
            "gpu:map: number of input arrays must match function arity")
    (let* [[n          (length (inputs 0))]
           [_          (each inp in inputs
                        (assert (= (length inp) n)
                                "gpu:map: all input arrays must have the same length"))]
           [num-bufs   (+ (length inputs) 1)]
           [spirv      (if (fn/git? f) (disgit f) (mlir/compile-spirv f wg-size))]
           [shader     (plugin:shader ctx spirv num-bufs)]
           [wg-count   (+ (/ n wg-size) (if (= (rem n wg-size) 0) 0 1))]
           [elem-size  (if (= dtype :i64) 8 4)]
           [in-bufs    (map (fn [data] {:data data :usage :input :dtype dtype}) inputs)]
           [out-buf    {:size (* n elem-size) :usage :output}]
           [bufs       (push (concat in-bufs) out-buf)]
           [handle     (plugin:dispatch shader wg-count 1 1 bufs)]
           [_          (plugin:wait handle)]
           [result     (plugin:decode (plugin:collect handle) dtype)]]
      result)))

## ── Export ─────────────────────────────────────────────────────
{:init        gpu-init
 :compile     gpu-compile
 :load-shader gpu-load-shader
 :input       gpu-input
 :output      gpu-output
 :inout       gpu-inout
 :run         gpu-run
 :map         gpu-map})
