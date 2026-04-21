(elle/epoch 8)
# tests/elle/lib/raylib.lisp — raylib module smoke tests
#
# Tests module loading, constructors, constants, struct sizes, and accessors.
# Does NOT open a window — all tests are non-graphical.

(def [ok rl] (protect ((import "std/raylib"))))
(unless ok
  (println "raylib: skipping — libraylib.so not available")
  (exit 0))

(println "raylib: module loaded")

# ── Color constants ──────────────────────────────────────────────────

(assert (= (length rl:RED) 4) "RED has 4 components")
(assert (= (get rl:RED 0) 230) "RED r=230")
(assert (= (get rl:RED 1) 41) "RED g=41")
(assert (= (get rl:RED 2) 55) "RED b=55")
(assert (= (get rl:RED 3) 255) "RED a=255")

(assert (= rl:WHITE [255 255 255 255]) "WHITE")
(assert (= rl:BLACK [0 0 0 255]) "BLACK")
(assert (= rl:BLANK [0 0 0 0]) "BLANK")
(assert (= rl:RAYWHITE [245 245 245 255]) "RAYWHITE")

# ── Key constants ────────────────────────────────────────────────────

(assert (= rl:KEY_ESCAPE 256) "KEY_ESCAPE")
(assert (= rl:KEY_SPACE 32) "KEY_SPACE")
(assert (= rl:KEY_A 65) "KEY_A")
(assert (= rl:KEY_Z 90) "KEY_Z")
(assert (= rl:KEY_ENTER 257) "KEY_ENTER")
(assert (= rl:KEY_RIGHT 262) "KEY_RIGHT")
(assert (= rl:KEY_UP 265) "KEY_UP")
(assert (= rl:KEY_F1 290) "KEY_F1")
(assert (= rl:KEY_F12 301) "KEY_F12")

# ── Mouse constants ──────────────────────────────────────────────────

(assert (= rl:MOUSE_LEFT 0) "MOUSE_LEFT")
(assert (= rl:MOUSE_RIGHT 1) "MOUSE_RIGHT")
(assert (= rl:MOUSE_MIDDLE 2) "MOUSE_MIDDLE")

# ── Config flag constants ────────────────────────────────────────────

(assert (= rl:FLAG_VSYNC 0x40) "FLAG_VSYNC")
(assert (= rl:FLAG_FULLSCREEN 0x02) "FLAG_FULLSCREEN")
(assert (= rl:FLAG_RESIZABLE 0x04) "FLAG_RESIZABLE")

# ── Camera constants ─────────────────────────────────────────────────

(assert (= rl:CAMERA_PERSPECTIVE 0) "CAMERA_PERSPECTIVE")
(assert (= rl:CAMERA_ORTHOGRAPHIC 1) "CAMERA_ORTHOGRAPHIC")

# ── Blend mode constants ─────────────────────────────────────────────

(assert (= rl:BLEND_ALPHA 0) "BLEND_ALPHA")
(assert (= rl:BLEND_ADDITIVE 1) "BLEND_ADDITIVE")

# ── Constructor: color ───────────────────────────────────────────────

(let [c (rl:color 100 150 200)]
  (assert (= (length c) 4) "color has 4 elements")
  (assert (= (get c 0) 100) "color r")
  (assert (= (get c 1) 150) "color g")
  (assert (= (get c 2) 200) "color b")
  (assert (= (get c 3) 255) "color default alpha"))

(let [c (rl:color 10 20 30 :a 128)]
  (assert (= (get c 3) 128) "color explicit alpha"))

# ── Constructor: vec2 ────────────────────────────────────────────────

(let [v (rl:vec2 3.5 7.25)]
  (assert (= (length v) 2) "vec2 has 2 elements")
  (assert (= (get v 0) 3.5) "vec2 x")
  (assert (= (get v 1) 7.25) "vec2 y"))

# vec2 coerces ints to floats
(let [v (rl:vec2 1 2)]
  (assert (= (type-of (get v 0)) :float) "vec2 coerces to float"))

# ── Constructor: vec3 ────────────────────────────────────────────────

(let [v (rl:vec3 1.0 2.0 3.0)]
  (assert (= (length v) 3) "vec3 has 3 elements")
  (assert (= (get v 0) 1.0) "vec3 x")
  (assert (= (get v 1) 2.0) "vec3 y")
  (assert (= (get v 2) 3.0) "vec3 z"))

# ── Constructor: rect ────────────────────────────────────────────────

(let [r (rl:rect 10 20 100 50)]
  (assert (= (length r) 4) "rect has 4 elements")
  (assert (= (get r 0) 10.0) "rect x")
  (assert (= (get r 1) 20.0) "rect y")
  (assert (= (get r 2) 100.0) "rect w")
  (assert (= (get r 3) 50.0) "rect h"))

# ── Constructor: camera-2d ───────────────────────────────────────────

(let [cam (rl:camera-2d)]
  (assert (= (length cam) 4) "camera-2d has 4 fields")
  (assert (= (get cam 0) [0.0 0.0]) "camera-2d default offset")
  (assert (= (get cam 1) [0.0 0.0]) "camera-2d default target")
  (assert (= (get cam 2) 0.0) "camera-2d default rotation")
  (assert (= (get cam 3) 1.0) "camera-2d default zoom"))

(let [cam (rl:camera-2d :offset [400.0 300.0] :target [100.0 100.0] :zoom 2.0)]
  (assert (= (get cam 0) [400.0 300.0]) "camera-2d custom offset")
  (assert (= (get cam 1) [100.0 100.0]) "camera-2d custom target")
  (assert (= (get cam 3) 2.0) "camera-2d custom zoom"))

# ── Constructor: camera-3d ───────────────────────────────────────────

(let [cam (rl:camera-3d [0.0 10.0 10.0] [0.0 0.0 0.0] [0.0 1.0 0.0])]
  (assert (= (length cam) 5) "camera-3d has 5 fields")
  (assert (= (get cam 0) [0.0 10.0 10.0]) "camera-3d position")
  (assert (= (get cam 1) [0.0 0.0 0.0]) "camera-3d target")
  (assert (= (get cam 2) [0.0 1.0 0.0]) "camera-3d up")
  (assert (= (get cam 3) 45.0) "camera-3d default fovy")
  (assert (= (get cam 4) 0) "camera-3d default projection"))

(let [cam (rl:camera-3d [0.0 0.0 0.0] [0.0 0.0 -1.0] [0.0 1.0 0.0]
            :fovy 90.0 :projection rl:CAMERA_ORTHOGRAPHIC)]
  (assert (= (get cam 3) 90.0) "camera-3d custom fovy")
  (assert (= (get cam 4) 1) "camera-3d orthographic"))

# ── Struct type sizes ────────────────────────────────────────────────

(assert (= (ffi/size (ffi/struct [:u8 :u8 :u8 :u8])) 4) "Color size = 4")
(assert (= (ffi/size (ffi/struct [:float :float])) 8) "Vector2 size = 8")
(assert (= (ffi/size (ffi/struct [:float :float :float])) 12) "Vector3 size = 12")
(assert (= (ffi/size (ffi/struct [:float :float :float :float])) 16) "Rectangle size = 16")
(assert (= (ffi/size (ffi/struct [:uint :int :int :int :int])) 20) "Texture2D size = 20")

# ── Module API surface ───────────────────────────────────────────────
# Verify key exports exist (they should be functions or values, not nil)

(assert (not (nil? rl:init-window)) "init-window exported")
(assert (not (nil? rl:close-window)) "close-window exported")
(assert (not (nil? rl:window-should-close)) "window-should-close exported")
(assert (not (nil? rl:set-target-fps)) "set-target-fps exported")
(assert (not (nil? rl:begin-drawing)) "begin-drawing exported")
(assert (not (nil? rl:end-drawing)) "end-drawing exported")
(assert (not (nil? rl:with-drawing)) "with-drawing exported")
(assert (not (nil? rl:clear)) "clear exported")
(assert (not (nil? rl:draw-text)) "draw-text exported")
(assert (not (nil? rl:draw-rect)) "draw-rect exported")
(assert (not (nil? rl:draw-circle)) "draw-circle exported")
(assert (not (nil? rl:draw-line)) "draw-line exported")
(assert (not (nil? rl:key-pressed?)) "key-pressed? exported")
(assert (not (nil? rl:key-down?)) "key-down? exported")
(assert (not (nil? rl:mouse-position)) "mouse-position exported")
(assert (not (nil? rl:load-texture)) "load-texture exported")
(assert (not (nil? rl:load-font)) "load-font exported")
(assert (not (nil? rl:load-sound)) "load-sound exported")
(assert (not (nil? rl:collision-recs?)) "collision-recs? exported")
(assert (not (nil? rl:fade)) "fade exported")
(assert (not (nil? rl:measure-text)) "measure-text exported")
(assert (not (nil? rl:draw-cube)) "draw-cube exported")
(assert (not (nil? rl:draw-sphere)) "draw-sphere exported")

(println "raylib: all tests passed")
