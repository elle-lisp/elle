(elle/epoch 9)
## Vulkan compute plugin tests
##
## Tests that we can:
##   - emit SPIR-V at runtime from Elle code (no offline compilation)
##   - load it, move data CPU → GPU → CPU
##   - dispatch a compute shader and collect correct results
##   - allocate and free GPU resources deterministically

(def [ok? gpu] (protect ((import "std/gpu"))))
(when (not ok?)
  (println "SKIP: vulkan plugin not built")
  (exit 0))

(def [gpu-ok? ctx] (protect (gpu:init)))
(when (not gpu-ok?)
  (println "SKIP: no Vulkan GPU available")
  (exit 0))

## ── Compile shader at runtime ─────────────────────────────────
(def shader
  (gpu:compile ctx 256 3
    (fn [s]
      (let* [id (s:global-id)
             a (s:load 0 id)
             b (s:load 1 id)]
        (s:store 2 id (s:fadd a b))))))

## ── Vector addition: 256 elements = 1 workgroup ───────────────
(def n 256)
(def a (map float (range n)))
(def b (map (fn [i] (* 10.0 (float i))) (range n)))

(def result
  (gpu:run shader [1 1 1] [(gpu:input a) (gpu:input b) (gpu:output n)]))

(assert (= (length result) n) "all elements returned")
(assert (= (result 0) 0.0) "0 + 0 = 0")
(assert (= (result 1) 11.0) "1 + 10 = 11")
(assert (= (result 10) 110.0) "10 + 100 = 110")
(assert (= (result 255) 2805.0) "255 + 2550 = 2805")

## ── Mandelbrot: loops + local variables ──────────────────────
(def mandel-shader
  (gpu:compile ctx 256 3
    (fn [s]
      (let* [id (s:global-id)
             cx (s:load 0 id)
             cy (s:load 1 id)
             zr (s:var-f)
             zi (s:var-f)
             iter (s:var-u)
             max-iter (s:const-u 64)
             four (s:const-f 4.0)
             zero-f (s:const-f 0.0)
             zero-u (s:const-u 0)
             one-u (s:const-u 1)
             hdr (s:block)
             body (s:block)
             cont (s:block)
             done (s:block)]
        (s:store-var zr zero-f)
        (s:store-var zi zero-f)
        (s:store-var iter zero-u)
        (s:branch hdr)
        (s:begin-block hdr)
        (let* [r (s:load-var zr)
               i (s:load-var zi)
               r2 (s:fmul r r)
               i2 (s:fmul i i)
               mag (s:fadd r2 i2)
               ok (s:flt mag four)
               n (s:load-var iter)
               lim (s:slt n max-iter)
               go (s:logical-and ok lim)]
          (s:loop-merge done cont)
          (s:branch-cond go body done))
        (s:begin-block body)
        (let* [r (s:load-var zr)
               i (s:load-var zi)
               ri (s:fmul r i)
               r2 (s:fmul r r)
               i2 (s:fmul i i)
               nr (s:fadd (s:fsub r2 i2) cx)
               ni (s:fadd (s:fadd ri ri) cy)]
          (s:store-var zr nr)
          (s:store-var zi ni)
          (s:store-var iter (s:iadd (s:load-var iter) one-u))
          (s:branch cont))
        (s:begin-block cont)
        (s:branch hdr)
        (s:begin-block done)
        (s:store 2 id (s:u2f (s:load-var iter)))))))

## 4 test points: origin (inside set), far point (outside), real axis, edge
(def mandel-cx [0.0 10.0 -2.0 0.25])
(def mandel-cy [0.0 10.0 0.0 0.0])
(def mandel-result
  (gpu:run mandel-shader [1 1 1]
    [(gpu:input mandel-cx) (gpu:input mandel-cy) (gpu:output 4)]))

(assert (= (mandel-result 0) 64.0) "origin reaches max iterations")
(assert (< (mandel-result 1) 5.0) "far point escapes quickly")
(assert (< (mandel-result 2) 5.0) "c=-2 escapes quickly")
(assert (> (mandel-result 3) 10.0) "c=0.25 near boundary iterates")

## ── Bitwise ops: ior, iand, ishl, ishr, bitcast, umin ────────
(def bitwise-shader
  (gpu:compile ctx 256 2
    (fn [s]
      (let* [id (s:global-id)
             a (s:f2u (s:load 0 id))
             b (s:const-u 8)  ## test ior: a | 0xFF = 0xFF for a in [0..8]
             or-r (s:ior a (s:const-u 0xFF))  ## test ishl: a << 8
             shl-r (s:ishl a b)  ## test umin: min(a, 3)
             ## pack results:
             ## (or << 16) | (shl-low-byte << 8) | min
             ## (all values fit in a byte
             ## for a in [0..8])
             min-r (s:umin a (s:const-u 3))
             shl-byte (s:iand shl-r (s:const-u 0xFF00))
             packed (s:ior (s:ishl or-r (s:const-u 16)) (s:ior shl-byte min-r))]
        (s:store 1 id (s:u2f packed))))))

(def bitwise-input (map float (range 8)))
(def bitwise-result
  (gpu:run bitwise-shader [1 1 1] [(gpu:input bitwise-input) (gpu:output 256)]))

# a=0: or=0xFF, shl=0x0000, min=0 → packed = 0x00FF0000
(assert (= (bitwise-result 0) (float 0x00FF0000)) "bitwise: a=0")
# a=1: or=0xFF, shl=0x0100, min=1 → packed = 0x00FF0101
(assert (= (bitwise-result 1) (float 0x00FF0101)) "bitwise: a=1")
# a=5: or=0xFF, shl=0x0500, min=3 → packed = 0x00FF0503
(assert (= (bitwise-result 5) (float 0x00FF0503)) "bitwise: a=5")

## ── Bitcast u32→f32 round-trip ──────────────────────────────
(def bitcast-shader
  (gpu:compile ctx 256 2
    (fn [s]
      (let* [id (s:global-id)
             val (s:const-u 0x42280000)  # IEEE 754 bits for 42.0
             as-f (s:bitcast-u2f val)]
        (s:store 1 id as-f)))))

(def bitcast-result
  (gpu:run bitcast-shader [1 1 1] [(gpu:input [0.0]) (gpu:output 256)]))
(assert (= (bitcast-result 0) 42.0) "bitcast 0x42280000 = 42.0")

## ── Error: bad SPIR-V ─────────────────────────────────────────
(def [bad-ok? _] (protect (gpu:load-shader ctx "/dev/null" 1)))
(assert (not bad-ok?) "invalid SPIR-V errors")

(println "All Vulkan plugin tests passed")
