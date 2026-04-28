#!/usr/bin/env elle
(elle/epoch 9)

# Conway's Game of Life — SDL3 demo
#
# Interactive cellular automaton rendered via std/sdl3 (pure FFI).
#
# Controls:
#   Click / drag  toggle cells
#   Space         pause / resume
#   G             toggle grid lines
#   R             randomize grid
#   C             clear grid
#   +/-           speed up / slow down
#   Escape / Q    quit

(def sdl ((import "std/sdl3")))

# ── Grid parameters ───────────────────────────────────────────────────

(def cols 80)
(def rows 60)
(def cell 10)
(def win-w (* cols cell))
(def win-h (* rows cell))
(def ncells (* rows cols))

# ── Simple xorshift PRNG (no plugin needed) ───────────────────────────

(def @rng-state 2463534242)

(defn rand-u32 []
  (assign rng-state (bit/xor rng-state (bit/shl rng-state 13)))
  (assign rng-state (bit/xor rng-state (bit/shr rng-state 17)))
  (assign rng-state (bit/xor rng-state (bit/shl rng-state 5)))
  (assign rng-state (bit/and rng-state 0xFFFFFFFF))
  rng-state)

(defn rand-float []
  (/ (float (rand-u32)) 4294967296.0))

# ── Grid — flat mutable array, row-major ──────────────────────────────

(defn make-grid []
  (def g @[])
  (def @i 0)
  (while (< i ncells)
    (push g 0)
    (assign i (+ i 1)))
  g)

(defn idx [r c]
  (+ (* r cols) c))

(defn cell-at [g r c]
  (if (and (>= r 0) (< r rows) (>= c 0) (< c cols)) (get g (idx r c)) 0))

(defn set-cell [g r c v]
  (put g (idx r c) v))

# Precomputed neighbor offsets (avoids arithmetic per lookup)
(def off-nw (- 0 cols 1))
(def off-n (- 0 cols))
(def off-ne (- 1 cols))
(def off-w -1)
(def off-e 1)
(def off-sw (- cols 1))
(def off-s cols)
(def off-se (+ cols 1))

# Interior neighbor count — uses get (not array-as-fn) for JIT eligibility
(defn count-neighbors-fast [g i]
  (+ (get g (+ i off-nw)) (get g (+ i off-n)) (get g (+ i off-ne))
    (get g (+ i off-w)) (get g (+ i off-e)) (get g (+ i off-sw))
    (get g (+ i off-s)) (get g (+ i off-se))))

(defn count-alive [g]
  (def @n 0)
  (def @i 0)
  (while (< i ncells)
    (when (nonzero? (g i)) (assign n (+ n 1)))
    (assign i (+ i 1)))
  n)

(defn step [g]
  (def nxt (make-grid))  # Interior cells — skip border row/col, use direct indexing
  (def @r 1)
  (while (< r (- rows 1))
    (def @i (+ (* r cols) 1))
    (def @c 1)
    (while (< c (- cols 1))
      (let [n (count-neighbors-fast g i)]
        (when (or (= n 3) (and (= (get g i) 1) (= n 2))) (put nxt i 1)))
      (assign i (+ i 1))
      (assign c (+ c 1)))
    (assign r (+ r 1)))  # Border cells are always dead (simpler than wrapping)
  nxt)

# ── Seed patterns ─────────────────────────────────────────────────────

(defn place-glider [g r c]
  (set-cell g r (+ c 1) 1)
  (set-cell g (+ r 1) (+ c 2) 1)
  (set-cell g (+ r 2) c 1)
  (set-cell g (+ r 2) (+ c 1) 1)
  (set-cell g (+ r 2) (+ c 2) 1))

(defn place-r-pentomino [g r c]
  (set-cell g r (+ c 1) 1)
  (set-cell g r (+ c 2) 1)
  (set-cell g (+ r 1) c 1)
  (set-cell g (+ r 1) (+ c 1) 1)
  (set-cell g (+ r 2) (+ c 1) 1))

(defn place-lwss [g r c]
  (set-cell g r (+ c 1) 1)
  (set-cell g r (+ c 4) 1)
  (set-cell g (+ r 1) c 1)
  (set-cell g (+ r 2) c 1)
  (set-cell g (+ r 2) (+ c 4) 1)
  (set-cell g (+ r 3) c 1)
  (set-cell g (+ r 3) (+ c 1) 1)
  (set-cell g (+ r 3) (+ c 2) 1)
  (set-cell g (+ r 3) (+ c 3) 1))

(defn place-pulsar [g r c]
  (def offsets
    (list @[0 2] @[0 3] @[0 4] @[2 0] @[3 0] @[4 0] @[2 5] @[3 5] @[4 5] @[5 2]
      @[5 3] @[5 4]))
  (each off offsets
    (let [dr (off 0)
          dc (off 1)]
      (set-cell g (+ r dr) (+ c dc) 1)
      (set-cell g (+ r dr) (- (+ c 12) dc) 1)
      (set-cell g (- (+ r 12) dr) (+ c dc) 1)
      (set-cell g (- (+ r 12) dr) (- (+ c 12) dc) 1))))

(defn randomize [g]
  (def @i 0)
  (while (< i ncells)
    (put g i (if (< (rand-float) 0.72) 0 1))
    (assign i (+ i 1)))
  g)

# ── Rendering ─────────────────────────────────────────────────────────

(defn render-grid-lines [ren]
  (sdl:set-blend-mode ren sdl:blend-blend)
  (sdl:set-color ren 60 60 80 :a 80)  # Vertical lines
  (def @c 0)
  (while (<= c cols)
    (sdl:draw-line ren (float (* c cell)) 0.0 (float (* c cell)) (float win-h))
    (assign c (+ c 1)))  # Horizontal lines
  (def @r 0)
  (while (<= r rows)
    (sdl:draw-line ren 0.0 (float (* r cell)) (float win-w) (float (* r cell)))
    (assign r (+ r 1)))
  (sdl:set-blend-mode ren sdl:blend-none))

(defn render-cells [ren g]
  (sdl:set-color ren 0 220 100)
  (def @i 0)
  (while (< i ncells)
    (when (nonzero? (g i))
      (let* [c (mod i cols)
             r (/ (- i c) cols)]
        (sdl:fill-rect ren (float (+ (* c cell) 1)) (float (+ (* r cell) 1))
          (float (- cell 2)) (float (- cell 2)))))
    (assign i (+ i 1))))

(defn render-hud [ren gen alive paused speed fps]
  (sdl:set-color ren 0 255 0)
  (sdl:debug-text ren 10.0 10.0
    (string "Gen: " gen "  Alive: " alive "  FPS: " fps))
  (sdl:debug-text ren 10.0 26.0
    (string "Speed: " speed "  " (if paused "[PAUSED]" "[RUNNING]")))
  (sdl:set-color ren 120 120 140)
  (sdl:debug-text ren 10.0 (float (- win-h 18))
    "SPC:pause  G:grid  R:rand  C:clear  +/-:speed  Q:quit"))

(defn render [ren g gen alive paused speed fps show-grid]
  (sdl:set-color ren 15 15 25)
  (sdl:clear ren)
  (when show-grid (render-grid-lines ren))
  (render-cells ren g)
  (render-hud ren gen alive paused speed fps)
  (sdl:present ren))

# ── Event handling ────────────────────────────────────────────────────

(defn handle-key [state ev]
  (case ev:scancode
    sdl:scancode-escape (put state :running false)
    20 (put state :running false)  # q
    sdl:scancode-space
      (put state :paused (not (state :paused)))
    21
      (begin
        (put state :grid (randomize (make-grid)))  # r
        (put state :gen 0))
    6
      (begin
        (put state :grid (make-grid))  # c
        (put state :gen 0))
    46
      (put state :speed (min 20 (+ (state :speed) 1)))  # +
      45
      (put state :speed (max 1 (- (state :speed) 1)))  # -
      10
      (put state :show-grid (not (state :show-grid)))  # g
      nil))

(defn handle-mouse [state ev]
  (when (or (= ev:type :mouse-down)
      (and (= ev:type :mouse-motion) (nonzero? ev:state)))
    (let* [gc (int (/ ev:x (float cell)))
           gr (int (/ ev:y (float cell)))]
      (when (and (>= gr 0) (< gr rows) (>= gc 0) (< gc cols))
        (set-cell (state :grid) gr gc
          (if (= ev:type :mouse-down)
            (if (nonzero? (cell-at (state :grid) gr gc)) 0 1)
            1))))))

(defn handle-events [state]
  (each ev (sdl:poll-events)
    (match ev:type
      :quit (put state :running false)
      :key-down (handle-key state ev)
      :mouse-down (handle-mouse state ev)
      :mouse-motion (handle-mouse state ev)
      _ nil))
  state)


# ── Main ──────────────────────────────────────────────────────────────

(defn main []
  (sdl:init)
  (def win
    (sdl:create-window "Conway's Game of Life" win-w win-h
      :flags sdl:window-resizable))
  (def ren (sdl:create-renderer win))
  (sdl:set-vsync ren 1)

  # Seed the grid
  (def @grid (make-grid))
  (place-r-pentomino grid 25 35)
  (place-glider grid 3 3)
  (place-glider grid 3 70)
  (place-lwss grid 50 10)
  (place-pulsar grid 5 30)
  (def state
    @{:running true :paused false :gen 0 :grid grid :speed 1 :show-grid false})
  (def @last-tick (sdl:ticks))
  (def @frame-count 0)
  (def @fps 0)
  (def @alive 0)
  (println "Conway's Game of Life")
  (println "  click/drag to draw, space to pause, g for grid")
  (println "  r to randomize, c to clear, +/- speed, q to quit")
  (while (state :running)
    (handle-events state)

    # FPS counter
    (assign frame-count (+ frame-count 1))
    (let [now (sdl:ticks)]
      (when (>= (- now last-tick) 1000)
        (assign fps frame-count)
        (assign frame-count 0)
        (assign last-tick now)
        (assign alive (count-alive (state :grid)))
        (sdl:set-title win
          (string "Conway's Game of Life — Gen " (state :gen)))))

    # Simulation steps per frame
    (unless (state :paused)
      (def @s 0)
      (while (< s (state :speed))
        (put state :grid (step (state :grid)))
        (put state :gen (+ (state :gen) 1))
        (assign s (+ s 1))))

    # Render
    (render ren (state :grid) (state :gen) alive (state :paused) (state :speed)
      fps (state :show-grid)))

  # Cleanup
  (sdl:destroy-renderer ren)
  (sdl:destroy-window win)
  (sdl:quit)
  (println (string "finished after " (state :gen) " generations")))

(main)
