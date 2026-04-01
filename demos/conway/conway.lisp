#!/usr/bin/env elle
(elle/epoch 5)

# Conway's Game of Life — SDL2 demo
#
# Interactive cellular automaton rendered via SDL2 FFI.
#
# Controls:
#   Click       toggle cell
#   Space       pause / resume
#   R           randomize grid
#   C           clear grid
#   Escape / Q  quit

# ---------------------------------------------------------------------------
# SDL2 library + constants
# ---------------------------------------------------------------------------

(def sdl (ffi/native "libSDL2-2.0.so.0"))

(def SDL_INIT_VIDEO           0x00000020)
(def SDL_WINDOWPOS_CENTERED   0x2FFF0000)
(def SDL_WINDOW_SHOWN         0x00000004)
(def SDL_RENDERER_ACCELERATED 0x00000002)
(def SDL_RENDERER_PRESENTVSYNC 0x00000004)

# Event type constants
(def EV_QUIT       0x100)
(def EV_KEYDOWN    0x300)
(def EV_MOUSEDOWN  0x401)

# Keycodes
(def KEY_ESCAPE 27)
(def KEY_SPACE  32)
(def KEY_c      99)
(def KEY_q      113)
(def KEY_r      114)

# ---------------------------------------------------------------------------
# Grid parameters
# ---------------------------------------------------------------------------

(def COLS      80)
(def ROWS      60)
(def CELL      10)
(def WIN_W     (* COLS CELL))
(def WIN_H     (* ROWS CELL))
(def NCELLS    (* ROWS COLS))

# ---------------------------------------------------------------------------
# SDL2 bindings
# ---------------------------------------------------------------------------

(ffi/defbind sdl-init           sdl "SDL_Init"              :int  @[:u32])
(ffi/defbind sdl-quit           sdl "SDL_Quit"              :void @[])
(ffi/defbind sdl-create-win     sdl "SDL_CreateWindow"      :ptr  @[:string :int :int :int :int :u32])
(ffi/defbind sdl-destroy-win    sdl "SDL_DestroyWindow"     :void @[:ptr])
(ffi/defbind sdl-create-ren     sdl "SDL_CreateRenderer"    :ptr  @[:ptr :int :u32])
(ffi/defbind sdl-destroy-ren    sdl "SDL_DestroyRenderer"   :void @[:ptr])
(ffi/defbind sdl-set-color      sdl "SDL_SetRenderDrawColor" :int @[:ptr :u8 :u8 :u8 :u8])
(ffi/defbind sdl-clear          sdl "SDL_RenderClear"       :int  @[:ptr])
(ffi/defbind sdl-present        sdl "SDL_RenderPresent"     :void @[:ptr])
(ffi/defbind sdl-fill-rect      sdl "SDL_RenderFillRect"    :int  @[:ptr :ptr])
(ffi/defbind sdl-poll-event     sdl "SDL_PollEvent"         :int  @[:ptr])
(ffi/defbind sdl-delay          sdl "SDL_Delay"             :void @[:u32])
(ffi/defbind sdl-get-ticks      sdl "SDL_GetTicks"          :u32  @[])

# ---------------------------------------------------------------------------
# SDL helpers
# ---------------------------------------------------------------------------

(def rect-type (ffi/struct @[:i32 :i32 :i32 :i32]))
(def rect-buf  (ffi/malloc (ffi/size rect-type)))

(defn draw-rect (ren x y w h)
  (ffi/write rect-buf rect-type @[x y w h])
  (sdl-fill-rect ren rect-buf))

# Event buffer — SDL_Event union is 56 bytes; allocate 64 for safety
(def ev-buf (ffi/malloc 64))

# Struct overlays for reading event fields
(def ev-type-st   (ffi/struct @[:u32]))
# Keyboard: type u32, timestamp u32, windowID u32, state u8, repeat u8,
#           pad u8, pad u8, scancode u32, sym i32
(def ev-key-st    (ffi/struct @[:u32 :u32 :u32 :u8 :u8 :u8 :u8 :u32 :i32]))
# Mouse button: type u32, timestamp u32, windowID u32, which u32,
#               button u8, state u8, clicks u8, pad u8, x i32, y i32
(def ev-mouse-st  (ffi/struct @[:u32 :u32 :u32 :u32 :u8 :u8 :u8 :u8 :i32 :i32]))

# ---------------------------------------------------------------------------
# Random number generator (plugin)
# ---------------------------------------------------------------------------

(def rng (import "plugin/random"))
(def rand-float (get rng :float))

# ---------------------------------------------------------------------------
# Grid — flat mutable array, row-major
# ---------------------------------------------------------------------------

(defn make-grid ()
  (def g @[])
  (var i 0)
  (while (< i NCELLS)
    (push g 0)
    (assign i (+ i 1)))
  g)

(defn cell-at (g r c)
  (if (and (>= r 0) (< r ROWS) (>= c 0) (< c COLS))
    (get g (+ (* r COLS) c))
    0))

(defn set-cell (g r c v)
  (put g (+ (* r COLS) c) v))

(defn count-neighbors (g r c)
  (+ (cell-at g (- r 1) (- c 1))
     (cell-at g (- r 1) c)
     (cell-at g (- r 1) (+ c 1))
     (cell-at g r       (- c 1))
     (cell-at g r       (+ c 1))
     (cell-at g (+ r 1) (- c 1))
     (cell-at g (+ r 1) c)
     (cell-at g (+ r 1) (+ c 1))))

(defn step (g)
  (def nxt (make-grid))
  (var r 0)
  (while (< r ROWS)
    (var c 0)
    (while (< c COLS)
      (let* ((alive (= (cell-at g r c) 1))
             (n     (count-neighbors g r c)))
        (when (or (= n 3) (and alive (= n 2)))
          (set-cell nxt r c 1)))
      (assign c (+ c 1)))
    (assign r (+ r 1)))
  nxt)

# ---------------------------------------------------------------------------
# Seed patterns
# ---------------------------------------------------------------------------

(defn place-glider (g r c)
  (set-cell g r       (+ c 1) 1)
  (set-cell g (+ r 1) (+ c 2) 1)
  (set-cell g (+ r 2) c       1)
  (set-cell g (+ r 2) (+ c 1) 1)
  (set-cell g (+ r 2) (+ c 2) 1))

(defn place-r-pentomino (g r c)
  (set-cell g r       (+ c 1) 1)
  (set-cell g r       (+ c 2) 1)
  (set-cell g (+ r 1) c       1)
  (set-cell g (+ r 1) (+ c 1) 1)
  (set-cell g (+ r 2) (+ c 1) 1))

(defn place-lwss (g r c)
  (set-cell g r       (+ c 1) 1)
  (set-cell g r       (+ c 4) 1)
  (set-cell g (+ r 1) c       1)
  (set-cell g (+ r 2) c       1)
  (set-cell g (+ r 2) (+ c 4) 1)
  (set-cell g (+ r 3) c       1)
  (set-cell g (+ r 3) (+ c 1) 1)
  (set-cell g (+ r 3) (+ c 2) 1)
  (set-cell g (+ r 3) (+ c 3) 1))

(defn place-pulsar (g r c)
  # Pulsar — period-3 oscillator
  (def offsets
    (list
      # Top-left quadrant (reflected to all four)
      @[0 2] @[0 3] @[0 4]
      @[2 0] @[3 0] @[4 0]
      @[2 5] @[3 5] @[4 5]
      @[5 2] @[5 3] @[5 4]))
  (each off in offsets
    (let ((dr (get off 0)) (dc (get off 1)))
      # Four-way symmetry
      (set-cell g (+ r dr)         (+ c dc)         1)
      (set-cell g (+ r dr)         (- (+ c 12) dc)  1)
      (set-cell g (- (+ r 12) dr)  (+ c dc)         1)
      (set-cell g (- (+ r 12) dr)  (- (+ c 12) dc)  1))))

(defn randomize (g)
  (var i 0)
  (while (< i NCELLS)
    (put g i (if (< (rand-float) 0.72) 0 1))
    (assign i (+ i 1)))
  g)

# ---------------------------------------------------------------------------
# Rendering
# ---------------------------------------------------------------------------

(defn render (ren g)
  # Dark background
  (sdl-set-color ren 15 15 25 255)
  (sdl-clear ren)

  # Live cells — bright green
  (sdl-set-color ren 0 220 100 255)
  (var i 0)
  (while (< i NCELLS)
    (when (= (get g i) 1)
      (let* ((c (mod i COLS))
             (r (/ (- i c) COLS)))
        (draw-rect ren (* c CELL) (* r CELL)
                       (- CELL 1) (- CELL 1))))
    (assign i (+ i 1)))

  (sdl-present ren))

# ---------------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------------

(defn main ()
  (sdl-init SDL_INIT_VIDEO)

  (def win (sdl-create-win "Conway's Game of Life"
    SDL_WINDOWPOS_CENTERED SDL_WINDOWPOS_CENTERED
    WIN_W WIN_H SDL_WINDOW_SHOWN))

  (def ren (sdl-create-ren win -1
    (+ SDL_RENDERER_ACCELERATED SDL_RENDERER_PRESENTVSYNC)))

  # Seed the grid
  (var grid (make-grid))
  (place-r-pentomino grid 25 35)
  (place-glider grid 3 3)
  (place-glider grid 3 70)
  (place-lwss grid 50 10)
  (place-pulsar grid 5 30)

  (var running true)
  (var paused  false)
  (var gen     0)

  (println "Conway's Game of Life — click to draw, space to pause, r to randomize, q to quit")

  (while running
    # --- events ---
    (while (= (sdl-poll-event ev-buf) 1)
      (let ((etype (get (ffi/read ev-buf ev-type-st) 0)))
        (cond
          ((= etype EV_QUIT)
            (assign running false))

          ((= etype EV_KEYDOWN)
            (let ((sym (get (ffi/read ev-buf ev-key-st) 8)))
              (cond
                ((or (= sym KEY_ESCAPE) (= sym KEY_q))
                  (assign running false))
                ((= sym KEY_SPACE)
                  (assign paused (not paused))
                  (println (if paused "paused" "running")))
                ((= sym KEY_r)
                  (assign grid (randomize (make-grid)))
                  (assign gen 0)
                  (println "randomized"))
                ((= sym KEY_c)
                  (assign grid (make-grid))
                  (assign gen 0)
                  (println "cleared")))))

          ((= etype EV_MOUSEDOWN)
            (let* ((mdata (ffi/read ev-buf ev-mouse-st))
                   (mx    (get mdata 8))
                   (my    (get mdata 9))
                   (gc    (/ mx CELL))
                   (gr    (/ my CELL)))
              (when (and (>= gr 0) (< gr ROWS) (>= gc 0) (< gc COLS))
                (let ((cur (cell-at grid gr gc)))
                  (set-cell grid gr gc (if (= cur 1) 0 1)))))))))

    # --- simulation step ---
    (unless paused
      (assign grid (step grid))
      (assign gen (+ gen 1)))

    # --- draw ---
    (render ren grid))

  # --- cleanup ---
  (ffi/free rect-buf)
  (ffi/free ev-buf)
  (sdl-destroy-ren ren)
  (sdl-destroy-win win)
  (sdl-quit)
  (println (string "finished after " gen " generations")))

(main)
