(elle/epoch 9)
## std/raylib — Raylib bindings for Elle via FFI
##
## Pure FFI bindings to libraylib (v5.5). No Rust plugin needed.
##
## Dependencies:
##   - libraylib.so installed on the system
##   - ffi primitives (ffi/native, ffi/defbind, ffi/malloc, etc.)
##
## Struct representation:
##   Raylib structs are represented as Elle arrays matching C field order.
##   Color = [r g b a], Vector2 = [x y], Rectangle = [x y w h], etc.
##   Constructor functions (color, vec2, rect, ...) build these arrays.
##   Predefined color constants (RED, BLUE, ...) are ready-to-use arrays.
##
## Usage:
##   (def rl ((import "std/raylib")))
##   (rl:init-window 800 450 "Hello")
##   (rl:set-target-fps 60)
##   (while (not (rl:window-should-close))
##     (rl:begin-drawing)
##     (rl:clear rl:RAYWHITE)
##     (rl:draw-text "Hello, Elle!" 190 200 20 rl:DARKGRAY)
##     (rl:draw-fps 10 10)
##     (rl:end-drawing))
##   (rl:close-window)

(fn []

# ── Load libraylib ───────────────────────────────────────────────────

(def lib (ffi/native "libraylib.so"))

# ── Struct types ─────────────────────────────────────────────────────
#
# Raylib passes most structs by value. Elle's FFI marshals arrays to/from
# C struct layout automatically. Each struct value is an Elle array whose
# elements correspond to the C fields in order.

(def color-type    (ffi/struct [:u8 :u8 :u8 :u8]))
(def vec2-type     (ffi/struct [:float :float]))
(def vec3-type     (ffi/struct [:float :float :float]))
(def vec4-type     (ffi/struct [:float :float :float :float]))
(def rect-type     (ffi/struct [:float :float :float :float]))
(def texture-type  (ffi/struct [:uint :int :int :int :int]))
(def image-type    (ffi/struct [:ptr :int :int :int :int]))
(def camera2d-type (ffi/struct [vec2-type vec2-type :float :float]))
(def camera3d-type (ffi/struct [vec3-type vec3-type vec3-type :float :int]))
(def font-type     (ffi/struct [:int :int :int texture-type :ptr :ptr]))
(def audiostream-type (ffi/struct [:ptr :ptr :uint :uint :uint]))
(def sound-type    (ffi/struct [audiostream-type :uint]))

# ── Color constants ──────────────────────────────────────────────────

(def LIGHTGRAY [200 200 200 255])
(def GRAY      [130 130 130 255])
(def DARKGRAY  [80 80 80 255])
(def YELLOW    [253 249 0 255])
(def GOLD      [255 203 0 255])
(def ORANGE    [255 161 0 255])
(def PINK      [255 109 194 255])
(def RED       [230 41 55 255])
(def MAROON    [190 33 55 255])
(def GREEN     [0 228 48 255])
(def LIME      [0 158 47 255])
(def DARKGREEN [0 117 44 255])
(def SKYBLUE   [102 191 255 255])
(def BLUE      [0 121 241 255])
(def DARKBLUE  [0 82 172 255])
(def PURPLE    [200 122 255 255])
(def VIOLET    [135 60 190 255])
(def MAGENTA   [255 0 255 255])
(def BROWN     [127 106 79 255])
(def BLACK     [0 0 0 255])
(def WHITE     [255 255 255 255])
(def BLANK     [0 0 0 0])
(def RAYWHITE  [245 245 245 255])

# ── Key constants ────────────────────────────────────────────────────

(def KEY_NULL       0)
(def KEY_SPACE     32)
(def KEY_APOSTROPHE 39)
(def KEY_COMMA     44)
(def KEY_MINUS     45)
(def KEY_PERIOD    46)
(def KEY_SLASH     47)
(def KEY_ZERO      48) (def KEY_ONE   49) (def KEY_TWO   50)
(def KEY_THREE     51) (def KEY_FOUR  52) (def KEY_FIVE  53)
(def KEY_SIX       54) (def KEY_SEVEN 55) (def KEY_EIGHT 56)
(def KEY_NINE      57)
(def KEY_SEMICOLON 59) (def KEY_EQUAL 61)
(def KEY_A 65) (def KEY_B 66) (def KEY_C 67) (def KEY_D 68)
(def KEY_E 69) (def KEY_F 70) (def KEY_G 71) (def KEY_H 72)
(def KEY_I 73) (def KEY_J 74) (def KEY_K 75) (def KEY_L 76)
(def KEY_M 77) (def KEY_N 78) (def KEY_O 79) (def KEY_P 80)
(def KEY_Q 81) (def KEY_R 82) (def KEY_S 83) (def KEY_T 84)
(def KEY_U 85) (def KEY_V 86) (def KEY_W 87) (def KEY_X 88)
(def KEY_Y 89) (def KEY_Z 90)
(def KEY_ESCAPE    256) (def KEY_ENTER     257)
(def KEY_TAB       258) (def KEY_BACKSPACE 259)
(def KEY_INSERT    260) (def KEY_DELETE    261)
(def KEY_RIGHT     262) (def KEY_LEFT      263)
(def KEY_DOWN      264) (def KEY_UP        265)
(def KEY_PAGE_UP   266) (def KEY_PAGE_DOWN 267)
(def KEY_HOME      268) (def KEY_END       269)
(def KEY_CAPS_LOCK 280) (def KEY_SCROLL_LOCK 281)
(def KEY_NUM_LOCK  282) (def KEY_PRINT_SCREEN 283)
(def KEY_PAUSE     284)
(def KEY_F1  290) (def KEY_F2  291) (def KEY_F3  292)
(def KEY_F4  293) (def KEY_F5  294) (def KEY_F6  295)
(def KEY_F7  296) (def KEY_F8  297) (def KEY_F9  298)
(def KEY_F10 299) (def KEY_F11 300) (def KEY_F12 301)
(def KEY_LEFT_SHIFT    340) (def KEY_LEFT_CONTROL  341)
(def KEY_LEFT_ALT      342) (def KEY_LEFT_SUPER    343)
(def KEY_RIGHT_SHIFT   344) (def KEY_RIGHT_CONTROL 345)
(def KEY_RIGHT_ALT     346) (def KEY_RIGHT_SUPER   347)
(def KEY_KP_0 320) (def KEY_KP_1 321) (def KEY_KP_2 322)
(def KEY_KP_3 323) (def KEY_KP_4 324) (def KEY_KP_5 325)
(def KEY_KP_6 326) (def KEY_KP_7 327) (def KEY_KP_8 328)
(def KEY_KP_9 329)
(def KEY_KP_DECIMAL  330) (def KEY_KP_DIVIDE   331)
(def KEY_KP_MULTIPLY 332) (def KEY_KP_SUBTRACT 333)
(def KEY_KP_ADD      334) (def KEY_KP_ENTER    335)
(def KEY_KP_EQUAL    336)

# ── Mouse button constants ───────────────────────────────────────────

(def MOUSE_LEFT   0)
(def MOUSE_RIGHT  1)
(def MOUSE_MIDDLE 2)
(def MOUSE_SIDE   3)
(def MOUSE_EXTRA  4)

# ── Gamepad constants ────────────────────────────────────────────────

(def GAMEPAD_BUTTON_LEFT_FACE_UP     1)
(def GAMEPAD_BUTTON_LEFT_FACE_RIGHT  2)
(def GAMEPAD_BUTTON_LEFT_FACE_DOWN   3)
(def GAMEPAD_BUTTON_LEFT_FACE_LEFT   4)
(def GAMEPAD_BUTTON_RIGHT_FACE_UP    5)
(def GAMEPAD_BUTTON_RIGHT_FACE_RIGHT 6)
(def GAMEPAD_BUTTON_RIGHT_FACE_DOWN  7)
(def GAMEPAD_BUTTON_RIGHT_FACE_LEFT  8)
(def GAMEPAD_BUTTON_LEFT_TRIGGER_1   9)
(def GAMEPAD_BUTTON_LEFT_TRIGGER_2  10)
(def GAMEPAD_BUTTON_RIGHT_TRIGGER_1 11)
(def GAMEPAD_BUTTON_RIGHT_TRIGGER_2 12)
(def GAMEPAD_BUTTON_MIDDLE_LEFT     13)
(def GAMEPAD_BUTTON_MIDDLE          14)
(def GAMEPAD_BUTTON_MIDDLE_RIGHT    15)
(def GAMEPAD_AXIS_LEFT_X  0) (def GAMEPAD_AXIS_LEFT_Y  1)
(def GAMEPAD_AXIS_RIGHT_X 2) (def GAMEPAD_AXIS_RIGHT_Y 3)
(def GAMEPAD_AXIS_LEFT_TRIGGER  4)
(def GAMEPAD_AXIS_RIGHT_TRIGGER 5)

# ── Config flags ─────────────────────────────────────────────────────

(def FLAG_VSYNC       0x00000040)
(def FLAG_FULLSCREEN  0x00000002)
(def FLAG_RESIZABLE   0x00000004)
(def FLAG_UNDECORATED 0x00000008)
(def FLAG_HIDDEN      0x00000080)
(def FLAG_MINIMIZED   0x00000200)
(def FLAG_MAXIMIZED   0x00000400)
(def FLAG_UNFOCUSED   0x00000800)
(def FLAG_TOPMOST     0x00001000)
(def FLAG_ALWAYS_RUN  0x00000100)
(def FLAG_TRANSPARENT 0x00000010)
(def FLAG_HIGHDPI     0x00002000)
(def FLAG_MSAA_4X     0x00000020)

# ── Camera projection ───────────────────────────────────────────────

(def CAMERA_PERSPECTIVE  0)
(def CAMERA_ORTHOGRAPHIC 1)

# ── Blend modes ──────────────────────────────────────────────────────

(def BLEND_ALPHA             0)
(def BLEND_ADDITIVE          1)
(def BLEND_MULTIPLIED        2)
(def BLEND_ADD_COLORS        3)
(def BLEND_SUBTRACT_COLORS   4)
(def BLEND_ALPHA_PREMULTIPLY 5)

# ── Raw bindings: Window ─────────────────────────────────────────────

(ffi/defbind c-init-window         lib "InitWindow"         :void [:int :int :string])
(ffi/defbind c-close-window        lib "CloseWindow"        :void [])
(ffi/defbind c-window-should-close lib "WindowShouldClose"  :bool [])
(ffi/defbind c-is-window-ready     lib "IsWindowReady"      :bool [])
(ffi/defbind c-is-window-fullscreen lib "IsWindowFullscreen" :bool [])
(ffi/defbind c-is-window-hidden    lib "IsWindowHidden"     :bool [])
(ffi/defbind c-is-window-minimized lib "IsWindowMinimized"  :bool [])
(ffi/defbind c-is-window-maximized lib "IsWindowMaximized"  :bool [])
(ffi/defbind c-is-window-focused   lib "IsWindowFocused"    :bool [])
(ffi/defbind c-is-window-resized   lib "IsWindowResized"    :bool [])
(ffi/defbind c-set-window-state    lib "SetWindowState"     :void [:uint])
(ffi/defbind c-clear-window-state  lib "ClearWindowState"   :void [:uint])
(ffi/defbind c-toggle-fullscreen   lib "ToggleFullscreen"   :void [])
(ffi/defbind c-set-window-title    lib "SetWindowTitle"     :void [:string])
(ffi/defbind c-set-window-position lib "SetWindowPosition"  :void [:int :int])
(ffi/defbind c-set-window-size     lib "SetWindowSize"      :void [:int :int])
(ffi/defbind c-get-screen-width    lib "GetScreenWidth"     :int  [])
(ffi/defbind c-get-screen-height   lib "GetScreenHeight"    :int  [])
(ffi/defbind c-get-render-width    lib "GetRenderWidth"     :int  [])
(ffi/defbind c-get-render-height   lib "GetRenderHeight"    :int  [])

# ── Raw bindings: Timing ─────────────────────────────────────────────

(ffi/defbind c-set-target-fps lib "SetTargetFPS" :void [:int])
(ffi/defbind c-get-fps        lib "GetFPS"       :int  [])
(ffi/defbind c-get-frame-time lib "GetFrameTime" :float [])
(ffi/defbind c-get-time       lib "GetTime"      :double [])

# ── Raw bindings: Drawing ────────────────────────────────────────────

(ffi/defbind c-clear-background   lib "ClearBackground"   :void [color-type])
(ffi/defbind c-begin-drawing      lib "BeginDrawing"      :void [])
(ffi/defbind c-end-drawing        lib "EndDrawing"        :void [])
(ffi/defbind c-begin-mode-2d      lib "BeginMode2D"       :void [camera2d-type])
(ffi/defbind c-end-mode-2d        lib "EndMode2D"         :void [])
(ffi/defbind c-begin-mode-3d      lib "BeginMode3D"       :void [camera3d-type])
(ffi/defbind c-end-mode-3d        lib "EndMode3D"         :void [])
(ffi/defbind c-begin-blend-mode   lib "BeginBlendMode"    :void [:int])
(ffi/defbind c-end-blend-mode     lib "EndBlendMode"      :void [])
(ffi/defbind c-begin-scissor-mode lib "BeginScissorMode"  :void [:int :int :int :int])
(ffi/defbind c-end-scissor-mode   lib "EndScissorMode"    :void [])

# ── Raw bindings: Shapes ─────────────────────────────────────────────

(ffi/defbind c-draw-pixel       lib "DrawPixel"       :void [:int :int color-type])
(ffi/defbind c-draw-line        lib "DrawLine"        :void [:int :int :int :int color-type])
(ffi/defbind c-draw-line-ex     lib "DrawLineEx"      :void [vec2-type vec2-type :float color-type])
(ffi/defbind c-draw-line-bezier lib "DrawLineBezier"   :void [vec2-type vec2-type :float color-type])
(ffi/defbind c-draw-circle      lib "DrawCircle"      :void [:int :int :float color-type])
(ffi/defbind c-draw-circle-gradient lib "DrawCircleGradient" :void [:int :int :float color-type color-type])
(ffi/defbind c-draw-circle-lines lib "DrawCircleLines" :void [:int :int :float color-type])
(ffi/defbind c-draw-ellipse     lib "DrawEllipse"     :void [:int :int :float :float color-type])
(ffi/defbind c-draw-rectangle   lib "DrawRectangle"   :void [:int :int :int :int color-type])
(ffi/defbind c-draw-rectangle-rec lib "DrawRectangleRec" :void [rect-type color-type])
(ffi/defbind c-draw-rectangle-pro lib "DrawRectanglePro" :void [rect-type vec2-type :float color-type])
(ffi/defbind c-draw-rectangle-gradient-v lib "DrawRectangleGradientV" :void [:int :int :int :int color-type color-type])
(ffi/defbind c-draw-rectangle-gradient-h lib "DrawRectangleGradientH" :void [:int :int :int :int color-type color-type])
(ffi/defbind c-draw-rectangle-lines lib "DrawRectangleLines" :void [:int :int :int :int color-type])
(ffi/defbind c-draw-rectangle-lines-ex lib "DrawRectangleLinesEx" :void [rect-type :float color-type])
(ffi/defbind c-draw-rectangle-rounded lib "DrawRectangleRounded" :void [rect-type :float :int color-type])
(ffi/defbind c-draw-rectangle-rounded-lines lib "DrawRectangleRoundedLines" :void [rect-type :float :int color-type])
(ffi/defbind c-draw-rectangle-rounded-lines-ex lib "DrawRectangleRoundedLinesEx" :void [rect-type :float :int :float color-type])
(ffi/defbind c-draw-triangle      lib "DrawTriangle"      :void [vec2-type vec2-type vec2-type color-type])
(ffi/defbind c-draw-triangle-lines lib "DrawTriangleLines" :void [vec2-type vec2-type vec2-type color-type])
(ffi/defbind c-draw-poly          lib "DrawPoly"          :void [vec2-type :int :float :float color-type])
(ffi/defbind c-draw-poly-lines    lib "DrawPolyLines"     :void [vec2-type :int :float :float color-type])
(ffi/defbind c-draw-ring          lib "DrawRing"          :void [vec2-type :float :float :float :float :int color-type])
(ffi/defbind c-draw-ring-lines    lib "DrawRingLines"     :void [vec2-type :float :float :float :float :int color-type])

# ── Raw bindings: 3D Shapes ──────────────────────────────────────────

(ffi/defbind c-draw-line-3d        lib "DrawLine3D"        :void [vec3-type vec3-type color-type])
(ffi/defbind c-draw-cube           lib "DrawCube"          :void [vec3-type :float :float :float color-type])
(ffi/defbind c-draw-cube-wires     lib "DrawCubeWires"     :void [vec3-type :float :float :float color-type])
(ffi/defbind c-draw-sphere         lib "DrawSphere"        :void [vec3-type :float color-type])
(ffi/defbind c-draw-sphere-ex      lib "DrawSphereEx"      :void [vec3-type :float :int :int color-type])
(ffi/defbind c-draw-sphere-wires   lib "DrawSphereWires"   :void [vec3-type :float :int :int color-type])
(ffi/defbind c-draw-cylinder       lib "DrawCylinder"      :void [vec3-type :float :float :float :int color-type])
(ffi/defbind c-draw-cylinder-wires lib "DrawCylinderWires"  :void [vec3-type :float :float :float :int color-type])
(ffi/defbind c-draw-plane          lib "DrawPlane"         :void [vec3-type vec2-type color-type])
(ffi/defbind c-draw-grid           lib "DrawGrid"          :void [:int :float])

# ── Raw bindings: Text ───────────────────────────────────────────────

(ffi/defbind c-draw-fps         lib "DrawFPS"           :void [:int :int])
(ffi/defbind c-draw-text        lib "DrawText"          :void [:string :int :int :int color-type])
(ffi/defbind c-draw-text-ex     lib "DrawTextEx"        :void [font-type :string vec2-type :float :float color-type])
(ffi/defbind c-draw-text-pro    lib "DrawTextPro"       :void [font-type :string vec2-type vec2-type :float :float :float color-type])
(ffi/defbind c-measure-text     lib "MeasureText"       :int  [:string :int])
(ffi/defbind c-measure-text-ex  lib "MeasureTextEx"     vec2-type [font-type :string :float :float])
(ffi/defbind c-get-font-default lib "GetFontDefault"    font-type [])
(ffi/defbind c-load-font        lib "LoadFont"          font-type [:string])
(ffi/defbind c-load-font-ex     lib "LoadFontEx"        font-type [:string :int :ptr :int])
(ffi/defbind c-unload-font      lib "UnloadFont"        :void [font-type])
(ffi/defbind c-set-text-line-spacing lib "SetTextLineSpacing" :void [:int])

# ── Raw bindings: Textures ───────────────────────────────────────────

(ffi/defbind c-load-texture            lib "LoadTexture"            texture-type [:string])
(ffi/defbind c-load-texture-from-image lib "LoadTextureFromImage"   texture-type [image-type])
(ffi/defbind c-unload-texture          lib "UnloadTexture"          :void [texture-type])
(ffi/defbind c-draw-texture            lib "DrawTexture"            :void [texture-type :int :int color-type])
(ffi/defbind c-draw-texture-v          lib "DrawTextureV"           :void [texture-type vec2-type color-type])
(ffi/defbind c-draw-texture-ex         lib "DrawTextureEx"          :void [texture-type vec2-type :float :float color-type])
(ffi/defbind c-draw-texture-rec        lib "DrawTextureRec"         :void [texture-type rect-type vec2-type color-type])
(ffi/defbind c-draw-texture-pro        lib "DrawTexturePro"         :void [texture-type rect-type rect-type vec2-type :float color-type])

# ── Raw bindings: Image ──────────────────────────────────────────────

(ffi/defbind c-load-image        lib "LoadImage"          image-type [:string])
(ffi/defbind c-unload-image      lib "UnloadImage"        :void [image-type])
(ffi/defbind c-export-image      lib "ExportImage"        :bool [image-type :string])
(ffi/defbind c-gen-image-color   lib "GenImageColor"      image-type [:int :int color-type])

# ── Raw bindings: Input ──────────────────────────────────────────────

(ffi/defbind c-is-key-pressed    lib "IsKeyPressed"    :bool [:int])
(ffi/defbind c-is-key-down       lib "IsKeyDown"       :bool [:int])
(ffi/defbind c-is-key-released   lib "IsKeyReleased"   :bool [:int])
(ffi/defbind c-is-key-up         lib "IsKeyUp"         :bool [:int])
(ffi/defbind c-get-key-pressed   lib "GetKeyPressed"   :int  [])
(ffi/defbind c-get-char-pressed  lib "GetCharPressed"  :int  [])
(ffi/defbind c-set-exit-key      lib "SetExitKey"      :void [:int])

(ffi/defbind c-is-mouse-button-pressed  lib "IsMouseButtonPressed"  :bool [:int])
(ffi/defbind c-is-mouse-button-down     lib "IsMouseButtonDown"     :bool [:int])
(ffi/defbind c-is-mouse-button-released lib "IsMouseButtonReleased" :bool [:int])
(ffi/defbind c-is-mouse-button-up       lib "IsMouseButtonUp"       :bool [:int])
(ffi/defbind c-get-mouse-x              lib "GetMouseX"             :int  [])
(ffi/defbind c-get-mouse-y              lib "GetMouseY"             :int  [])
(ffi/defbind c-get-mouse-position       lib "GetMousePosition"      vec2-type [])
(ffi/defbind c-get-mouse-delta          lib "GetMouseDelta"         vec2-type [])
(ffi/defbind c-set-mouse-position       lib "SetMousePosition"      :void [:int :int])
(ffi/defbind c-get-mouse-wheel-move     lib "GetMouseWheelMove"     :float [])
(ffi/defbind c-set-mouse-cursor         lib "SetMouseCursor"        :void [:int])
(ffi/defbind c-show-cursor              lib "ShowCursor"            :void [])
(ffi/defbind c-hide-cursor              lib "HideCursor"            :void [])
(ffi/defbind c-is-cursor-hidden         lib "IsCursorHidden"        :bool [])

(ffi/defbind c-is-gamepad-available     lib "IsGamepadAvailable"    :bool [:int])
(ffi/defbind c-get-gamepad-name         lib "GetGamepadName"        :ptr  [:int])
(ffi/defbind c-is-gamepad-button-pressed lib "IsGamepadButtonPressed" :bool [:int :int])
(ffi/defbind c-is-gamepad-button-down   lib "IsGamepadButtonDown"   :bool [:int :int])
(ffi/defbind c-get-gamepad-axis-movement lib "GetGamepadAxisMovement" :float [:int :int])

(ffi/defbind c-get-touch-x            lib "GetTouchX"           :int [])
(ffi/defbind c-get-touch-y            lib "GetTouchY"           :int [])
(ffi/defbind c-get-touch-position     lib "GetTouchPosition"    vec2-type [:int])
(ffi/defbind c-get-touch-point-count  lib "GetTouchPointCount"  :int [])

# ── Raw bindings: Audio ──────────────────────────────────────────────

(ffi/defbind c-init-audio-device    lib "InitAudioDevice"    :void [])
(ffi/defbind c-close-audio-device   lib "CloseAudioDevice"   :void [])
(ffi/defbind c-is-audio-device-ready lib "IsAudioDeviceReady" :bool [])
(ffi/defbind c-set-master-volume    lib "SetMasterVolume"    :void [:float])
(ffi/defbind c-get-master-volume    lib "GetMasterVolume"    :float [])
(ffi/defbind c-load-sound           lib "LoadSound"          sound-type [:string])
(ffi/defbind c-unload-sound         lib "UnloadSound"        :void [sound-type])
(ffi/defbind c-play-sound           lib "PlaySound"          :void [sound-type])
(ffi/defbind c-stop-sound           lib "StopSound"          :void [sound-type])
(ffi/defbind c-pause-sound          lib "PauseSound"         :void [sound-type])
(ffi/defbind c-resume-sound         lib "ResumeSound"        :void [sound-type])
(ffi/defbind c-is-sound-playing     lib "IsSoundPlaying"     :bool [sound-type])
(ffi/defbind c-set-sound-volume     lib "SetSoundVolume"     :void [sound-type :float])
(ffi/defbind c-set-sound-pitch      lib "SetSoundPitch"      :void [sound-type :float])
(ffi/defbind c-set-sound-pan        lib "SetSoundPan"        :void [sound-type :float])

# ── Raw bindings: Collision ──────────────────────────────────────────

(ffi/defbind c-check-collision-recs       lib "CheckCollisionRecs"       :bool [rect-type rect-type])
(ffi/defbind c-check-collision-circles    lib "CheckCollisionCircles"    :bool [vec2-type :float vec2-type :float])
(ffi/defbind c-check-collision-circle-rec lib "CheckCollisionCircleRec"  :bool [vec2-type :float rect-type])
(ffi/defbind c-check-collision-point-rec  lib "CheckCollisionPointRec"   :bool [vec2-type rect-type])
(ffi/defbind c-check-collision-point-circle lib "CheckCollisionPointCircle" :bool [vec2-type vec2-type :float])
(ffi/defbind c-get-collision-rec          lib "GetCollisionRec"          rect-type [rect-type rect-type])

# ── Raw bindings: Color ──────────────────────────────────────────────

(ffi/defbind c-fade            lib "Fade"            color-type [color-type :float])
(ffi/defbind c-color-from-hsv  lib "ColorFromHSV"    color-type [:float :float :float])
(ffi/defbind c-color-alpha     lib "ColorAlpha"       color-type [color-type :float])
(ffi/defbind c-color-brightness lib "ColorBrightness" color-type [color-type :float])
(ffi/defbind c-color-tint      lib "ColorTint"        color-type [color-type color-type])

# ── Raw bindings: Misc ───────────────────────────────────────────────

(ffi/defbind c-set-config-flags    lib "SetConfigFlags"    :void [:uint])
(ffi/defbind c-set-trace-log-level lib "SetTraceLogLevel"  :void [:int])
(ffi/defbind c-get-random-value    lib "GetRandomValue"    :int  [:int :int])
(ffi/defbind c-take-screenshot     lib "TakeScreenshot"    :void [:string])
(ffi/defbind c-open-url            lib "OpenURL"           :void [:string])

# ── Constructors ─────────────────────────────────────────────────────

(defn color [r g b &named @a]
  "Create a color [r g b a]. Alpha defaults to 255."
  (default a 255)
  [r g b a])

(defn vec2 [x y]
  "Create a Vector2 [x y]."
  [(float x) (float y)])

(defn vec3 [x y z]
  "Create a Vector3 [x y z]."
  [(float x) (float y) (float z)])

(defn rect [x y w h]
  "Create a Rectangle [x y width height]."
  [(float x) (float y) (float w) (float h)])

(defn camera-2d [&named @offset @target @rotation @zoom]
  "Create a Camera2D. Defaults: offset [0 0], target [0 0], rotation 0, zoom 1."
  (default offset [0.0 0.0])
  (default target [0.0 0.0])
  (default rotation 0.0)
  (default zoom 1.0)
  [offset target rotation zoom])

(defn camera-3d [pos target up &named @fovy @projection]
  "Create a Camera3D. fovy defaults to 45.0, projection to perspective."
  (default fovy 45.0)
  (default projection CAMERA_PERSPECTIVE)
  [pos target up fovy projection])

# ── Accessors ────────────────────────────────────────────────────────

(defn texture-width [tex]  (get tex 1))
(defn texture-height [tex] (get tex 2))
(defn texture-size [tex]   {:width (get tex 1) :height (get tex 2)})

# ── Public API: Window ───────────────────────────────────────────────

(defn init-window [width height title]
  "Initialize window and OpenGL context."
  (c-init-window width height title))

(defn close-window []
  "Close window and unload OpenGL context."
  (c-close-window))

(defn window-should-close []
  "Check if ESC pressed or close button clicked."
  (c-window-should-close))

(defn window-ready? []       (c-is-window-ready))
(defn window-fullscreen? []  (c-is-window-fullscreen))
(defn window-hidden? []      (c-is-window-hidden))
(defn window-minimized? []   (c-is-window-minimized))
(defn window-maximized? []   (c-is-window-maximized))
(defn window-focused? []     (c-is-window-focused))
(defn window-resized? []     (c-is-window-resized))

(defn set-window-state [flags]    (c-set-window-state flags))
(defn clear-window-state [flags]  (c-clear-window-state flags))
(defn toggle-fullscreen []        (c-toggle-fullscreen))
(defn set-window-title [title]    (c-set-window-title title))
(defn set-window-position [x y]   (c-set-window-position x y))
(defn set-window-size [w h]       (c-set-window-size w h))
(defn screen-width []             (c-get-screen-width))
(defn screen-height []            (c-get-screen-height))
(defn render-width []             (c-get-render-width))
(defn render-height []            (c-get-render-height))

# ── Public API: Timing ───────────────────────────────────────────────

(defn set-target-fps [fps]  (c-set-target-fps fps))
(defn fps []                (c-get-fps))
(defn frame-time []         (c-get-frame-time))
(defn elapsed-time []       (c-get-time))

# ── Public API: Drawing ──────────────────────────────────────────────

(defn clear [col]       (c-clear-background col))
(defn begin-drawing []  (c-begin-drawing))
(defn end-drawing []    (c-end-drawing))

(defn with-drawing [body-fn]
  "Call body-fn between BeginDrawing/EndDrawing."
  (c-begin-drawing)
  (defer (c-end-drawing) (body-fn)))

(defn begin-mode-2d [cam] (c-begin-mode-2d cam))
(defn end-mode-2d []      (c-end-mode-2d))

(defn with-mode-2d [cam body-fn]
  "Call body-fn between BeginMode2D/EndMode2D."
  (c-begin-mode-2d cam)
  (defer (c-end-mode-2d) (body-fn)))

(defn begin-mode-3d [cam] (c-begin-mode-3d cam))
(defn end-mode-3d []      (c-end-mode-3d))

(defn with-mode-3d [cam body-fn]
  "Call body-fn between BeginMode3D/EndMode3D."
  (c-begin-mode-3d cam)
  (defer (c-end-mode-3d) (body-fn)))

(defn begin-blend-mode [mode] (c-begin-blend-mode mode))
(defn end-blend-mode []       (c-end-blend-mode))
(defn begin-scissor-mode [x y w h] (c-begin-scissor-mode x y w h))
(defn end-scissor-mode []           (c-end-scissor-mode))

# ── Public API: Shapes ───────────────────────────────────────────────

(defn draw-pixel [x y col]
  (c-draw-pixel x y col))

(defn draw-line [x1 y1 x2 y2 col]
  (c-draw-line x1 y1 x2 y2 col))

(defn draw-line-ex [start end thick col]
  "start/end are [x y] arrays."
  (c-draw-line-ex start end (float thick) col))

(defn draw-line-bezier [start end thick col]
  "Draw line using cubic-bezier interpolation."
  (c-draw-line-bezier start end (float thick) col))

(defn draw-circle [cx cy radius col]
  (c-draw-circle cx cy (float radius) col))

(defn draw-circle-gradient [cx cy radius inner outer]
  (c-draw-circle-gradient cx cy (float radius) inner outer))

(defn draw-circle-lines [cx cy radius col]
  (c-draw-circle-lines cx cy (float radius) col))

(defn draw-ellipse [cx cy rx ry col]
  (c-draw-ellipse cx cy (float rx) (float ry) col))

(defn draw-rect [x y w h col]
  (c-draw-rectangle x y w h col))

(defn draw-rect-rec [rec col]
  "rec is [x y w h]."
  (c-draw-rectangle-rec rec col))

(defn draw-rect-pro [rec origin rotation col]
  "rec is [x y w h], origin is [x y]."
  (c-draw-rectangle-pro rec origin (float rotation) col))

(defn draw-rect-gradient-v [x y w h top bottom]
  (c-draw-rectangle-gradient-v x y w h top bottom))

(defn draw-rect-gradient-h [x y w h left right]
  (c-draw-rectangle-gradient-h x y w h left right))

(defn draw-rect-lines [x y w h col]
  (c-draw-rectangle-lines x y w h col))

(defn draw-rect-lines-ex [rec thick col]
  (c-draw-rectangle-lines-ex rec (float thick) col))

(defn draw-rect-rounded [rec roundness segments col]
  (c-draw-rectangle-rounded rec (float roundness) segments col))

(defn draw-rect-rounded-lines [rec roundness segments col]
  (c-draw-rectangle-rounded-lines rec (float roundness) segments col))

(defn draw-rect-rounded-lines-ex [rec roundness segments thick col]
  (c-draw-rectangle-rounded-lines-ex rec (float roundness) segments (float thick) col))

(defn draw-triangle [v1 v2 v3 col]
  "v1/v2/v3 are [x y] arrays."
  (c-draw-triangle v1 v2 v3 col))

(defn draw-triangle-lines [v1 v2 v3 col]
  (c-draw-triangle-lines v1 v2 v3 col))

(defn draw-poly [center sides radius rotation col]
  (c-draw-poly center sides (float radius) (float rotation) col))

(defn draw-poly-lines [center sides radius rotation col]
  (c-draw-poly-lines center sides (float radius) (float rotation) col))

(defn draw-ring [center inner outer start-angle end-angle segments col]
  (c-draw-ring center (float inner) (float outer)
    (float start-angle) (float end-angle) segments col))

(defn draw-ring-lines [center inner outer start-angle end-angle segments col]
  (c-draw-ring-lines center (float inner) (float outer)
    (float start-angle) (float end-angle) segments col))

# ── Public API: 3D Shapes ────────────────────────────────────────────

(defn draw-line-3d [start end col]
  (c-draw-line-3d start end col))

(defn draw-cube [pos w h len col]
  (c-draw-cube pos (float w) (float h) (float len) col))

(defn draw-cube-wires [pos w h len col]
  (c-draw-cube-wires pos (float w) (float h) (float len) col))

(defn draw-sphere [center radius col]
  (c-draw-sphere center (float radius) col))

(defn draw-sphere-ex [center radius rings slices col]
  (c-draw-sphere-ex center (float radius) rings slices col))

(defn draw-sphere-wires [center radius rings slices col]
  (c-draw-sphere-wires center (float radius) rings slices col))

(defn draw-cylinder [pos rtop rbottom h slices col]
  (c-draw-cylinder pos (float rtop) (float rbottom) (float h) slices col))

(defn draw-cylinder-wires [pos rtop rbottom h slices col]
  (c-draw-cylinder-wires pos (float rtop) (float rbottom) (float h) slices col))

(defn draw-plane [center size col]
  "center is [x y z], size is [x z]."
  (c-draw-plane center size col))

(defn draw-grid [slices spacing]
  (c-draw-grid slices (float spacing)))

# ── Public API: Text ─────────────────────────────────────────────────

(defn draw-fps [x y]
  (c-draw-fps x y))

(defn draw-text [text x y size col]
  (c-draw-text text x y size col))

(defn draw-text-ex [font text pos font-size spacing col]
  "pos is [x y]."
  (c-draw-text-ex font text pos (float font-size) (float spacing) col))

(defn draw-text-pro [font text pos origin rotation font-size spacing col]
  (c-draw-text-pro font text pos origin (float rotation)
    (float font-size) (float spacing) col))

(defn measure-text [text size]
  (c-measure-text text size))

(defn measure-text-ex [font text font-size spacing]
  "Returns [width height]."
  (c-measure-text-ex font text (float font-size) (float spacing)))

(defn default-font []         (c-get-font-default))
(defn load-font [path]        (c-load-font path))

(defn load-font-ex [path size &named codepoints @count]
  (default count 0)
  (c-load-font-ex path size codepoints count))

(defn unload-font [font]      (c-unload-font font))
(defn set-text-line-spacing [n] (c-set-text-line-spacing n))

# ── Public API: Textures ─────────────────────────────────────────────

(defn load-texture [path]     (c-load-texture path))
(defn unload-texture [tex]    (c-unload-texture tex))

(defn draw-texture [tex x y &named @tint]
  (default tint WHITE)
  (c-draw-texture tex x y tint))

(defn draw-texture-v [tex pos &named @tint]
  (default tint WHITE)
  (c-draw-texture-v tex pos tint))

(defn draw-texture-ex [tex pos rotation scale &named @tint]
  (default tint WHITE)
  (c-draw-texture-ex tex pos (float rotation) (float scale) tint))

(defn draw-texture-rec [tex source pos &named @tint]
  (default tint WHITE)
  (c-draw-texture-rec tex source pos tint))

(defn draw-texture-pro [tex source dest origin rotation &named @tint]
  (default tint WHITE)
  (c-draw-texture-pro tex source dest origin (float rotation) tint))

# ── Public API: Image ────────────────────────────────────────────────

(defn load-image [path]            (c-load-image path))
(defn unload-image [img]           (c-unload-image img))
(defn export-image [img path]      (c-export-image img path))
(defn gen-image-color [w h col]    (c-gen-image-color w h col))
(defn load-texture-from-image [img] (c-load-texture-from-image img))

# ── Public API: Input — keyboard ─────────────────────────────────────

(defn key-pressed? [key]   (c-is-key-pressed key))
(defn key-down? [key]      (c-is-key-down key))
(defn key-released? [key]  (c-is-key-released key))
(defn key-up? [key]        (c-is-key-up key))
(defn key-pressed []       (c-get-key-pressed))
(defn char-pressed []      (c-get-char-pressed))
(defn set-exit-key [key]   (c-set-exit-key key))

# ── Public API: Input — mouse ────────────────────────────────────────

(defn mouse-button-pressed? [btn]  (c-is-mouse-button-pressed btn))
(defn mouse-button-down? [btn]     (c-is-mouse-button-down btn))
(defn mouse-button-released? [btn] (c-is-mouse-button-released btn))
(defn mouse-button-up? [btn]       (c-is-mouse-button-up btn))
(defn mouse-x []                   (c-get-mouse-x))
(defn mouse-y []                   (c-get-mouse-y))

(defn mouse-position []
  "Returns {:x x :y y}."
  (let [v (c-get-mouse-position)]
    {:x (get v 0) :y (get v 1)}))

(defn mouse-delta []
  "Returns {:x dx :y dy}."
  (let [v (c-get-mouse-delta)]
    {:x (get v 0) :y (get v 1)}))

(defn set-mouse-position [x y]  (c-set-mouse-position x y))
(defn mouse-wheel []             (c-get-mouse-wheel-move))
(defn set-mouse-cursor [cursor]  (c-set-mouse-cursor cursor))
(defn show-cursor []             (c-show-cursor))
(defn hide-cursor []             (c-hide-cursor))
(defn cursor-hidden? []          (c-is-cursor-hidden))

# ── Public API: Input — gamepad ──────────────────────────────────────

(defn gamepad-available? [pad]           (c-is-gamepad-available pad))
(defn gamepad-button-pressed? [pad btn]  (c-is-gamepad-button-pressed pad btn))
(defn gamepad-button-down? [pad btn]     (c-is-gamepad-button-down pad btn))
(defn gamepad-axis [pad axis]            (c-get-gamepad-axis-movement pad axis))

(defn gamepad-name [pad]
  (let [ptr (c-get-gamepad-name pad)]
    (if (= (ptr/to-int ptr) 0) "" (ffi/string ptr))))

# ── Public API: Input — touch ────────────────────────────────────────

(defn touch-x []             (c-get-touch-x))
(defn touch-y []             (c-get-touch-y))
(defn touch-point-count []   (c-get-touch-point-count))

(defn touch-position [index]
  (let [v (c-get-touch-position index)]
    {:x (get v 0) :y (get v 1)}))

# ── Public API: Audio ────────────────────────────────────────────────

(defn init-audio []          (c-init-audio-device))
(defn close-audio []         (c-close-audio-device))
(defn audio-ready? []        (c-is-audio-device-ready))
(defn set-master-volume [v]  (c-set-master-volume (float v)))
(defn master-volume []       (c-get-master-volume))
(defn load-sound [path]      (c-load-sound path))
(defn unload-sound [snd]     (c-unload-sound snd))
(defn play-sound [snd]       (c-play-sound snd))
(defn stop-sound [snd]       (c-stop-sound snd))
(defn pause-sound [snd]      (c-pause-sound snd))
(defn resume-sound [snd]     (c-resume-sound snd))
(defn sound-playing? [snd]   (c-is-sound-playing snd))
(defn set-sound-volume [snd v] (c-set-sound-volume snd (float v)))
(defn set-sound-pitch [snd p]  (c-set-sound-pitch snd (float p)))
(defn set-sound-pan [snd p]    (c-set-sound-pan snd (float p)))

# ── Public API: Collision ────────────────────────────────────────────

(defn collision-recs? [r1 r2]
  "Check collision between two rectangles."
  (c-check-collision-recs r1 r2))

(defn collision-circles? [c1 r1 c2 r2]
  "Check collision between two circles."
  (c-check-collision-circles c1 (float r1) c2 (float r2)))

(defn collision-circle-rec? [center radius rec]
  (c-check-collision-circle-rec center (float radius) rec))

(defn collision-point-rec? [point rec]
  (c-check-collision-point-rec point rec))

(defn collision-point-circle? [point center radius]
  (c-check-collision-point-circle point center (float radius)))

(defn collision-rec [r1 r2]
  "Get overlap rectangle between two colliding rectangles."
  (c-get-collision-rec r1 r2))

# ── Public API: Color manipulation ───────────────────────────────────

(defn fade [col alpha]
  "Color with applied alpha (0.0 to 1.0)."
  (c-fade col (float alpha)))

(defn color-from-hsv [hue saturation value]
  "Color from HSV values (hue 0-360, s/v 0-1)."
  (c-color-from-hsv (float hue) (float saturation) (float value)))

(defn color-alpha [col alpha]
  "Color with new alpha (0.0 to 1.0)."
  (c-color-alpha col (float alpha)))

(defn color-brightness [col factor]
  "Color with brightness correction (-1.0 to 1.0)."
  (c-color-brightness col (float factor)))

(defn color-tint [col tint]
  "Color with tint applied."
  (c-color-tint col tint))

# ── Public API: Misc ─────────────────────────────────────────────────

(defn set-config-flags [flags]   (c-set-config-flags flags))
(defn set-trace-log-level [lvl]  (c-set-trace-log-level lvl))
(defn random-value [lo hi]       (c-get-random-value lo hi))
(defn take-screenshot [path]     (c-take-screenshot path))
(defn open-url [url]             (c-open-url url))

# ── Export ───────────────────────────────────────────────────────────

{# constructors
 :color color :vec2 vec2 :vec3 vec3 :rect rect
 :camera-2d camera-2d :camera-3d camera-3d
 # accessors
 :texture-width texture-width :texture-height texture-height
 :texture-size texture-size

 # window
 :init-window init-window :close-window close-window
 :window-should-close window-should-close
 :window-ready? window-ready? :window-fullscreen? window-fullscreen?
 :window-hidden? window-hidden? :window-minimized? window-minimized?
 :window-maximized? window-maximized? :window-focused? window-focused?
 :window-resized? window-resized?
 :set-window-state set-window-state :clear-window-state clear-window-state
 :toggle-fullscreen toggle-fullscreen
 :set-window-title set-window-title
 :set-window-position set-window-position :set-window-size set-window-size
 :screen-width screen-width :screen-height screen-height
 :render-width render-width :render-height render-height

 # timing
 :set-target-fps set-target-fps :fps fps
 :frame-time frame-time :elapsed-time elapsed-time

 # drawing
 :clear clear :begin-drawing begin-drawing :end-drawing end-drawing
 :with-drawing with-drawing
 :begin-mode-2d begin-mode-2d :end-mode-2d end-mode-2d
 :with-mode-2d with-mode-2d
 :begin-mode-3d begin-mode-3d :end-mode-3d end-mode-3d
 :with-mode-3d with-mode-3d
 :begin-blend-mode begin-blend-mode :end-blend-mode end-blend-mode
 :begin-scissor-mode begin-scissor-mode :end-scissor-mode end-scissor-mode

 # shapes
 :draw-pixel draw-pixel :draw-line draw-line
 :draw-line-ex draw-line-ex :draw-line-bezier draw-line-bezier
 :draw-circle draw-circle :draw-circle-gradient draw-circle-gradient
 :draw-circle-lines draw-circle-lines :draw-ellipse draw-ellipse
 :draw-rect draw-rect :draw-rect-rec draw-rect-rec
 :draw-rect-pro draw-rect-pro
 :draw-rect-gradient-v draw-rect-gradient-v
 :draw-rect-gradient-h draw-rect-gradient-h
 :draw-rect-lines draw-rect-lines :draw-rect-lines-ex draw-rect-lines-ex
 :draw-rect-rounded draw-rect-rounded
 :draw-rect-rounded-lines draw-rect-rounded-lines
 :draw-rect-rounded-lines-ex draw-rect-rounded-lines-ex
 :draw-triangle draw-triangle :draw-triangle-lines draw-triangle-lines
 :draw-poly draw-poly :draw-poly-lines draw-poly-lines
 :draw-ring draw-ring :draw-ring-lines draw-ring-lines

 # 3d shapes
 :draw-line-3d draw-line-3d
 :draw-cube draw-cube :draw-cube-wires draw-cube-wires
 :draw-sphere draw-sphere :draw-sphere-ex draw-sphere-ex
 :draw-sphere-wires draw-sphere-wires
 :draw-cylinder draw-cylinder :draw-cylinder-wires draw-cylinder-wires
 :draw-plane draw-plane :draw-grid draw-grid

 # text
 :draw-fps draw-fps :draw-text draw-text
 :draw-text-ex draw-text-ex :draw-text-pro draw-text-pro
 :measure-text measure-text :measure-text-ex measure-text-ex
 :default-font default-font :load-font load-font
 :load-font-ex load-font-ex :unload-font unload-font
 :set-text-line-spacing set-text-line-spacing

 # textures
 :load-texture load-texture :unload-texture unload-texture
 :draw-texture draw-texture :draw-texture-v draw-texture-v
 :draw-texture-ex draw-texture-ex :draw-texture-rec draw-texture-rec
 :draw-texture-pro draw-texture-pro

 # images
 :load-image load-image :unload-image unload-image
 :export-image export-image :gen-image-color gen-image-color
 :load-texture-from-image load-texture-from-image

 # input — keyboard
 :key-pressed? key-pressed? :key-down? key-down?
 :key-released? key-released? :key-up? key-up?
 :key-pressed key-pressed :char-pressed char-pressed
 :set-exit-key set-exit-key

 # input — mouse
 :mouse-button-pressed? mouse-button-pressed?
 :mouse-button-down? mouse-button-down?
 :mouse-button-released? mouse-button-released?
 :mouse-button-up? mouse-button-up?
 :mouse-x mouse-x :mouse-y mouse-y
 :mouse-position mouse-position :mouse-delta mouse-delta
 :set-mouse-position set-mouse-position
 :mouse-wheel mouse-wheel :set-mouse-cursor set-mouse-cursor
 :show-cursor show-cursor :hide-cursor hide-cursor
 :cursor-hidden? cursor-hidden?

 # input — gamepad
 :gamepad-available? gamepad-available?
 :gamepad-name gamepad-name
 :gamepad-button-pressed? gamepad-button-pressed?
 :gamepad-button-down? gamepad-button-down?
 :gamepad-axis gamepad-axis

 # input — touch
 :touch-x touch-x :touch-y touch-y
 :touch-position touch-position
 :touch-point-count touch-point-count

 # audio
 :init-audio init-audio :close-audio close-audio
 :audio-ready? audio-ready?
 :set-master-volume set-master-volume :master-volume master-volume
 :load-sound load-sound :unload-sound unload-sound
 :play-sound play-sound :stop-sound stop-sound
 :pause-sound pause-sound :resume-sound resume-sound
 :sound-playing? sound-playing?
 :set-sound-volume set-sound-volume
 :set-sound-pitch set-sound-pitch
 :set-sound-pan set-sound-pan

 # collision
 :collision-recs? collision-recs?
 :collision-circles? collision-circles?
 :collision-circle-rec? collision-circle-rec?
 :collision-point-rec? collision-point-rec?
 :collision-point-circle? collision-point-circle?
 :collision-rec collision-rec

 # color manipulation
 :fade fade :color-from-hsv color-from-hsv
 :color-alpha color-alpha :color-brightness color-brightness
 :color-tint color-tint

 # misc
 :set-config-flags set-config-flags
 :set-trace-log-level set-trace-log-level
 :random-value random-value
 :take-screenshot take-screenshot :open-url open-url

 # color constants
 :LIGHTGRAY LIGHTGRAY :GRAY GRAY :DARKGRAY DARKGRAY
 :YELLOW YELLOW :GOLD GOLD :ORANGE ORANGE
 :PINK PINK :RED RED :MAROON MAROON
 :GREEN GREEN :LIME LIME :DARKGREEN DARKGREEN
 :SKYBLUE SKYBLUE :BLUE BLUE :DARKBLUE DARKBLUE
 :PURPLE PURPLE :VIOLET VIOLET :MAGENTA MAGENTA
 :BROWN BROWN :BLACK BLACK :WHITE WHITE
 :BLANK BLANK :RAYWHITE RAYWHITE

 # key constants
 :KEY_NULL KEY_NULL :KEY_SPACE KEY_SPACE
 :KEY_ESCAPE KEY_ESCAPE :KEY_ENTER KEY_ENTER
 :KEY_TAB KEY_TAB :KEY_BACKSPACE KEY_BACKSPACE
 :KEY_INSERT KEY_INSERT :KEY_DELETE KEY_DELETE
 :KEY_RIGHT KEY_RIGHT :KEY_LEFT KEY_LEFT
 :KEY_DOWN KEY_DOWN :KEY_UP KEY_UP
 :KEY_PAGE_UP KEY_PAGE_UP :KEY_PAGE_DOWN KEY_PAGE_DOWN
 :KEY_HOME KEY_HOME :KEY_END KEY_END
 :KEY_F1 KEY_F1 :KEY_F2 KEY_F2 :KEY_F3 KEY_F3
 :KEY_F4 KEY_F4 :KEY_F5 KEY_F5 :KEY_F6 KEY_F6
 :KEY_F7 KEY_F7 :KEY_F8 KEY_F8 :KEY_F9 KEY_F9
 :KEY_F10 KEY_F10 :KEY_F11 KEY_F11 :KEY_F12 KEY_F12
 :KEY_A KEY_A :KEY_B KEY_B :KEY_C KEY_C :KEY_D KEY_D
 :KEY_E KEY_E :KEY_F KEY_F :KEY_G KEY_G :KEY_H KEY_H
 :KEY_I KEY_I :KEY_J KEY_J :KEY_K KEY_K :KEY_L KEY_L
 :KEY_M KEY_M :KEY_N KEY_N :KEY_O KEY_O :KEY_P KEY_P
 :KEY_Q KEY_Q :KEY_R KEY_R :KEY_S KEY_S :KEY_T KEY_T
 :KEY_U KEY_U :KEY_V KEY_V :KEY_W KEY_W :KEY_X KEY_X
 :KEY_Y KEY_Y :KEY_Z KEY_Z
 :KEY_ZERO KEY_ZERO :KEY_ONE KEY_ONE :KEY_TWO KEY_TWO
 :KEY_THREE KEY_THREE :KEY_FOUR KEY_FOUR :KEY_FIVE KEY_FIVE
 :KEY_SIX KEY_SIX :KEY_SEVEN KEY_SEVEN :KEY_EIGHT KEY_EIGHT
 :KEY_NINE KEY_NINE
 :KEY_LEFT_SHIFT KEY_LEFT_SHIFT :KEY_LEFT_CONTROL KEY_LEFT_CONTROL
 :KEY_LEFT_ALT KEY_LEFT_ALT
 :KEY_RIGHT_SHIFT KEY_RIGHT_SHIFT :KEY_RIGHT_CONTROL KEY_RIGHT_CONTROL
 :KEY_RIGHT_ALT KEY_RIGHT_ALT

 # mouse button constants
 :MOUSE_LEFT MOUSE_LEFT :MOUSE_RIGHT MOUSE_RIGHT
 :MOUSE_MIDDLE MOUSE_MIDDLE :MOUSE_SIDE MOUSE_SIDE
 :MOUSE_EXTRA MOUSE_EXTRA

 # gamepad constants
 :GAMEPAD_BUTTON_LEFT_FACE_UP GAMEPAD_BUTTON_LEFT_FACE_UP
 :GAMEPAD_BUTTON_LEFT_FACE_RIGHT GAMEPAD_BUTTON_LEFT_FACE_RIGHT
 :GAMEPAD_BUTTON_LEFT_FACE_DOWN GAMEPAD_BUTTON_LEFT_FACE_DOWN
 :GAMEPAD_BUTTON_LEFT_FACE_LEFT GAMEPAD_BUTTON_LEFT_FACE_LEFT
 :GAMEPAD_BUTTON_RIGHT_FACE_UP GAMEPAD_BUTTON_RIGHT_FACE_UP
 :GAMEPAD_BUTTON_RIGHT_FACE_RIGHT GAMEPAD_BUTTON_RIGHT_FACE_RIGHT
 :GAMEPAD_BUTTON_RIGHT_FACE_DOWN GAMEPAD_BUTTON_RIGHT_FACE_DOWN
 :GAMEPAD_BUTTON_RIGHT_FACE_LEFT GAMEPAD_BUTTON_RIGHT_FACE_LEFT
 :GAMEPAD_BUTTON_LEFT_TRIGGER_1 GAMEPAD_BUTTON_LEFT_TRIGGER_1
 :GAMEPAD_BUTTON_LEFT_TRIGGER_2 GAMEPAD_BUTTON_LEFT_TRIGGER_2
 :GAMEPAD_BUTTON_RIGHT_TRIGGER_1 GAMEPAD_BUTTON_RIGHT_TRIGGER_1
 :GAMEPAD_BUTTON_RIGHT_TRIGGER_2 GAMEPAD_BUTTON_RIGHT_TRIGGER_2
 :GAMEPAD_BUTTON_MIDDLE_LEFT GAMEPAD_BUTTON_MIDDLE_LEFT
 :GAMEPAD_BUTTON_MIDDLE GAMEPAD_BUTTON_MIDDLE
 :GAMEPAD_BUTTON_MIDDLE_RIGHT GAMEPAD_BUTTON_MIDDLE_RIGHT
 :GAMEPAD_AXIS_LEFT_X GAMEPAD_AXIS_LEFT_X
 :GAMEPAD_AXIS_LEFT_Y GAMEPAD_AXIS_LEFT_Y
 :GAMEPAD_AXIS_RIGHT_X GAMEPAD_AXIS_RIGHT_X
 :GAMEPAD_AXIS_RIGHT_Y GAMEPAD_AXIS_RIGHT_Y
 :GAMEPAD_AXIS_LEFT_TRIGGER GAMEPAD_AXIS_LEFT_TRIGGER
 :GAMEPAD_AXIS_RIGHT_TRIGGER GAMEPAD_AXIS_RIGHT_TRIGGER

 # config flags
 :FLAG_VSYNC FLAG_VSYNC :FLAG_FULLSCREEN FLAG_FULLSCREEN
 :FLAG_RESIZABLE FLAG_RESIZABLE :FLAG_UNDECORATED FLAG_UNDECORATED
 :FLAG_HIDDEN FLAG_HIDDEN :FLAG_MINIMIZED FLAG_MINIMIZED
 :FLAG_MAXIMIZED FLAG_MAXIMIZED :FLAG_TOPMOST FLAG_TOPMOST
 :FLAG_ALWAYS_RUN FLAG_ALWAYS_RUN :FLAG_TRANSPARENT FLAG_TRANSPARENT
 :FLAG_HIGHDPI FLAG_HIGHDPI :FLAG_MSAA_4X FLAG_MSAA_4X

 # camera projection
 :CAMERA_PERSPECTIVE CAMERA_PERSPECTIVE
 :CAMERA_ORTHOGRAPHIC CAMERA_ORTHOGRAPHIC

 # blend modes
 :BLEND_ALPHA BLEND_ALPHA :BLEND_ADDITIVE BLEND_ADDITIVE
 :BLEND_MULTIPLIED BLEND_MULTIPLIED
 :BLEND_ADD_COLORS BLEND_ADD_COLORS
 :BLEND_SUBTRACT_COLORS BLEND_SUBTRACT_COLORS
 :BLEND_ALPHA_PREMULTIPLY BLEND_ALPHA_PREMULTIPLY}

) # end (fn [])
