#!/usr/bin/env elle
(elle/epoch 9)

# Raylib hello world — basic shapes, text, and input
#
# A bouncing ball with mouse interaction.
# Click to teleport the ball, press ESC to quit.

(def rl ((import "std/raylib")))

(def width  800)
(def height 450)

(rl:init-window width height "Elle + Raylib")
(rl:set-target-fps 60)

# Ball state
(def @bx (/ (float width) 2.0))
(def @by (/ (float height) 2.0))
(def @dx 4.0)
(def @dy 3.0)
(def radius 20.0)

(while (not (rl:window-should-close))
  # Update
  (assign bx (+ bx dx))
  (assign by (+ by dy))
  (when (or (>= bx (- (float width) radius)) (<= bx radius))
    (assign dx (- 0.0 dx)))
  (when (or (>= by (- (float height) radius)) (<= by radius))
    (assign dy (- 0.0 dy)))

  # Teleport on click
  (when (rl:mouse-button-pressed? rl:MOUSE_LEFT)
    (assign bx (float (rl:mouse-x)))
    (assign by (float (rl:mouse-y))))

  # Draw
  (rl:begin-drawing)
  (rl:clear rl:RAYWHITE)
  (rl:draw-text "Elle + Raylib" 10 10 20 rl:DARKGRAY)
  (rl:draw-text "Click to teleport the ball" 10 40 16 rl:GRAY)
  (rl:draw-circle (int bx) (int by) radius rl:RED)
  (rl:draw-circle-lines (int bx) (int by) radius rl:MAROON)
  (rl:draw-fps 10 (- height 30))
  (rl:end-drawing))

(rl:close-window)
