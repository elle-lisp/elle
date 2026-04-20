#!/usr/bin/env elle
(elle/epoch 8)

# Mandelbrot Explorer — GTK4 + Cairo, GPU-accelerated with CPU fallback
#
# Controls:
#   Left click    zoom in (2x) at cursor
#   Right click   zoom out (2x) at cursor
#   Scroll        zoom in / out
#   Arrow keys    pan
#   R             reset view
#   +/=           double max iterations
#   -             halve max iterations
#   Escape / Q    quit

# ── Libraries ─────────────────────────────────────────────────────

(def gtk ((import "std/gtk4")))
(def cr  ((import "std/cairo")))
(def b   ((import "std/gtk4/bind")))

(def [vk-ok? vk-plugin] (protect (import "plugin/vulkan")))
(def [gpu-ok? gpu] (when vk-ok? (protect ((import "std/gpu") :vulkan vk-plugin))))
(def gpu-ctx
  (when gpu-ok?
    (let [[ok? ctx] (protect (gpu:init))]
      (when ok? (println "GPU: enabled") ctx))))

(when (not gpu-ctx) (println "GPU: not available, using CPU"))

# ── Constants ─────────────────────────────────────────────────────

(def GPU-SCALE (if gpu-ctx 2 1))
(def WIDTH     (* 800 GPU-SCALE))
(def HEIGHT    (* 600 GPU-SCALE))
(def BPP       4)
(def STRIDE    (* WIDTH BPP))
(def NPIXELS   (* WIDTH HEIGHT))
(def NCPU      (parse-int (or (sys/env "NCPU") "16")))
(def BLACK     (bit/shl 0xFF 24))
(def LN2       (math/log 2.0))

# ── Mutable state ────────────────────────────────────────────────

(def @view @{:cx -0.5  :cy 0.0  :scale 3.5  :iter (if gpu-ctx 256 32)})
(def @handle nil)
(def @quit? false)
(def @actual-w WIDTH)
(def @actual-h HEIGHT)

# ── Viewport ─────────────────────────────────────────────────────

(defn viewport []
  "Compute viewport parameters from current view state."
  (let* [aspect (/ (float HEIGHT) (float WIDTH))
         scale  (view :scale)
         x-min  (- (view :cx) (/ scale 2.0))
         y-min  (- (view :cy) (/ (* scale aspect) 2.0))
         dx     (/ scale (float WIDTH))
         dy     (/ (* scale aspect) (float HEIGHT))]
    {:x-min x-min :y-min y-min :dx dx :dy dy :aspect aspect}))

# ── Pixel buffer ─────────────────────────────────────────────────

(def pixel-buf (ffi/malloc (* NPIXELS BPP)))
(def row-type  (ffi/array :u32 WIDTH))
(def row-buf   (map (fn [_] 0) (range WIDTH)))

# ── Color palette (Bernstein polynomials) ─────────────────────────

(def palette
  (map (fn [i]
    (let* [t   (/ (float i) 256.0)
           omt (- 1.0 t)
           r   (min 255 (integer (* 255.0 9.0 omt t t t)))
           g   (min 255 (integer (* 255.0 15.0 omt omt t t)))
           b   (min 255 (integer (* 255.0 8.5 omt omt omt t)))]
      (bit/or (bit/shl 0xFF 24) (bit/shl r 16) (bit/shl g 8) b)))
    (range 256)))

# ── GPU backend ──────────────────────────────────────────────────

(def gpu-plugin (when gpu-ctx vk-plugin))

# Shader compiled once — max-iter passed via params buffer, not baked in
(def gpu-shader
  (when gpu-ctx
    (gpu:compile gpu-ctx 256 2 (fn [s]
      (let* [id       (s:global-id)
             # ── viewport params from buffer 0 ──────────
             x-min    (s:load 0 (s:const-u 0))
             y-min    (s:load 0 (s:const-u 1))
             dx       (s:load 0 (s:const-u 2))
             dy       (s:load 0 (s:const-u 3))
             width-f  (s:load 0 (s:const-u 4))
             limit    (s:f2u (s:load 0 (s:const-u 5)))
             # ── pixel coords from global-id ────────────
             id-f     (s:u2f id)
             py-u     (s:f2u (s:fdiv id-f width-f))
             px-u     (s:isub id (s:imul py-u (s:f2u width-f)))
             cx       (s:fadd x-min (s:fmul (s:u2f px-u) dx))
             cy       (s:fadd y-min (s:fmul (s:u2f py-u) dy))
             # ── mandelbrot iteration ───────────────────
             zr       (s:var-f)
             zi       (s:var-f)
             iter     (s:var-u)
             four     (s:const-f 4.0)
             zero-f   (s:const-f 0.0)
             zero-u   (s:const-u 0)
             one-u    (s:const-u 1)
             hdr      (s:block)
             body     (s:block)
             cont     (s:block)
             merge    (s:block)]
        (s:store-var zr zero-f)
        (s:store-var zi zero-f)
        (s:store-var iter zero-u)
        (s:branch hdr)
        # loop header
        (s:begin-block hdr)
        (let* [r   (s:load-var zr)
               i   (s:load-var zi)
               r2  (s:fmul r r)
               i2  (s:fmul i i)
               mag (s:fadd r2 i2)
               ok  (s:flt mag four)
               n   (s:load-var iter)
               lim (s:slt n limit)
               go  (s:logical-and ok lim)]
          (s:loop-merge merge cont)
          (s:branch-cond go body merge))
        # loop body: z = z² + c
        (s:begin-block body)
        (let* [r  (s:load-var zr)
               i  (s:load-var zi)
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
        # color mapping (Bernstein polynomials → ARGB32)
        (s:begin-block merge)
        (let* [n-iters  (s:load-var iter)
               inside?  (s:logical-not (s:slt n-iters limit))
               idx      (s:umod (s:imul n-iters (s:const-u 3)) (s:const-u 256))
               t-val    (s:fdiv (s:u2f idx) (s:const-f 256.0))
               omt      (s:fsub (s:const-f 1.0) t-val)
               c255     (s:const-f 255.0)
               t3       (s:fmul t-val (s:fmul t-val t-val))
               ru       (s:umin (s:f2u (s:fmul c255 (s:fmul (s:const-f 9.0)  (s:fmul omt t3)))) (s:const-u 255))
               t2       (s:fmul t-val t-val)
               omt2     (s:fmul omt omt)
               gu       (s:umin (s:f2u (s:fmul c255 (s:fmul (s:const-f 15.0) (s:fmul omt2 t2)))) (s:const-u 255))
               omt3     (s:fmul omt omt2)
               bu       (s:umin (s:f2u (s:fmul c255 (s:fmul (s:const-f 8.5)  (s:fmul omt3 t-val)))) (s:const-u 255))
               alpha    (s:const-u 0xFF000000)
               color    (s:ior alpha (s:ior (s:ishl ru (s:const-u 16)) (s:ior (s:ishl gu (s:const-u 8)) bu)))
               pixel    (s:select-u inside? alpha color)]
          (s:store 1 id (s:bitcast-u2f pixel))))))))

(defn compute-mandelbrot-gpu []
  (def vp (viewport))
  (def params [(vp :x-min) (vp :y-min) (vp :dx) (vp :dy)
               (float WIDTH) (float (view :iter))])
  (def wg-count (int (ceil (/ (float NPIXELS) 256.0))))
  (def handle (gpu-plugin:dispatch gpu-shader wg-count 1 1
                [(gpu:input params) (gpu:output NPIXELS)]))
  (gpu-plugin:wait handle)
  (def raw (gpu-plugin:collect handle))
  # blit raw ARGB32 bytes to pixel-buf (skip 8-byte collect header)
  (ffi/write pixel-buf (ffi/array :u8 (* NPIXELS 4)) (slice raw 8 (+ 8 (* NPIXELS 4)))))

# ── CPU backend (thread pool) ────────────────────────────────────

(defn compute-row [buf ci x-min dx max-iter]
  (def @px 0)
  (while (< px WIDTH)
    (def cr (+ x-min (* (float px) dx)))
    (def q (+ (* (- cr 0.25) (- cr 0.25)) (* ci ci)))
    (def color
      (if (or (<= (* q (+ q (- cr 0.25))) (* 0.25 (* ci ci)))
              (<= (+ (* (+ cr 1.0) (+ cr 1.0)) (* ci ci)) 0.0625))
        BLACK
        (begin
          (def @zr 0.0) (def @zi 0.0) (def @zr2 0.0) (def @zi2 0.0) (def @iter 0)
          (while (and (< iter max-iter) (<= (+ zr2 zi2) 4.0))
            (assign zi  (+ (* 2.0 zr zi) ci))
            (assign zr  (+ (- zr2 zi2) cr))
            (assign zr2 (* zr zr))
            (assign zi2 (* zi zi))
            (assign iter (inc iter)))
          (if (= iter max-iter)
            BLACK
            (let* [log-zn (/ (math/log (+ zr2 zi2)) 2.0)
                   smooth (- (+ (float iter) 1.0) (/ (math/log log-zn) LN2))
                   idx    (mod (integer (* smooth 3.0)) 256)]
              (palette idx))))))
    (put buf px color)
    (assign px (inc px))))

(defn recv-blocking [rx]
  (let [sel (chan/select @[rx])] (sel 1)))

(def @work-txs @[])
(def @done-rx nil)

(defn init-workers []
  (def [dtx drx] (chan))
  (assign done-rx drx)
  (repeat NCPU
    (def [wtx wrx] (chan))
    (push work-txs wtx)
    (sys/spawn (fn []
      (def buf (map (fn [_] 0) (range WIDTH)))
      (forever
        (match (recv-blocking wrx)
          ([paddr y-min dy x-min dx max-iter y-start y-end]
            (def pbuf (ptr/from-int paddr))
            (def @py y-start)
            (while (< py y-end)
              (compute-row buf (+ y-min (* (float py) dy)) x-min dx max-iter)
              (ffi/write (ptr/add pbuf (* py STRIDE)) row-type buf)
              (assign py (inc py)))
            (chan/send dtx :done))
          (_ (chan/send dtx :skip))))))))

(defn compute-mandelbrot-cpu []
  (def vp (viewport))
  (def paddr (ptr/to-int pixel-buf))
  (def rows-per (/ HEIGHT NCPU))
  (def @t 0)
  (while (< t NCPU)
    (def y-start (* t rows-per))
    (def y-end (if (= t (- NCPU 1)) HEIGHT (* (+ t 1) rows-per)))
    (chan/send (work-txs t) [paddr (vp :y-min) (vp :dy) (vp :x-min) (vp :dx)
                             (view :iter) y-start y-end])
    (assign t (inc t)))
  (repeat NCPU (recv-blocking done-rx)))

# ── Render ───────────────────────────────────────────────────────

(defn compute-mandelbrot []
  (def t0 (clock/monotonic))
  (if gpu-ctx (compute-mandelbrot-gpu) (compute-mandelbrot-cpu))
  (* 1000.0 (- (clock/monotonic) t0)))

(defn refresh []
  (ev/spawn (fn []
    (compute-mandelbrot)
    (when handle
      (gtk:set-title handle
        (string "Mandelbrot — " (view :cx) " + " (view :cy)
                "i  scale=" (view :scale) "  iter=" (view :iter)))
      (gtk:queue-draw handle :canvas)))))

# ── GTK callbacks ────────────────────────────────────────────────

(defn on-draw [_da cairo-ctx w h _data]
  (assign actual-w w)
  (assign actual-h h)
  (let* [s    (min (/ (float w) (float WIDTH)) (/ (float h) (float HEIGHT)))
         ox   (/ (- (float w) (* s (float WIDTH))) 2.0)
         oy   (/ (- (float h) (* s (float HEIGHT))) 2.0)
         surf (cr:image-surface-for-data pixel-buf cr:FORMAT_ARGB32 WIDTH HEIGHT STRIDE)]
    (cr:scale cairo-ctx s s)
    (cr:set-source-surface cairo-ctx surf (/ ox s) (/ oy s))
    (cr:paint cairo-ctx)
    (cr:surface-destroy surf)))

(defn on-click [gesture n x y]
  (let* [btn    (b:gtk-gesture-single-get-current-button gesture)
         aspect (/ (float HEIGHT) (float WIDTH))
         scale  (view :scale)
         nx     (/ x (float actual-w))
         ny     (/ y (float actual-h))
         cx     (+ (- (view :cx) (/ scale 2.0)) (* nx scale))
         cy     (+ (- (view :cy) (/ (* scale aspect) 2.0)) (* ny (* scale aspect)))
         factor (cond ((= btn 1) 0.5) ((= btn 3) 2.0) (true nil))]
    (when factor
      (put view :cx cx)
      (put view :cy cy)
      (put view :scale (* scale factor))
      (refresh))))

(defn on-scroll [dx dy]
  (put view :scale (* (view :scale) (if (< dy 0.0) (/ 1.0 1.5) 1.5)))
  (refresh)
  1)

(defn on-key [keyval keycode state]
  (let [step (/ (view :scale) 4.0)]
    (cond
      ((or (= keyval 0xff1b) (= keyval 0x71))           # ESC / Q
        (gtk:close handle)
        (assign quit? true) 1)
      ((= keyval 0x72)                                    # R
        (put view :cx -0.5) (put view :cy 0.0)
        (put view :scale 3.5) (put view :iter 32)
        (refresh) 1)
      ((= keyval 0xff51) (put view :cx (- (view :cx) step)) (refresh) 1)
      ((= keyval 0xff53) (put view :cx (+ (view :cx) step)) (refresh) 1)
      ((= keyval 0xff52) (put view :cy (- (view :cy) step)) (refresh) 1)
      ((= keyval 0xff54) (put view :cy (+ (view :cy) step)) (refresh) 1)
      ((or (= keyval 0x2b) (= keyval 0x3d) (= keyval 0xffab))
        (put view :iter (* (view :iter) 2)) (refresh) 1)
      ((or (= keyval 0x2d) (= keyval 0xffad))
        (when (> (view :iter) 16)
          (put view :iter (/ (view :iter) 2)))
        (refresh) 1)
      (true 0))))

# ── Main ─────────────────────────────────────────────────────────

(println "Mandelbrot Explorer")
(println "  left-click: zoom in    right-click: zoom out    scroll: zoom")
(println "  arrows: pan    +/-: iterations    r: reset    q: quit")

(assign handle
  (gtk:app/new "org.elle.mandelbrot"
    (fn [h]
      (b:gtk-window-set-default-size h:window WIDTH HEIGHT)
      (gtk:build h
        [:drawing-area {:id :canvas :width WIDTH :height HEIGHT
                        :on-draw on-draw}])
      (gtk:on-click h :canvas on-click)
      (gtk:on-scroll h :canvas on-scroll)
      (gtk:on-key h :window on-key)
      (when gpu-ctx (b:gtk-window-fullscreen h:window))
      (when (not gpu-ctx) (init-workers)))
    :flags 32))

(ev/spawn (fn [] (refresh)))
(gtk:app/run handle :quit (fn [] quit?))
