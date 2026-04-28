(elle/epoch 9)
## demos/gpu/mandelbrot.lisp — GPU Mandelbrot set
##
## Computes a Mandelbrot set on the GPU using a SPIR-V compute shader
## built at runtime. No GLSL, no offline tools.
##
## The shader uses loops and local variables (Phase 1a builder extensions).
## Output is iteration counts as f32 values.

(def vk (import "plugin/vulkan"))
(def gpu ((import "std/gpu") :vulkan vk))

(def ctx (gpu:init))
(println "GPU initialized")

## ── Parameters ────────────────────────────────────────────────
(def WIDTH 512)
(def HEIGHT 512)
(def MAX-ITER 256)
(def N (* WIDTH HEIGHT))

## Viewport: centered on (-0.5, 0), range [-2, 1] x [-1.5, 1.5]
(def X-MIN -2.0)
(def X-MAX 1.0)
(def Y-MIN -1.5)
(def Y-MAX 1.5)

## ── Generate coordinate arrays on CPU ─────────────────────────
(def dx (/ (- X-MAX X-MIN) (float WIDTH)))
(def dy (/ (- Y-MAX Y-MIN) (float HEIGHT)))

(def cx
  (map (fn [i]
         (let [px (% i WIDTH)]
           (float (+ X-MIN (* (float px) dx)))))
       (range N)))

(def cy
  (map (fn [i]
         (let [py (int (/ i WIDTH))]
           (float (+ Y-MIN (* (float py) dy)))))
       (range N)))

## ── Compile mandelbrot shader ─────────────────────────────────
(def shader
  (gpu:compile ctx
               256
               3
               (fn [s]
                 (let* [id (s:global-id)
                        cx (s:load 0 id)
                        cy (s:load 1 id)
                        zr (s:var-f)
                        zi (s:var-f)
                        iter (s:var-u)
                        max-iter (s:const-u MAX-ITER)
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
                   (s:branch hdr)  ## ── loop header ──────────────────────────
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
                     (s:branch-cond go body done))  ## ── loop body: z = z² + c ────────────────
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
                     (s:branch cont))  ## ── continue target ──────────────────────
                   (s:begin-block cont)
                   (s:branch hdr)  ## ── exit: store iteration count ──────────
                   (s:begin-block done)
                   (s:store 2 id (s:u2f (s:load-var iter)))))))

(println "Shader compiled (SPIR-V emitted at runtime)")

## ── Dispatch ──────────────────────────────────────────────────
(def wg-count (int (ceil (/ (float N) 256.0))))

(def t0 (clock/monotonic))
(def result
  (gpu:run shader [wg-count 1 1] [(gpu:input cx) (gpu:input cy) (gpu:output N)]))
(def elapsed (* 1000.0 (- (clock/monotonic) t0)))

(println "Computed" N "pixels in" elapsed "ms")
(println "  Workgroups:" wg-count "x 256 =" (* wg-count 256) "threads")

## ── Validate ──────────────────────────────────────────────────
## Origin (0, 0) is inside the main cardioid: should reach max iterations
(def origin-idx
  (+ (int (/ (* WIDTH (- 0.0 Y-MIN)) (- Y-MAX Y-MIN)))
     (int (/ (* WIDTH (- 0.0 X-MIN)) (- X-MAX X-MIN)))))
## Approximate center pixel
(def center-idx (+ (* (/ HEIGHT 2) WIDTH) (/ WIDTH 2)))

(println "  Center pixel ("
         center-idx
         "): "
         (int (result center-idx))
         "iterations")

## Point well outside the set: (2, 2) → should escape in 1 iteration
(def far-px (int (/ (* WIDTH (- 2.0 X-MIN)) (- X-MAX X-MIN))))
(def far-py (int (/ (* HEIGHT (- 2.0 Y-MIN)) (- Y-MAX Y-MIN))))
(def far-idx (+ (* far-py WIDTH) far-px))
(when (and (>= far-idx 0) (< far-idx N))
  (println "  Far point (" far-idx "): " (int (result far-idx)) "iterations")
  (assert (< (result far-idx) 5.0) "far point escapes quickly"))

## Check that we got the right number of results
(assert (= (length result) N) "correct result count")

## Sanity: center of set should have high iteration count
(assert (> (result center-idx) 200.0) "center of set reaches high iterations")

## ── Summary ───────────────────────────────────────────────────
(def inside (length (filter (fn [v] (= v (float MAX-ITER))) result)))
(println "  Pixels inside set:" inside "/" N)
(println "GPU Mandelbrot complete")
