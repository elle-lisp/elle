## std/sdl — SDL3 bindings for Elle via FFI
##
## Pure FFI bindings to libSDL3. No Rust plugin needed.
##
## Dependencies:
##   - libSDL3.so installed on the system
##   - ffi primitives (ffi/native, ffi/defbind, ffi/malloc, etc.)
##
## Usage:
##   (def sdl ((import "std/sdl")))
##   (sdl:init)
##   (def win (sdl:create-window "Hello" 640 480))
##   (def ren (sdl:create-renderer win))
##   (defer (sdl:destroy-renderer ren)
##   (defer (sdl:destroy-window win)
##   (defer (sdl:quit)
##     ...)))
##
## See demos/sdl/demo.lisp for a complete example.

(fn []

# ── Load libSDL3 ──────────────────────────────────────────────────────

(def libsdl (ffi/native "libSDL3.so"))

# ── Constants: init flags ─────────────────────────────────────────────

(def init-audio   0x00000010)
(def init-video   0x00000020)
(def init-joystick 0x00000200)
(def init-haptic  0x00001000)
(def init-gamepad 0x00002000)
(def init-events  0x00004000)
(def init-sensor  0x00008000)
(def init-camera  0x00010000)

# ── Constants: window flags ───────────────────────────────────────────

(def window-fullscreen  0x01)
(def window-opengl      0x02)
(def window-hidden      0x08)
(def window-borderless  0x10)
(def window-resizable   0x20)
(def window-minimized   0x40)
(def window-maximized   0x80)
(def window-high-dpi    0x2000)
(def window-always-on-top 0x10000)

# ── Constants: event types ────────────────────────────────────────────

(def event-quit             0x100)
(def event-key-down         0x300)
(def event-key-up           0x301)
(def event-text-editing     0x302)
(def event-text-input       0x303)
(def event-mouse-motion     0x400)
(def event-mouse-button-down 0x401)
(def event-mouse-button-up  0x402)
(def event-mouse-wheel      0x403)
(def event-window-first     0x202)
(def event-window-last      0x21a)

# ── Constants: scancodes ──────────────────────────────────────────────

(def scancode-return    40)
(def scancode-escape    41)
(def scancode-backspace 42)
(def scancode-tab       43)
(def scancode-space     44)
(def scancode-f1  58) (def scancode-f2  59) (def scancode-f3  60)
(def scancode-f4  61) (def scancode-f5  62) (def scancode-f6  63)
(def scancode-f7  64) (def scancode-f8  65) (def scancode-f9  66)
(def scancode-f10 67) (def scancode-f11 68) (def scancode-f12 69)
(def scancode-insert   73)
(def scancode-home     74)
(def scancode-pageup   75)
(def scancode-delete   76)
(def scancode-end      77)
(def scancode-pagedown 78)
(def scancode-right    79)
(def scancode-left     80)
(def scancode-down     81)
(def scancode-up       82)

# ── Constants: key modifiers ──────────────────────────────────────────

(def kmod-none   0x0000)
(def kmod-lshift 0x0001)
(def kmod-rshift 0x0002)
(def kmod-lctrl  0x0040)
(def kmod-rctrl  0x0080)
(def kmod-lalt   0x0100)
(def kmod-ralt   0x0200)
(def kmod-lgui   0x0400)
(def kmod-rgui   0x0800)
(def kmod-num    0x1000)
(def kmod-caps   0x2000)
(def kmod-scroll 0x8000)
(def kmod-shift  0x0003)
(def kmod-ctrl   0x00c0)
(def kmod-alt    0x0300)

# ── Constants: blend modes ────────────────────────────────────────────

(def blend-none  0x00000000)
(def blend-blend 0x00000001)
(def blend-add   0x00000002)
(def blend-mod   0x00000004)
(def blend-mul   0x00000008)

# ── Constants: window event subtypes ──────────────────────────────────

(def window-event-names
  {0x202 :shown        0x203 :hidden         0x204 :exposed
   0x205 :moved        0x206 :resized        0x207 :pixel-size-changed
   0x209 :minimized    0x20a :maximized       0x20b :restored
   0x20c :mouse-enter  0x20d :mouse-leave
   0x20e :focus-gained 0x20f :focus-lost
   0x210 :close-requested
   0x213 :display-changed 0x214 :display-scale-changed
   0x216 :occluded
   0x217 :enter-fullscreen 0x218 :leave-fullscreen
   0x219 :destroyed})

# ── Raw C bindings ────────────────────────────────────────────────────

# Init / quit
(ffi/defbind sdl-init            libsdl "SDL_Init"               :bool [:u32])
(ffi/defbind sdl-quit            libsdl "SDL_Quit"               :void [])
(ffi/defbind sdl-get-error       libsdl "SDL_GetError"           :ptr  [])

# Window
(ffi/defbind sdl-create-window   libsdl "SDL_CreateWindow"       :ptr  [:string :int :int :u64])
(ffi/defbind sdl-destroy-window  libsdl "SDL_DestroyWindow"      :void [:ptr])
(ffi/defbind sdl-set-window-title libsdl "SDL_SetWindowTitle"    :bool [:ptr :string])
(ffi/defbind sdl-get-window-size libsdl "SDL_GetWindowSize"      :bool [:ptr :ptr :ptr])
(ffi/defbind sdl-set-window-size libsdl "SDL_SetWindowSize"      :bool [:ptr :int :int])
(ffi/defbind sdl-get-window-pos  libsdl "SDL_GetWindowPosition"  :bool [:ptr :ptr :ptr])
(ffi/defbind sdl-set-window-pos  libsdl "SDL_SetWindowPosition"  :bool [:ptr :int :int])
(ffi/defbind sdl-set-fullscreen  libsdl "SDL_SetWindowFullscreen" :bool [:ptr :bool])

# Renderer
(ffi/defbind sdl-create-renderer  libsdl "SDL_CreateRenderer"     :ptr  [:ptr :string])
(ffi/defbind sdl-destroy-renderer libsdl "SDL_DestroyRenderer"    :void [:ptr])
(ffi/defbind sdl-set-draw-color   libsdl "SDL_SetRenderDrawColor" :bool [:ptr :u8 :u8 :u8 :u8])
(ffi/defbind sdl-render-clear     libsdl "SDL_RenderClear"        :bool [:ptr])
(ffi/defbind sdl-render-present   libsdl "SDL_RenderPresent"      :bool [:ptr])
(ffi/defbind sdl-render-point     libsdl "SDL_RenderPoint"        :bool [:ptr :float :float])
(ffi/defbind sdl-render-line      libsdl "SDL_RenderLine"         :bool [:ptr :float :float :float :float])
(ffi/defbind sdl-render-rect      libsdl "SDL_RenderRect"         :bool [:ptr :ptr])
(ffi/defbind sdl-render-fill-rect libsdl "SDL_RenderFillRect"     :bool [:ptr :ptr])
(ffi/defbind sdl-set-blend-mode   libsdl "SDL_SetRenderDrawBlendMode" :bool [:ptr :u32])
(ffi/defbind sdl-set-scale        libsdl "SDL_SetRenderScale"     :bool [:ptr :float :float])
(ffi/defbind sdl-set-viewport     libsdl "SDL_SetRenderViewport"  :bool [:ptr :ptr])
(ffi/defbind sdl-set-vsync        libsdl "SDL_SetRenderVSync"     :bool [:ptr :int])
(ffi/defbind sdl-debug-text       libsdl "SDL_RenderDebugText"    :bool [:ptr :float :float :string])

# Events
(ffi/defbind sdl-poll-event       libsdl "SDL_PollEvent"          :bool [:ptr])
(ffi/defbind sdl-wait-event       libsdl "SDL_WaitEvent"          :bool [:ptr])
(ffi/defbind sdl-wait-event-timeout libsdl "SDL_WaitEventTimeout" :bool [:ptr :i32])

# Timing
(ffi/defbind sdl-get-ticks        libsdl "SDL_GetTicks"           :u64  [])
(ffi/defbind sdl-delay            libsdl "SDL_Delay"              :void [:u32])
(ffi/defbind sdl-delay-ns         libsdl "SDL_DelayNS"            :void [:u64])
(ffi/defbind sdl-perf-counter     libsdl "SDL_GetPerformanceCounter"   :u64 [])
(ffi/defbind sdl-perf-frequency   libsdl "SDL_GetPerformanceFrequency" :u64 [])

# ── FRect type for draw calls ─────────────────────────────────────────

(def frect-type (ffi/struct @[:float :float :float :float]))

# Pre-allocated buffers — reused across calls to avoid per-frame allocation
(def rect-buf  (ffi/malloc (ffi/size frect-type)))
(def event-buf (ffi/malloc 128))

# ── Internal helpers ──────────────────────────────────────────────────

(defn null? [ptr]
  (= (ptr/to-int ptr) 0))

(defn sdl-error [name]
  "Raise an SDL error with context from SDL_GetError."
  (error {:error :sdl-error
          :fn name
          :message (concat name ": " (ffi/string (sdl-get-error)))}))

(defn check-bool [ok name]
  "Check an SDL bool return; error if false."
  (when (not ok) (sdl-error name))
  true)

(defn check-ptr [ptr name]
  "Check an SDL pointer return; error if NULL."
  (when (null? ptr) (sdl-error name))
  ptr)

# ── Event marshalling ─────────────────────────────────────────────────
#
# SDL_Event is a 128-byte union. We read fields at byte offsets.
# Offsets verified against SDL3 headers via offsetof().
#
# Common header:
#   +0  u32  type
#   +8  u64  timestamp (ns)
#
# Keyboard (type 0x300/0x301):
#   +16 u32  window-id    +20 u32  which
#   +24 u32  scancode     +28 u32  key
#   +32 u16  mod          +36 u8   down     +37 u8  repeat
#
# Mouse motion (type 0x400):
#   +16 u32  window-id    +20 u32  which
#   +24 u32  state        +28 float x       +32 float y
#   +36 float xrel        +40 float yrel
#
# Mouse button (type 0x401/0x402):
#   +16 u32  window-id    +20 u32  which
#   +24 u8   button       +25 u8   down     +26 u8  clicks
#   +28 float x           +32 float y
#
# Mouse wheel (type 0x403):
#   +16 u32  window-id    +20 u32  which
#   +24 float x           +28 float y
#   +32 u32  direction    +36 float mouse-x  +40 float mouse-y
#
# Window (type 0x202-0x21a):
#   +16 u32  window-id    +20 i32  data1    +24 i32  data2
#
# Text input (type 0x303):
#   +16 u32  window-id    +24 ptr  text
#
# Quit (type 0x100):
#   (header only)

(defn read-u8  [buf off] (ffi/read (ptr/add buf off) :u8))
(defn read-u16 [buf off] (ffi/read (ptr/add buf off) :u16))
(defn read-u32 [buf off] (ffi/read (ptr/add buf off) :u32))
(defn read-i32 [buf off] (ffi/read (ptr/add buf off) :i32))
(defn read-u64 [buf off] (ffi/read (ptr/add buf off) :u64))
(defn read-f32 [buf off] (ffi/read (ptr/add buf off) :float))
(defn read-ptr [buf off] (ffi/read (ptr/add buf off) :ptr))

(defn marshal-keyboard [buf etype]
  {:type      (if (= etype event-key-down) :key-down :key-up)
   :timestamp (read-u64 buf 8)
   :window-id (read-u32 buf 16)
   :scancode  (read-u32 buf 24)
   :key       (read-u32 buf 28)
   :mod       (read-u16 buf 32)
   :down      (not (= (read-u8 buf 36) 0))
   :repeat    (not (= (read-u8 buf 37) 0))})

(defn marshal-mouse-motion [buf]
  {:type      :mouse-motion
   :timestamp (read-u64 buf 8)
   :window-id (read-u32 buf 16)
   :state     (read-u32 buf 24)
   :x         (read-f32 buf 28)
   :y         (read-f32 buf 32)
   :xrel      (read-f32 buf 36)
   :yrel      (read-f32 buf 40)})

(defn marshal-mouse-button [buf etype]
  {:type      (if (= etype event-mouse-button-down) :mouse-down :mouse-up)
   :timestamp (read-u64 buf 8)
   :window-id (read-u32 buf 16)
   :button    (read-u8  buf 24)
   :down      (not (= (read-u8 buf 25) 0))
   :clicks    (read-u8  buf 26)
   :x         (read-f32 buf 28)
   :y         (read-f32 buf 32)})

(defn marshal-mouse-wheel [buf]
  {:type      :mouse-wheel
   :timestamp (read-u64 buf 8)
   :window-id (read-u32 buf 16)
   :x         (read-f32 buf 24)
   :y         (read-f32 buf 28)
   :direction (read-u32 buf 32)
   :mouse-x   (read-f32 buf 36)
   :mouse-y   (read-f32 buf 40)})

(defn marshal-window [buf etype]
  (let ([subtype (get window-event-names etype)])
    {:type      :window
     :subtype   (if (nil? subtype) :unknown subtype)
     :timestamp (read-u64 buf 8)
     :window-id (read-u32 buf 16)
     :data1     (read-i32 buf 20)
     :data2     (read-i32 buf 24)}))

(defn marshal-text-input [buf]
  (let ([text-ptr (read-ptr buf 24)])
    {:type      :text-input
     :timestamp (read-u64 buf 8)
     :window-id (read-u32 buf 16)
     :text      (if (null? text-ptr) "" (ffi/string text-ptr))}))

(defn marshal-event [buf]
  "Read one event from a 128-byte buffer. Returns a struct or nil."
  (let ([etype (read-u32 buf 0)])
    (cond
      ((= etype event-quit)
        {:type :quit :timestamp (read-u64 buf 8)})
      ((or (= etype event-key-down) (= etype event-key-up))
        (marshal-keyboard buf etype))
      ((= etype event-mouse-motion)
        (marshal-mouse-motion buf))
      ((or (= etype event-mouse-button-down) (= etype event-mouse-button-up))
        (marshal-mouse-button buf etype))
      ((= etype event-mouse-wheel)
        (marshal-mouse-wheel buf))
      ((and (>= etype event-window-first) (<= etype event-window-last))
        (marshal-window buf etype))
      ((= etype event-text-input)
        (marshal-text-input buf))
      (true
        {:type :unknown :raw-type etype :timestamp (read-u64 buf 8)}))))

# ── Public API ─────────────────────────────────────────────────────────

(defn sdl/init [&named audio video joystick haptic gamepad events sensor camera]
  "Initialize SDL subsystems. Pass keyword flags, e.g. (sdl/init :video true).
   With no arguments, initializes video (which implies events)."
  (let ([flags (+ (if audio    init-audio    0)
                  (if video    init-video    0)
                  (if joystick init-joystick 0)
                  (if haptic   init-haptic   0)
                  (if gamepad  init-gamepad  0)
                  (if events   init-events   0)
                  (if sensor   init-sensor   0)
                  (if camera   init-camera   0))])
    (let ([f (if (= flags 0) init-video flags)])
      (check-bool (sdl-init f) "sdl/init"))))

(defn sdl/quit []
  "Shut down all SDL subsystems."
  (sdl-quit))

(defn sdl/error-string []
  "Return the current SDL error string."
  (ffi/string (sdl-get-error)))

(defn sdl/create-window [title width height &named flags]
  "Create a window. Returns a window pointer.
   :flags is a u64 window flags bitmask (default 0)."
  (check-ptr (sdl-create-window title width height (if flags flags 0))
             "sdl/create-window"))

(defn sdl/destroy-window [win]
  "Destroy a window."
  (sdl-destroy-window win))

(defn sdl/set-title [win title]
  "Set window title."
  (check-bool (sdl-set-window-title win title) "sdl/set-title"))

(defn sdl/window-size [win]
  "Get window size as {:width w :height h}."
  (ffi/with-stack [[wp :int 0] [hp :int 0]]
    (check-bool (sdl-get-window-size win wp hp) "sdl/window-size")
    {:width (ffi/read wp :int) :height (ffi/read hp :int)}))

(defn sdl/set-window-size [win w h]
  "Set window size."
  (check-bool (sdl-set-window-size win w h) "sdl/set-window-size"))

(defn sdl/window-position [win]
  "Get window position as {:x x :y y}."
  (ffi/with-stack [[xp :int 0] [yp :int 0]]
    (check-bool (sdl-get-window-pos win xp yp) "sdl/window-position")
    {:x (ffi/read xp :int) :y (ffi/read yp :int)}))

(defn sdl/set-window-position [win x y]
  "Set window position."
  (check-bool (sdl-set-window-pos win x y) "sdl/set-window-position"))

(defn sdl/set-fullscreen [win fullscreen]
  "Set fullscreen mode."
  (check-bool (sdl-set-fullscreen win fullscreen) "sdl/set-fullscreen"))

(defn sdl/create-renderer [win &named name]
  "Create a renderer for a window. Returns a renderer pointer.
   :name selects a specific backend (default nil = auto)."
  (check-ptr (sdl-create-renderer win name) "sdl/create-renderer"))

(defn sdl/destroy-renderer [ren]
  "Destroy a renderer."
  (sdl-destroy-renderer ren))

(defn sdl/set-color [ren r g b &named a]
  "Set the draw color. Alpha defaults to 255."
  (check-bool (sdl-set-draw-color ren r g b (if a a 255))
              "sdl/set-color"))

(defn sdl/clear [ren]
  "Clear the renderer with the current draw color."
  (check-bool (sdl-render-clear ren) "sdl/clear"))

(defn sdl/present [ren]
  "Present the rendered frame."
  (check-bool (sdl-render-present ren) "sdl/present"))

(defn sdl/draw-point [ren x y]
  "Draw a point."
  (check-bool (sdl-render-point ren x y) "sdl/draw-point"))

(defn sdl/draw-line [ren x1 y1 x2 y2]
  "Draw a line."
  (check-bool (sdl-render-line ren x1 y1 x2 y2) "sdl/draw-line"))

(defn write-frect [x y w h]
  (ffi/write rect-buf :float x)
  (ffi/write (ptr/add rect-buf 4) :float y)
  (ffi/write (ptr/add rect-buf 8) :float w)
  (ffi/write (ptr/add rect-buf 12) :float h))

(defn sdl/draw-rect [ren x y w h]
  "Draw a rectangle outline."
  (write-frect x y w h)
  (check-bool (sdl-render-rect ren rect-buf) "sdl/draw-rect"))

(defn sdl/fill-rect [ren x y w h]
  "Draw a filled rectangle."
  (write-frect x y w h)
  (check-bool (sdl-render-fill-rect ren rect-buf) "sdl/fill-rect"))

(defn sdl/debug-text [ren x y text]
  "Draw debug text (built-in 8x8 font, no TTF needed)."
  (check-bool (sdl-debug-text ren x y text) "sdl/debug-text"))

(defn sdl/set-blend-mode [ren mode]
  "Set blend mode. Use blend-* constants."
  (check-bool (sdl-set-blend-mode ren mode) "sdl/set-blend-mode"))

(defn sdl/set-scale [ren sx sy]
  "Set render scale."
  (check-bool (sdl-set-scale ren sx sy) "sdl/set-scale"))

(defn sdl/set-vsync [ren vsync]
  "Set vsync. 0=off, 1=on, -1=adaptive."
  (check-bool (sdl-set-vsync ren vsync) "sdl/set-vsync"))

(defn sdl/poll-events []
  "Poll all pending events. Returns an array of event structs.
   Each event has at minimum :type and :timestamp."
  (let ([events @[]])
    (while (sdl-poll-event event-buf)
      (push events (marshal-event event-buf)))
    events))

(defn sdl/wait-event []
  "Wait for an event (blocks). Returns a single event struct."
  (check-bool (sdl-wait-event event-buf) "sdl/wait-event")
  (marshal-event event-buf))

(defn sdl/wait-event-timeout [timeout-ms]
  "Wait for an event with timeout (ms). Returns event struct or nil."
  (if (sdl-wait-event-timeout event-buf timeout-ms)
    (marshal-event event-buf)
    nil))

(defn sdl/ticks []
  "Get milliseconds since SDL init."
  (sdl-get-ticks))

(defn sdl/delay [ms]
  "Delay for ms milliseconds."
  (sdl-delay ms))

(defn sdl/delay-ns [ns]
  "Delay for ns nanoseconds."
  (sdl-delay-ns ns))

(defn sdl/perf-counter []
  "Get high-resolution performance counter."
  (sdl-perf-counter))

(defn sdl/perf-frequency []
  "Get performance counter frequency (counts per second)."
  (sdl-perf-frequency))

# ── Constructors ──────────────────────────────────────────────────────

(defn sdl/rgb [r g b]
  "Color struct with alpha 255."
  {:r r :g g :b b :a 255})

(defn sdl/rgba [r g b a]
  "Color struct with explicit alpha."
  {:r r :g g :b b :a a})

(defn sdl/rect [x y w h]
  "Rectangle struct."
  {:x x :y y :w w :h h})

(defn sdl/point [x y]
  "Point struct."
  {:x x :y y})

# ── Export ─────────────────────────────────────────────────────────────

{# Lifecycle
 :init              sdl/init
 :quit              sdl/quit
 :error-string      sdl/error-string

 # Window
 :create-window     sdl/create-window
 :destroy-window    sdl/destroy-window
 :set-title         sdl/set-title
 :window-size       sdl/window-size
 :set-window-size   sdl/set-window-size
 :window-position   sdl/window-position
 :set-window-position sdl/set-window-position
 :set-fullscreen    sdl/set-fullscreen

 # Renderer
 :create-renderer   sdl/create-renderer
 :destroy-renderer  sdl/destroy-renderer
 :set-color         sdl/set-color
 :clear             sdl/clear
 :present           sdl/present
 :draw-point        sdl/draw-point
 :draw-line         sdl/draw-line
 :draw-rect         sdl/draw-rect
 :fill-rect         sdl/fill-rect
 :debug-text        sdl/debug-text
 :set-blend-mode    sdl/set-blend-mode
 :set-scale         sdl/set-scale
 :set-vsync         sdl/set-vsync

 # Events
 :poll-events       sdl/poll-events
 :wait-event        sdl/wait-event
 :wait-event-timeout sdl/wait-event-timeout

 # Timing
 :ticks             sdl/ticks
 :delay             sdl/delay
 :delay-ns          sdl/delay-ns
 :perf-counter      sdl/perf-counter
 :perf-frequency    sdl/perf-frequency

 # Constructors
 :rgb               sdl/rgb
 :rgba              sdl/rgba
 :rect              sdl/rect
 :point             sdl/point

 # Constants (useful for users)
 :init-audio        init-audio
 :init-video        init-video
 :init-gamepad      init-gamepad
 :init-events       init-events
 :window-fullscreen window-fullscreen
 :window-resizable  window-resizable
 :window-borderless window-borderless
 :window-hidden     window-hidden
 :window-maximized  window-maximized
 :window-high-dpi   window-high-dpi
 :window-always-on-top window-always-on-top
 :blend-none        blend-none
 :blend-blend       blend-blend
 :blend-add         blend-add
 :blend-mod         blend-mod
 :blend-mul         blend-mul
 :scancode-escape   scancode-escape
 :scancode-return   scancode-return
 :scancode-space    scancode-space
 :scancode-tab      scancode-tab
 :scancode-backspace scancode-backspace
 :scancode-up       scancode-up
 :scancode-down     scancode-down
 :scancode-left     scancode-left
 :scancode-right    scancode-right
 :scancode-f1  scancode-f1  :scancode-f2  scancode-f2
 :scancode-f3  scancode-f3  :scancode-f4  scancode-f4
 :scancode-f5  scancode-f5  :scancode-f6  scancode-f6
 :scancode-f7  scancode-f7  :scancode-f8  scancode-f8
 :scancode-f9  scancode-f9  :scancode-f10 scancode-f10
 :scancode-f11 scancode-f11 :scancode-f12 scancode-f12
 :kmod-shift   kmod-shift
 :kmod-ctrl    kmod-ctrl
 :kmod-alt     kmod-alt}

) # end (fn [])
