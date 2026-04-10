#!/usr/bin/env elle
(elle/epoch 6)

# Mandelbrot Explorer — GTK4 + Cairo via lib/gtk4
#
# Interactive fractal viewer rendered via GTK4 drawing area and Cairo.
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
#
# Dependencies: libgtk-4, libcairo, libgobject-2.0, libgio-2.0

# ── Libraries ─────────────────────────────────────────────────────

(def b ((import "std/gtk4/bind")))

(def cairo (ffi/native "libcairo.so.2"))
(def gio   (ffi/native "libgio-2.0.so.0"))
(def libc  (ffi/native nil))

# ── Additional bindings not in gtk4/bind ──────────────────────────

(ffi/defbind gtk-app-new     b:libgtk "gtk_application_new"                :ptr  [:string :u32])
(ffi/defbind gtk-app-win-new b:libgtk "gtk_application_window_new"         :ptr  [:ptr])
(ffi/defbind gtk-queue-draw  b:libgtk "gtk_widget_queue_draw"              :void [:ptr])
(ffi/defbind gtk-da-new      b:libgtk "gtk_drawing_area_new"               :ptr  [])
(ffi/defbind gtk-da-set-cw   b:libgtk "gtk_drawing_area_set_content_width" :void [:ptr :int])
(ffi/defbind gtk-da-set-ch   b:libgtk "gtk_drawing_area_set_content_height" :void [:ptr :int])
(ffi/defbind gtk-da-draw-fn  b:libgtk "gtk_drawing_area_set_draw_func"     :void [:ptr :ptr :ptr :ptr])
(ffi/defbind gtk-add-ctrl    b:libgtk "gtk_widget_add_controller"          :void [:ptr :ptr])
(ffi/defbind gtk-click-new   b:libgtk "gtk_gesture_click_new"              :ptr  [])
(ffi/defbind gtk-gesture-btn b:libgtk "gtk_gesture_single_set_button"      :void [:ptr :u32])
(ffi/defbind gtk-get-btn     b:libgtk "gtk_gesture_single_get_current_button" :u32 [:ptr])
(ffi/defbind gtk-scroll-new  b:libgtk "gtk_event_controller_scroll_new"    :ptr  [:u32])
(ffi/defbind gtk-key-new     b:libgtk "gtk_event_controller_key_new"       :ptr  [])

# GIO
(ffi/defbind g-app-run       gio "g_application_run"                       :int  [:ptr :int :ptr])
(ffi/defbind g-unref         b:libgobj "g_object_unref"                    :void [:ptr])

# Cairo
(ffi/defbind cairo-img-surface cairo "cairo_image_surface_create_for_data"  :ptr  [:ptr :int :int :int :int])
(ffi/defbind cairo-set-source  cairo "cairo_set_source_surface"            :void [:ptr :ptr :double :double])
(ffi/defbind cairo-paint       cairo "cairo_paint"                         :void [:ptr])
(ffi/defbind cairo-surf-free   cairo "cairo_surface_destroy"               :void [:ptr])
(ffi/defbind cairo-scale       cairo "cairo_scale"                         :void [:ptr :double :double])

# ── Constants ─────────────────────────────────────────────────────

(def WIDTH  800)
(def HEIGHT 600)
(def SCALE  1)
(def WIN_W  800)
(def WIN_H  600)
(def BPP    4)
(def STRIDE (* WIDTH BPP))
(def NCPU  (integer (or (sys/env "NCPU") "16")))

(def KEY_ESC    0xff1b)
(def KEY_q      0x71)
(def KEY_r      0x72)
(def KEY_LEFT   0xff51)
(def KEY_UP     0xff52)
(def KEY_RIGHT  0xff53)
(def KEY_DOWN   0xff54)
(def KEY_PLUS   0x2b)
(def KEY_MINUS  0x2d)
(def KEY_EQUAL  0x3d)
(def KP_ADD     0xffab)
(def KP_SUB     0xffad)

(def CAIRO_FORMAT_ARGB32 0)
(def SCROLL_VERTICAL     2)
(def CLOCK_MONOTONIC     1)

# ── Timing ────────────────────────────────────────────────────────

(def timespec-type (ffi/struct [:long :long]))
(def ts-buf (ffi/malloc (ffi/size timespec-type)))
(ffi/defbind clock-gettime libc "clock_gettime" :int [:int :ptr])

(defn now-ms []
  (clock-gettime CLOCK_MONOTONIC ts-buf)
  (let [[ts (ffi/read ts-buf timespec-type)]]
    (+ (* (ts 0) 1000) (/ (ts 1) 1000000))))

# ── View state ────────────────────────────────────────────────────

(var view-cx    -0.5)
(var view-cy     0.0)
(var view-scale  3.5)
(var max-iter    32)
(var da-widget   nil)
(var app-window  nil)

# ── Pixel buffer ──────────────────────────────────────────────────

(def pixel-buf (ffi/malloc (* WIDTH HEIGHT BPP)))
(def row-type  (ffi/array :u32 WIDTH))

(def row-buf
  (let [[r @[]]]
    (var i 0)
    (while (< i WIDTH)
      (push r 0)
      (assign i (inc i)))
    r))

# ── Color palette (Bernstein polynomials) ─────────────────────────

(def PALETTE_SIZE 256)
(def LN2 (math/log 2.0))

(def palette
  (let [[p @[]]]
    (var i 0)
    (while (< i PALETTE_SIZE)
      (let* [[t   (/ (float i) (float PALETTE_SIZE))]
             [omt (- 1.0 t)]
             [r   (min 255 (integer (* 255.0 9.0 omt t t t)))]
             [g   (min 255 (integer (* 255.0 15.0 omt omt t t)))]
             [b   (min 255 (integer (* 255.0 8.5 omt omt omt t)))]]
        (push p (bit/or (bit/shl 255 24)
                        (bit/or (bit/shl r 16)
                                (bit/or (bit/shl g 8) b)))))
      (assign i (inc i)))
    p))

# ── Per-row computation (JIT compiles to native x86) ─────────────

(def BLACK (bit/shl 255 24))

(defn compute-row [row-buf ci x-min dx max-iter]
  (var px 0)
  (while (< px WIDTH)
    (def cr (+ x-min (* (float px) dx)))
    # cardioid and period-2 bulb check — skip full iteration
    (def q (+ (* (- cr 0.25) (- cr 0.25)) (* ci ci)))
    (def color
      (if (or (<= (* q (+ q (- cr 0.25))) (* 0.25 (* ci ci)))
              (<= (+ (* (+ cr 1.0) (+ cr 1.0)) (* ci ci)) 0.0625))
        BLACK
        (begin
          (var zr  0.0)
          (var zi  0.0)
          (var zr2 0.0)
          (var zi2 0.0)
          (var iter 0)
          (while (and (< iter max-iter) (<= (+ zr2 zi2) 4.0))
            (assign zi  (+ (* 2.0 zr zi) ci))
            (assign zr  (+ (- zr2 zi2) cr))
            (assign zr2 (* zr zr))
            (assign zi2 (* zi zi))
            (assign iter (inc iter)))
          (if (= iter max-iter)
            BLACK
            (let* [[log-zn (/ (math/log (+ zr2 zi2)) 2.0)]
                   [smooth (- (+ (float iter) 1.0) (/ (math/log log-zn) LN2))]
                   [idx    (mod (integer (* smooth 3.0)) PALETTE_SIZE)]]
              (palette idx))))))
    (put row-buf px color)
    (assign px (inc px))))

# ── Thread pool ──────────────────────────────────────────────────

(defn recv-blocking [rx]
  (def result (chan/select @[rx]))
  (result 1))

(var work-txs @[])
(var done-rx  nil)

(defn init-workers []
  (def [dtx drx] (chan))
  (assign done-rx drx)
  (var i 0)
  (while (< i NCPU)
    (def [wtx wrx] (chan))
    (push work-txs wtx)
    (sys/spawn (fn []
      (def local-buf
        (let [[r @[]]]
          (var j 0)
          (while (< j WIDTH) (push r 0) (assign j (inc j)))
          r))
      (forever
        (def msg (recv-blocking wrx))
        (match msg
          ([pixel-addr y-min dy x-min dx max-iter y-start y-end]
            (def pbuf (ptr/from-int pixel-addr))
            (var py y-start)
            (while (< py y-end)
              (compute-row local-buf (+ y-min (* (float py) dy)) x-min dx max-iter)
              (ffi/write (ptr/add pbuf (* py STRIDE)) row-type local-buf)
              (assign py (inc py)))
            (chan/send dtx :done))
          (_ (chan/send dtx :skip))))))
    (assign i (inc i))))

# ── Mandelbrot computation ────────────────────────────────────────

(defn compute-mandelbrot []
  (def t0 (now-ms))
  (def aspect (/ (float HEIGHT) (float WIDTH)))
  (def x-min  (- view-cx (/ view-scale 2.0)))
  (def y-min  (- view-cy (/ (* view-scale aspect) 2.0)))
  (def dx     (/ view-scale (float WIDTH)))
  (def dy     (/ (* view-scale aspect) (float HEIGHT)))

  (def paddr (ptr/to-int pixel-buf))
  (def rows-per (/ HEIGHT NCPU))
  (var t 0)
  (while (< t NCPU)
    (def y-start (* t rows-per))
    (def y-end (if (= t (- NCPU 1)) HEIGHT (* (+ t 1) rows-per)))
    (chan/send (work-txs t) [paddr y-min dy x-min dx max-iter y-start y-end])
    (assign t (inc t)))

  (var i 0)
  (while (< i NCPU)
    (recv-blocking done-rx)
    (assign i (inc i)))

  (- (now-ms) t0))

# ── Helpers ───────────────────────────────────────────────────────

(defn update-title []
  (when app-window
    (b:gtk-window-set-title app-window
      (string "Mandelbrot — " view-cx " + " view-cy
              "i  scale=" view-scale "  iter=" max-iter))))

(defn refresh []
  (compute-mandelbrot)
  (update-title)
  (when da-widget (gtk-queue-draw da-widget)))

# ── GTK callbacks ─────────────────────────────────────────────────

(defn on-draw [_da cr _w _h _data]
  (def surf (cairo-img-surface pixel-buf CAIRO_FORMAT_ARGB32 WIDTH HEIGHT STRIDE))
  (cairo-scale cr (float SCALE) (float SCALE))
  (cairo-set-source cr surf 0.0 0.0)
  (cairo-paint cr)
  (cairo-surf-free surf))

(defn on-click [gesture _n x y _data]
  (let* [[btn    (gtk-get-btn gesture)]
         [aspect (/ (float HEIGHT) (float WIDTH))]
         [cx     (+ (- view-cx (/ view-scale 2.0))
                    (* (/ x (float WIN_W)) view-scale))]
         [cy     (+ (- view-cy (/ (* view-scale aspect) 2.0))
                    (* (/ y (float WIN_H)) (* view-scale aspect)))]]
    (cond
      ((= btn 1)
        (assign view-cx cx)
        (assign view-cy cy)
        (assign view-scale (/ view-scale 2.0))
        (refresh))
      ((= btn 3)
        (assign view-cx cx)
        (assign view-cy cy)
        (assign view-scale (* view-scale 2.0))
        (refresh)))))

(defn on-scroll [_ctrl _dx dy _data]
  (if (< dy 0.0)
    (assign view-scale (/ view-scale 1.5))
    (assign view-scale (* view-scale 1.5)))
  (refresh)
  1)

(defn on-key [_ctrl keyval _keycode _state _data]
  (let [[step (/ view-scale 4.0)]]
    (cond
      ((or (= keyval KEY_ESC) (= keyval KEY_q))
        (when app-window (b:gtk-window-close app-window))
        1)
      ((= keyval KEY_r)
        (assign view-cx -0.5)
        (assign view-cy 0.0)
        (assign view-scale 3.5)
        (assign max-iter 32)
        (refresh) 1)
      ((= keyval KEY_LEFT)   (assign view-cx (- view-cx step)) (refresh) 1)
      ((= keyval KEY_RIGHT)  (assign view-cx (+ view-cx step)) (refresh) 1)
      ((= keyval KEY_UP)     (assign view-cy (- view-cy step)) (refresh) 1)
      ((= keyval KEY_DOWN)   (assign view-cy (+ view-cy step)) (refresh) 1)
      ((or (= keyval KEY_PLUS) (= keyval KEY_EQUAL) (= keyval KP_ADD))
        (assign max-iter (* max-iter 2))
        (refresh) 1)
      ((or (= keyval KEY_MINUS) (= keyval KP_SUB))
        (when (> max-iter 16)
          (assign max-iter (/ max-iter 2)))
        (refresh) 1)
      (true 0))))

# ── Activate ──────────────────────────────────────────────────────

(defn on-activate [app _data]
  (def win (gtk-app-win-new app))
  (assign app-window win)
  (b:gtk-window-set-default-size win WIN_W WIN_H)

  (def da (gtk-da-new))
  (assign da-widget da)
  (gtk-da-set-cw da WIN_W)
  (gtk-da-set-ch da WIN_H)

  # draw function
  (gtk-da-draw-fn da
    (ffi/callback (ffi/signature :void [:ptr :ptr :int :int :ptr]) on-draw)
    nil nil)

  # click gesture (all buttons)
  (let [[click (gtk-click-new)]]
    (gtk-gesture-btn click 0)
    (b:g-signal-connect-data click "pressed"
      (ffi/callback (ffi/signature :void [:ptr :int :double :double :ptr]) on-click)
      nil nil 0)
    (gtk-add-ctrl da click))

  # scroll zoom
  (let [[scroll (gtk-scroll-new SCROLL_VERTICAL)]]
    (b:g-signal-connect-data scroll "scroll"
      (ffi/callback (ffi/signature :int [:ptr :double :double :ptr]) on-scroll)
      nil nil 0)
    (gtk-add-ctrl da scroll))

  # keyboard (on window for global capture)
  (let [[keys (gtk-key-new)]]
    (b:g-signal-connect-data keys "key-pressed"
      (ffi/callback (ffi/signature :int [:ptr :u32 :u32 :u32 :ptr]) on-key)
      nil nil 0)
    (gtk-add-ctrl win keys))

  (b:gtk-window-set-child win da)
  (b:gtk-window-present win)

  (init-workers)
  (compute-mandelbrot)
  (update-title)
  (gtk-queue-draw da))

# ── Main ──────────────────────────────────────────────────────────

(defn main []
  (println "Mandelbrot Explorer")
  (println "  left-click: zoom in    right-click: zoom out    scroll: zoom")
  (println "  arrows: pan    +/-: iterations    r: reset    q: quit")

  (b:gtk-init)
  (def app (gtk-app-new "org.elle.mandelbrot" 32))
  (b:g-signal-connect-data app "activate"
    (ffi/callback (ffi/signature :void [:ptr :ptr]) on-activate)
    nil nil 0)

  # g_application_run needs argc >= 1
  (def arg0 (ffi/malloc 16))
  (ffi/write arg0 (ffi/array :u8 12) [109 97 110 100 101 108 98 114 111 116 0 0])
  (def argv (ffi/malloc 16))
  (ffi/write argv :ptr arg0)
  (ffi/write (ptr/add argv 8) :ptr nil)

  (g-app-run app 1 argv)
  (ffi/free argv)
  (ffi/free arg0)
  (g-unref app)
  (ffi/free pixel-buf)
  (ffi/free ts-buf))

(main)
