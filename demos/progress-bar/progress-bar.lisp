(elle/epoch 9)
## demos/progress-bar/progress-bar.lisp — OSD progress bar overlay
##
## Reads percentages (0–100) from stdin, one per line.
## Renders a progress bar as a Wayland overlay (layer-shell).
##
## Usage:
##   echo "50" | cargo run --release -- demos/progress-bar/progress-bar.lisp
##   seq 0 5 100 | cargo run --release -- demos/progress-bar/progress-bar.lisp
##   (some-long-process) | tee >(cut -f2 | cargo run --release -- demos/progress-bar/progress-bar.lisp)

(def wl-plugin (import "plugin/wayland"))
(def wl ((import "std/wayland") wl-plugin))

## ── Colors (ARGB8888) ──────────────────────────────────────────────────

(def fill-color 0xDD7EC8E3)
# light blue, slightly transparent
(def track-color 0x55000000)
# transparent black

## ── Connect and create full-screen overlay ──────────────────────────────

(def conn (wl:connect))
(def fd (wl:fd conn))

(def surf-id
  (wl:layer-surface conn :layer :overlay :anchor [:top :bottom :left :right]
    :height 0 :exclusive-zone 0))

## ── Wait for the initial configure event ────────────────────────────────

(def @configured false)
(def @screen-w 0)
(def @screen-h 0)

(while (not configured)
  (wl:flush conn)
  (ev/poll-fd fd :read 0.1)
  (wl:dispatch conn)
  (each ev in (wl:poll-events conn)
    (when (and (= ev:type :configure) (= ev:surface-id surf-id))
      (assign configured true)
      (assign screen-w ev:width)
      (assign screen-h ev:height))))

## ── Compute bar geometry ───────────────────────────────────────────────
## Bar is 50% of screen width, centered horizontally,
## positioned 25% up from the bottom edge.
## Height is 2% of screen height.

(def bar-height (max 1 (int (* screen-h 0.03))))

(def bar-w (int (/ screen-w 2)))
(def bar-x (int (/ (- screen-w bar-w) 2)))
(def bar-y (- screen-h (int (/ screen-h 4)) (int (/ bar-height 2))))

## ── Create SHM buffer (full screen, mostly transparent) ────────────────

(def buf-id (wl:shm-buffer conn screen-w screen-h))

## ── Rendering ──────────────────────────────────────────────────────────

(defn render-bar [pct]
  "Fill buffer: clear to transparent, draw pill-shaped bar with rounded endcaps."
  (wl:buffer-fill conn buf-id 0x00000000)
  (def r (int (/ bar-height 2)))  # bar track — transparent black pill
  (wl:buffer-fill-circle conn buf-id (+ bar-x r) (+ bar-y r) r track-color)
  (wl:buffer-fill-circle conn buf-id (+ bar-x bar-w (- r)) (+ bar-y r) r
    track-color)
  (wl:buffer-fill-rect conn buf-id (+ bar-x r) bar-y (- bar-w (* 2 r))
    bar-height track-color)  # progress fill — light blue pill (clipped to fill width)
  (def fill-w (int (* bar-w (/ (max 0 (min 100 pct)) 100.0))))
  (when (> fill-w 0)
    (cond  # fill is smaller than one diameter — just a circle
      (<= fill-w (* 2 r)) (wl:buffer-fill-circle conn buf-id (+ bar-x r)
        (+ bar-y r) r fill-color)  # fill spans past one diameter — left cap + rect + right cap
      true
        (begin
          (wl:buffer-fill-circle conn buf-id (+ bar-x r) (+ bar-y r) r
            fill-color)
          (wl:buffer-fill-rect conn buf-id (+ bar-x r) bar-y (- fill-w (* 2 r))
            bar-height fill-color)
          (wl:buffer-fill-circle conn buf-id (+ bar-x fill-w (- r)) (+ bar-y r)
            r fill-color))))
  (wl:attach conn surf-id buf-id)
  (wl:damage conn surf-id 0 0 screen-w screen-h)
  (wl:commit conn surf-id))

## ── Initial render at 0% ───────────────────────────────────────────────

(render-bar 0)

## ── Main loop ──────────────────────────────────────────────────────────
##
## Two fibers: stdin reader updates @target-pct, main loop animates toward it.

(def @target-pct 0.0)
(def @display-pct 0.0)
(def @done false)

(ev/spawn (fn []
            (forever
              (def line (port/read-line (*stdin*)))
              (when (nil? line)
                (assign done true)
                (break))
              (assign target-pct (float (parse-int line))))))

(def anim-step 0.15)
# ease-out fraction per frame

(while (not done)  # ease toward target
  (when (not (= display-pct target-pct))
    (assign display-pct (+ display-pct (* (- target-pct display-pct) anim-step)))
    (when (< (abs (- display-pct target-pct)) 0.5)
      (assign display-pct target-pct)))
  (render-bar display-pct)
  (wl:flush conn)
  (ev/poll-fd fd :read 0.033)
  (wl:dispatch conn)
  (each _ev in (wl:poll-events conn)
    nil))

## ── Cleanup ────────────────────────────────────────────────────────────

(wl:layer-destroy conn surf-id)
(wl:disconnect conn)
