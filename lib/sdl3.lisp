(elle/epoch 7)
## std/sdl3 — SDL3 bindings for Elle via FFI
##
## Pure FFI bindings to libSDL3. No Rust plugin needed.
##
## Dependencies:
##   - libSDL3.so installed on the system
##   - ffi primitives (ffi/native, ffi/defbind, ffi/malloc, etc.)
##
## Usage:
##   (def sdl ((import "std/sdl3")))
##   (sdl:init)
##   (def win (sdl:create-window "Hello" 640 480))
##   (def ren (sdl:create-renderer win))
##   (defer (sdl:destroy-renderer ren)
##   (defer (sdl:destroy-window win)
##   (defer (sdl:quit)
##     ...)))
##
## See demos/conway/conway.lisp for a complete example.

(fn []

# ── Load libSDL3 ──────────────────────────────────────────────────────

(def libsdl (ffi/native "libSDL3.so"))
(def libimg (ffi/native "libSDL3_image.so"))
(def libttf (ffi/native "libSDL3_ttf.so"))

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

# ── Constants: texture access ─────────────────────────────────────────

(def texture-static    0)
(def texture-streaming 1)
(def texture-target    2)

# ── Constants: pixel formats ──────────────────────────────────────────

(def pixfmt-unknown    0)
(def pixfmt-rgba8888   0x16462004)
(def pixfmt-argb8888   0x16362004)
(def pixfmt-rgba32     pixfmt-rgba8888)   # little-endian alias
(def pixfmt-argb32     pixfmt-argb8888)

# ── Constants: scale modes ────────────────────────────────────────────

(def scalemode-nearest 0)
(def scalemode-linear  1)

# ── Constants: flip modes ─────────────────────────────────────────────

(def flip-none       0)
(def flip-horizontal 1)
(def flip-vertical   2)

# ── Constants: font styles ────────────────────────────────────────────

(def font-normal        0x00)
(def font-bold          0x01)
(def font-italic        0x02)
(def font-underline     0x04)
(def font-strikethrough 0x08)

# ── Constants: audio ──────────────────────────────────────────────────

(def audio-u8       0x0008)
(def audio-s16      0x8010)
(def audio-s32      0x8020)
(def audio-f32      0x8120)
(def audio-device-default-playback  0xFFFFFFFF)
(def audio-device-default-recording 0xFFFFFFFE)

# ── Constants: message box ────────────────────────────────────────────

(def msgbox-error       0x00000010)
(def msgbox-warning     0x00000020)
(def msgbox-information 0x00000040)

# ── Constants: flash operation ────────────────────────────────────────

(def flash-cancel         0)
(def flash-briefly        1)
(def flash-until-focused  2)

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

# ── Compound types for draw calls ─────────────────────────────────────

(def frect-type (ffi/struct @[:float :float :float :float]))
(def irect-type (ffi/struct @[:int :int :int :int]))
(def fpoint-type (ffi/struct @[:float :float]))
(def color-type (ffi/struct @[:u8 :u8 :u8 :u8]))
(def fcolor-type (ffi/struct @[:float :float :float :float]))
# SDL_Vertex = SDL_FPoint position + SDL_FColor color + SDL_FPoint tex_coord
(def vertex-type (ffi/struct @[:float :float :float :float :float :float :float :float]))
(def vertex-size (ffi/size vertex-type))
# SDL_AudioSpec = {SDL_AudioFormat(u32), channels(int), freq(int)} = 12 bytes
(def audio-spec-type (ffi/struct @[:u32 :int :int]))

# Pre-allocated buffers — reused across calls to avoid per-frame allocation
(def rect-buf   (ffi/malloc (ffi/size frect-type)))
(def rect-buf-2 (ffi/malloc (ffi/size frect-type)))
(def point-buf  (ffi/malloc (ffi/size fpoint-type)))
(def event-buf  (ffi/malloc 128))

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

# Textures
(ffi/defbind sdl-create-texture   libsdl "SDL_CreateTexture"      :ptr  [:ptr :u32 :int :int :int])
(ffi/defbind sdl-destroy-texture  libsdl "SDL_DestroyTexture"     :void [:ptr])
(ffi/defbind sdl-render-texture   libsdl "SDL_RenderTexture"      :bool [:ptr :ptr :ptr :ptr])
(ffi/defbind sdl-render-texture-rotated libsdl "SDL_RenderTextureRotated" :bool [:ptr :ptr :ptr :ptr :double :ptr :int])
(ffi/defbind sdl-set-texture-blend libsdl "SDL_SetTextureBlendMode" :bool [:ptr :u32])
(ffi/defbind sdl-set-texture-alpha libsdl "SDL_SetTextureAlphaMod" :bool [:ptr :u8])
(ffi/defbind sdl-set-texture-color libsdl "SDL_SetTextureColorMod" :bool [:ptr :u8 :u8 :u8])
(ffi/defbind sdl-set-texture-scale-mode libsdl "SDL_SetTextureScaleMode" :bool [:ptr :int])
(ffi/defbind sdl-get-texture-size libsdl "SDL_GetTextureSize"     :bool [:ptr :ptr :ptr])
(ffi/defbind sdl-set-render-target libsdl "SDL_SetRenderTarget"   :bool [:ptr :ptr])
(ffi/defbind sdl-create-texture-from-surface libsdl "SDL_CreateTextureFromSurface" :ptr [:ptr :ptr])

# Texture streaming
(ffi/defbind sdl-lock-texture     libsdl "SDL_LockTexture"        :bool [:ptr :ptr :ptr :ptr])
(ffi/defbind sdl-unlock-texture   libsdl "SDL_UnlockTexture"      :void [:ptr])

# Surface
(ffi/defbind sdl-destroy-surface  libsdl "SDL_DestroySurface"     :void [:ptr])

# Geometry
(ffi/defbind sdl-render-geometry  libsdl "SDL_RenderGeometry"     :bool [:ptr :ptr :ptr :int :ptr :int])

# Images (SDL3_image)
(ffi/defbind img-load-texture     libimg "IMG_LoadTexture"        :ptr  [:ptr :string])
(ffi/defbind img-save-png         libimg "IMG_SavePNG"            :bool [:ptr :string])

# TTF (SDL3_ttf)
(ffi/defbind ttf-init             libttf "TTF_Init"               :bool [])
(ffi/defbind ttf-quit             libttf "TTF_Quit"               :void [])
(ffi/defbind ttf-open-font        libttf "TTF_OpenFont"           :ptr  [:string :float])
(ffi/defbind ttf-close-font       libttf "TTF_CloseFont"          :void [:ptr])
(ffi/defbind ttf-set-font-size    libttf "TTF_SetFontSize"        :bool [:ptr :float])
(ffi/defbind ttf-set-font-style   libttf "TTF_SetFontStyle"       :void [:ptr :int])
(ffi/defbind ttf-get-string-size  libttf "TTF_GetStringSize"      :bool [:ptr :string :size :ptr :ptr])
(ffi/defbind ttf-render-blended   libttf "TTF_RenderText_Blended"  :ptr  [:ptr :string :size color-type])

# Audio
(ffi/defbind sdl-open-audio-device-stream libsdl "SDL_OpenAudioDeviceStream" :ptr [:u32 :ptr :ptr :ptr])
(ffi/defbind sdl-resume-audio    libsdl "SDL_ResumeAudioStreamDevice"  :bool [:ptr])
(ffi/defbind sdl-pause-audio     libsdl "SDL_PauseAudioStreamDevice"   :bool [:ptr])
(ffi/defbind sdl-put-audio-data  libsdl "SDL_PutAudioStreamData"       :bool [:ptr :ptr :int])
(ffi/defbind sdl-clear-audio     libsdl "SDL_ClearAudioStream"         :bool [:ptr])
(ffi/defbind sdl-destroy-audio   libsdl "SDL_DestroyAudioStream"       :void [:ptr])
(ffi/defbind sdl-load-wav        libsdl "SDL_LoadWAV"                  :bool [:string :ptr :ptr :ptr])
(ffi/defbind sdl-get-audio-playback-devices libsdl "SDL_GetAudioPlaybackDevices" :ptr [:ptr])

# Input
(ffi/defbind sdl-get-keyboard-state libsdl "SDL_GetKeyboardState"      :ptr  [:ptr])
(ffi/defbind sdl-get-mouse-state   libsdl "SDL_GetMouseState"         :u32  [:ptr :ptr])
(ffi/defbind sdl-warp-mouse        libsdl "SDL_WarpMouseInWindow"     :void [:ptr :float :float])
(ffi/defbind sdl-show-cursor       libsdl "SDL_ShowCursor"            :bool [])
(ffi/defbind sdl-hide-cursor       libsdl "SDL_HideCursor"            :bool [])
(ffi/defbind sdl-set-relative-mouse libsdl "SDL_SetWindowRelativeMouseMode" :bool [:ptr :bool])
(ffi/defbind sdl-start-text-input  libsdl "SDL_StartTextInput"        :bool [:ptr])
(ffi/defbind sdl-stop-text-input   libsdl "SDL_StopTextInput"         :bool [:ptr])
(ffi/defbind sdl-set-keyboard-grab libsdl "SDL_SetWindowKeyboardGrab" :bool [:ptr :bool])
(ffi/defbind sdl-set-mouse-grab    libsdl "SDL_SetWindowMouseGrab"    :bool [:ptr :bool])

# Clipboard
(ffi/defbind sdl-set-clipboard     libsdl "SDL_SetClipboardText"      :bool [:string])
(ffi/defbind sdl-get-clipboard     libsdl "SDL_GetClipboardText"      :ptr  [])
(ffi/defbind sdl-has-clipboard     libsdl "SDL_HasClipboardText"      :bool [])

# Misc
(ffi/defbind sdl-open-url          libsdl "SDL_OpenURL"               :bool [:string])
(ffi/defbind sdl-show-message-box  libsdl "SDL_ShowSimpleMessageBox"  :bool [:u32 :string :string :ptr])
(ffi/defbind sdl-get-displays      libsdl "SDL_GetDisplays"           :ptr  [:ptr])
(ffi/defbind sdl-get-display-bounds libsdl "SDL_GetDisplayBounds"     :bool [:u32 :ptr])
(ffi/defbind sdl-disable-screensaver libsdl "SDL_DisableScreenSaver"  :bool [])
(ffi/defbind sdl-enable-screensaver  libsdl "SDL_EnableScreenSaver"   :bool [])
(ffi/defbind sdl-set-window-bordered libsdl "SDL_SetWindowBordered"   :bool [:ptr :bool])
(ffi/defbind sdl-set-window-opacity  libsdl "SDL_SetWindowOpacity"    :bool [:ptr :float])
(ffi/defbind sdl-flash-window      libsdl "SDL_FlashWindow"           :bool [:ptr :int])
(ffi/defbind sdl-set-window-icon   libsdl "SDL_SetWindowIcon"         :bool [:ptr :ptr])

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
  (let [subtype (get window-event-names etype)]
    {:type      :window
     :subtype   (if (nil? subtype) :unknown subtype)
     :timestamp (read-u64 buf 8)
     :window-id (read-u32 buf 16)
     :data1     (read-i32 buf 20)
     :data2     (read-i32 buf 24)}))

(defn marshal-text-input [buf]
  (let [text-ptr (read-ptr buf 24)]
    {:type      :text-input
     :timestamp (read-u64 buf 8)
     :window-id (read-u32 buf 16)
     :text      (if (null? text-ptr) "" (ffi/string text-ptr))}))

(defn marshal-event [buf]
  "Read one event from a 128-byte buffer. Returns a struct or nil."
  (let [etype (read-u32 buf 0)]
    (case etype
      event-quit
        {:type :quit :timestamp (read-u64 buf 8)}
      event-key-down       (marshal-keyboard buf etype)
      event-key-up         (marshal-keyboard buf etype)
      event-mouse-motion   (marshal-mouse-motion buf)
      event-mouse-button-down (marshal-mouse-button buf etype)
      event-mouse-button-up   (marshal-mouse-button buf etype)
      event-mouse-wheel    (marshal-mouse-wheel buf)
      event-text-input     (marshal-text-input buf)
      (if (and (>= etype event-window-first) (<= etype event-window-last))
        (marshal-window buf etype)
        {:type :unknown :raw-type etype :timestamp (read-u64 buf 8)}))))

# ── Public API ─────────────────────────────────────────────────────────

(defn sdl/init [&named audio video joystick haptic gamepad events sensor camera]
  "Initialize SDL subsystems. Pass keyword flags, e.g. (sdl/init :video true).
   With no arguments, initializes video (which implies events)."
  (let [flags (+ (if audio    init-audio    0)
                  (if video    init-video    0)
                  (if joystick init-joystick 0)
                  (if haptic   init-haptic   0)
                  (if gamepad  init-gamepad  0)
                  (if events   init-events   0)
                  (if sensor   init-sensor   0)
                  (if camera   init-camera   0))]
    (let [f (if (zero? flags) init-video flags)]
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
  (let [events @[]]
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

# ── Textures ───────────────────────────────────────────────────────────

(defn sdl/create-texture [ren format access w h]
  "Create a texture. format: pixel format constant, access: texture-static/streaming/target."
  (check-ptr (sdl-create-texture ren format access w h) "sdl/create-texture"))

(defn sdl/destroy-texture [tex]
  "Destroy a texture."
  (sdl-destroy-texture tex))

(defn sdl/texture-size [tex]
  "Get texture size as {:width w :height h}."
  (ffi/with-stack [[wp :float 0.0] [hp :float 0.0]]
    (check-bool (sdl-get-texture-size tex wp hp) "sdl/texture-size")
    {:width (ffi/read wp :float) :height (ffi/read hp :float)}))

(defn sdl/set-texture-blend-mode [tex mode]
  "Set texture blend mode. Use blend-* constants."
  (check-bool (sdl-set-texture-blend tex mode) "sdl/set-texture-blend-mode"))

(defn sdl/set-texture-alpha [tex alpha]
  "Set texture alpha mod (0-255)."
  (check-bool (sdl-set-texture-alpha tex alpha) "sdl/set-texture-alpha"))

(defn sdl/set-texture-color [tex r g b]
  "Set texture color mod (0-255 per channel)."
  (check-bool (sdl-set-texture-color tex r g b) "sdl/set-texture-color"))

(defn sdl/set-texture-scale-mode [tex mode]
  "Set texture scale mode. scalemode-nearest or scalemode-linear."
  (check-bool (sdl-set-texture-scale-mode tex mode) "sdl/set-texture-scale-mode"))

(defn sdl/render-texture [ren tex &named src dst]
  "Render a texture. src/dst are {:x :y :w :h} or nil for full."
  (when src
    (write-frect (src :x) (src :y) (src :w) (src :h)))
  (when dst
    (ffi/write rect-buf-2 frect-type @[(dst :x) (dst :y) (dst :w) (dst :h)]))
  (check-bool (sdl-render-texture ren tex
    (if src rect-buf nil)
    (if dst rect-buf-2 nil)) "sdl/render-texture"))

(defn sdl/render-texture-rotated [ren tex angle &named src dst center flip]
  "Render a texture with rotation. angle in degrees.
   center is {:x :y} or nil for texture center. flip: flip-none/horizontal/vertical."
  (when src
    (write-frect (src :x) (src :y) (src :w) (src :h)))
  (when dst
    (ffi/write rect-buf-2 frect-type @[(dst :x) (dst :y) (dst :w) (dst :h)]))
  (when center
    (ffi/write point-buf fpoint-type @[(center :x) (center :y)]))
  (check-bool (sdl-render-texture-rotated ren tex
    (if src rect-buf nil)
    (if dst rect-buf-2 nil)
    (float angle)
    (if center point-buf nil)
    (if flip flip flip-none)) "sdl/render-texture-rotated"))

(defn sdl/set-render-target [ren tex]
  "Set render target to a texture, or nil to reset to default."
  (check-bool (sdl-set-render-target ren tex) "sdl/set-render-target"))

(defn sdl/create-texture-from-surface [ren surface]
  "Create a texture from an SDL_Surface. Caller must destroy the surface separately."
  (check-ptr (sdl-create-texture-from-surface ren surface) "sdl/create-texture-from-surface"))

(defn sdl/lock-texture [tex &named rect]
  "Lock a streaming texture for pixel access. Returns {:pixels ptr :pitch int}.
   rect is {:x :y :w :h} or nil for entire texture."
  (var irect-buf nil)
  (when rect
    (assign irect-buf (ffi/malloc (ffi/size irect-type)))
    (ffi/write irect-buf irect-type @[(rect :x) (rect :y) (rect :w) (rect :h)]))
  (ffi/with-stack [[pix-ptr :ptr nil] [pitch-ptr :int 0]]
    (check-bool (sdl-lock-texture tex (if rect irect-buf nil) pix-ptr pitch-ptr)
                "sdl/lock-texture")
    (when irect-buf (ffi/free irect-buf))
    {:pixels (ffi/read pix-ptr :ptr) :pitch (ffi/read pitch-ptr :int)}))

(defn sdl/unlock-texture [tex]
  "Unlock a previously locked texture."
  (sdl-unlock-texture tex))

(defn sdl/destroy-surface [surface]
  "Destroy an SDL surface."
  (sdl-destroy-surface surface))

# ── Geometry ───────────────────────────────────────────────────────────

(defn sdl/vertex [x y r g b a &named tx ty]
  "Create a vertex struct for render-geometry.
   x,y = position; r,g,b,a = color (0.0-1.0 floats); tx,ty = tex coords (0.0-1.0)."
  {:x x :y y :r r :g g :b b :a a :tx (if tx tx 0.0) :ty (if ty ty 0.0)})

(defn sdl/render-geometry [ren vertices &named texture indices]
  "Render triangles. vertices is an array of vertex structs.
   texture is optional. indices is an optional array of ints."
  (let* [nv (length vertices)
         buf (ffi/malloc (* nv vertex-size))]
    # Write vertices into contiguous buffer
    (var i 0)
    (while (< i nv)
      (let [v (get vertices i)
            off (* i vertex-size)]
        (ffi/write (ptr/add buf off) vertex-type
          @[(v :x) (v :y) (v :r) (v :g) (v :b) (v :a) (v :tx) (v :ty)]))
      (assign i (+ i 1)))
    # Write index buffer if provided
    (var idx-buf nil)
    (var ni 0)
    (when indices
      (assign ni (length indices))
      (assign idx-buf (ffi/malloc (* ni (ffi/size :int))))
      (var j 0)
      (while (< j ni)
        (ffi/write (ptr/add idx-buf (* j (ffi/size :int))) :int (get indices j))
        (assign j (+ j 1))))
    (let [result (sdl-render-geometry ren (if texture texture nil)
                    buf nv (if idx-buf idx-buf nil) ni)]
      (ffi/free buf)
      (when idx-buf (ffi/free idx-buf))
      (check-bool result "sdl/render-geometry"))))

# ── Images (SDL3_image) ────────────────────────────────────────────────

(defn sdl/load-texture [ren path]
  "Load an image file as a texture (PNG, JPG, BMP, etc.)."
  (check-ptr (img-load-texture ren path) "sdl/load-texture"))

(defn sdl/save-png [surface path]
  "Save an SDL_Surface to a PNG file."
  (check-bool (img-save-png surface path) "sdl/save-png"))

# ── TTF (SDL3_ttf) ─────────────────────────────────────────────────────

(defn sdl/ttf-init []
  "Initialize the TTF subsystem."
  (check-bool (ttf-init) "sdl/ttf-init"))

(defn sdl/ttf-quit []
  "Shut down the TTF subsystem."
  (ttf-quit))

(defn sdl/open-font [path size]
  "Open a TTF font at the given point size. Returns a font pointer."
  (check-ptr (ttf-open-font path (float size)) "sdl/open-font"))

(defn sdl/close-font [font]
  "Close a font."
  (ttf-close-font font))

(defn sdl/set-font-size [font size]
  "Set font point size."
  (check-bool (ttf-set-font-size font (float size)) "sdl/set-font-size"))

(defn sdl/set-font-style [font style]
  "Set font style. Use font-* constants (can be OR'd together)."
  (ttf-set-font-style font style))

(defn sdl/text-size [font text]
  "Measure text dimensions. Returns {:width w :height h}."
  (ffi/with-stack [[wp :int 0] [hp :int 0]]
    (check-bool (ttf-get-string-size font text (length text) wp hp) "sdl/text-size")
    {:width (ffi/read wp :int) :height (ffi/read hp :int)}))

(defn sdl/render-text-blended [font text color]
  "Render text to a new ARGB surface with alpha blending.
   color is {:r :g :b :a} (0-255). Returns an SDL_Surface pointer.
   Caller must destroy with sdl/destroy-surface."
  (check-ptr (ttf-render-blended font text (length text)
               @[(color :r) (color :g) (color :b) (color :a)])
             "sdl/render-text-blended"))

(defn sdl/draw-text [ren font text x y color]
  "Convenience: render text and blit to renderer at (x,y).
   color is {:r :g :b :a} or use (sdl:rgb r g b)."
  (let* [surf (sdl/render-text-blended font text color)
         tex  (sdl/create-texture-from-surface ren surf)]
    (sdl/destroy-surface surf)
    (let [sz (sdl/texture-size tex)]
      (sdl/render-texture ren tex
        :dst {:x (float x) :y (float y) :w (sz :width) :h (sz :height)}))
    (sdl/destroy-texture tex)))

# ── Audio ──────────────────────────────────────────────────────────────

(defn sdl/open-audio [&named device format channels freq]
  "Open an audio playback stream. Returns an audio stream pointer.
   :device defaults to default playback. :format defaults to audio-f32.
   :channels defaults to 2 (stereo). :freq defaults to 48000."
  (ffi/with-stack [[spec audio-spec-type @[(if format format audio-f32)
                                           (if channels channels 2)
                                           (if freq freq 48000)]]]
    (check-ptr (sdl-open-audio-device-stream
                 (if device device audio-device-default-playback)
                 spec nil nil)
               "sdl/open-audio")))

(defn sdl/resume-audio [stream]
  "Resume audio playback."
  (check-bool (sdl-resume-audio stream) "sdl/resume-audio"))

(defn sdl/pause-audio [stream]
  "Pause audio playback."
  (check-bool (sdl-pause-audio stream) "sdl/pause-audio"))

(defn sdl/put-audio [stream data]
  "Put audio data into a stream. data is a bytes value."
  (let [ptr (ffi/pin data)]
    (defer (ffi/free ptr)
      (check-bool (sdl-put-audio-data stream ptr (length data)) "sdl/put-audio"))))

(defn sdl/clear-audio [stream]
  "Clear buffered audio data."
  (check-bool (sdl-clear-audio stream) "sdl/clear-audio"))

(defn sdl/destroy-audio [stream]
  "Destroy an audio stream."
  (sdl-destroy-audio stream))

(defn sdl/load-wav [path]
  "Load a WAV file. Returns {:spec {:format f :channels c :freq f} :data bytes :length n}."
  (ffi/with-stack [[spec-buf audio-spec-type @[0 0 0]]
                   [audio-ptr :ptr nil]
                   [audio-len :u32 0]]
    (check-bool (sdl-load-wav path spec-buf audio-ptr audio-len) "sdl/load-wav")
    (let* [sp (ffi/read spec-buf audio-spec-type)
           ptr (ffi/read audio-ptr :ptr)
           len (ffi/read audio-len :u32)
           data (if (> len 0) (ffi/read ptr (ffi/array :u8 len)) (bytes))]
      {:spec {:format (get sp 0) :channels (get sp 1) :freq (get sp 2)}
       :data data
       :length len})))

(defn sdl/audio-playback-devices []
  "Get list of audio playback device IDs."
  (ffi/with-stack [[count-ptr :int 0]]
    (let* [ptr (sdl-get-audio-playback-devices count-ptr)
           count (ffi/read count-ptr :int)]
      (when (null? ptr)
        (if (= count 0) (list) (sdl-error "sdl/audio-playback-devices")))
      (var result @[])
      (var i 0)
      (while (< i count)
        (push result (ffi/read (ptr/add ptr (* i 4)) :u32))
        (assign i (+ i 1)))
      result)))

# ── Input ──────────────────────────────────────────────────────────────

(defn sdl/key-pressed? [scancode]
  "Check if a key is currently pressed (by scancode)."
  (ffi/with-stack [[n-ptr :int 0]]
    (let [state-ptr (sdl-get-keyboard-state n-ptr)]
      (not (= (ffi/read (ptr/add state-ptr scancode) :u8) 0)))))

(defn sdl/mouse-state []
  "Get mouse position and button state. Returns {:x f :y f :buttons u32}."
  (ffi/with-stack [[xp :float 0.0] [yp :float 0.0]]
    (let [buttons (sdl-get-mouse-state xp yp)]
      {:x (ffi/read xp :float) :y (ffi/read yp :float) :buttons buttons})))

(defn sdl/warp-mouse [win x y]
  "Move the mouse to (x,y) within a window."
  (sdl-warp-mouse win (float x) (float y)))

(defn sdl/show-cursor []
  "Show the mouse cursor."
  (check-bool (sdl-show-cursor) "sdl/show-cursor"))

(defn sdl/hide-cursor []
  "Hide the mouse cursor."
  (check-bool (sdl-hide-cursor) "sdl/hide-cursor"))

(defn sdl/set-relative-mouse [win enabled]
  "Enable or disable relative mouse mode for a window."
  (check-bool (sdl-set-relative-mouse win enabled) "sdl/set-relative-mouse"))

(defn sdl/start-text-input [win]
  "Start text input for a window (enables text-input events)."
  (check-bool (sdl-start-text-input win) "sdl/start-text-input"))

(defn sdl/stop-text-input [win]
  "Stop text input for a window."
  (check-bool (sdl-stop-text-input win) "sdl/stop-text-input"))

(defn sdl/set-keyboard-grab [win grabbed]
  "Grab or release keyboard input for a window."
  (check-bool (sdl-set-keyboard-grab win grabbed) "sdl/set-keyboard-grab"))

(defn sdl/set-mouse-grab [win grabbed]
  "Grab or release mouse input for a window."
  (check-bool (sdl-set-mouse-grab win grabbed) "sdl/set-mouse-grab"))

# ── Clipboard ──────────────────────────────────────────────────────────

(defn sdl/set-clipboard [text]
  "Set clipboard text."
  (check-bool (sdl-set-clipboard text) "sdl/set-clipboard"))

(defn sdl/get-clipboard []
  "Get clipboard text. Returns a string."
  (let [ptr (sdl-get-clipboard)]
    (if (null? ptr) "" (ffi/string ptr))))

(defn sdl/has-clipboard? []
  "Check if clipboard has text."
  (sdl-has-clipboard))

# ── Misc ───────────────────────────────────────────────────────────────

(defn sdl/open-url [url]
  "Open a URL in the default browser."
  (check-bool (sdl-open-url url) "sdl/open-url"))

(defn sdl/message-box [title message &named flags window]
  "Show a simple message box. :flags msgbox-error/warning/information."
  (check-bool (sdl-show-message-box (if flags flags msgbox-information)
                                     title message (if window window nil))
              "sdl/message-box"))

(defn sdl/displays []
  "Get list of display IDs."
  (ffi/with-stack [[count-ptr :int 0]]
    (let* [ptr (sdl-get-displays count-ptr)
           count (ffi/read count-ptr :int)]
      (when (null? ptr)
        (if (= count 0) (list) (sdl-error "sdl/displays")))
      (var result @[])
      (var i 0)
      (while (< i count)
        (push result (ffi/read (ptr/add ptr (* i 4)) :u32))
        (assign i (+ i 1)))
      result)))

(defn sdl/display-bounds [display-id]
  "Get display bounds as {:x :y :w :h}."
  (ffi/with-stack [[rect-ptr irect-type @[0 0 0 0]]]
    (check-bool (sdl-get-display-bounds display-id rect-ptr) "sdl/display-bounds")
    (let [r (ffi/read rect-ptr irect-type)]
      {:x (get r 0) :y (get r 1) :w (get r 2) :h (get r 3)})))

(defn sdl/disable-screensaver []
  "Disable the screen saver."
  (check-bool (sdl-disable-screensaver) "sdl/disable-screensaver"))

(defn sdl/enable-screensaver []
  "Enable the screen saver."
  (check-bool (sdl-enable-screensaver) "sdl/enable-screensaver"))

(defn sdl/set-bordered [win bordered]
  "Set window bordered or borderless."
  (check-bool (sdl-set-window-bordered win bordered) "sdl/set-bordered"))

(defn sdl/set-opacity [win opacity]
  "Set window opacity (0.0 transparent, 1.0 opaque)."
  (check-bool (sdl-set-window-opacity win (float opacity)) "sdl/set-opacity"))

(defn sdl/flash-window [win operation]
  "Flash a window. Use flash-cancel/briefly/until-focused."
  (check-bool (sdl-flash-window win operation) "sdl/flash-window"))

(defn sdl/set-icon [win surface]
  "Set window icon from an SDL_Surface."
  (check-bool (sdl-set-window-icon win surface) "sdl/set-icon"))

# ── Resource macros ────────────────────────────────────────────────────

# Note: with-window, with-font, with-texture are documented in the export
# table but implemented as plain higher-order functions with defer.

(defn sdl/with-window* [title w h opts body-fn]
  "Open a window, call (body-fn win), destroy on exit. opts: {:flags n}."
  (let [win (sdl/create-window title w h :flags (if opts (opts :flags) nil))]
    (defer (sdl/destroy-window win)
      (body-fn win))))

(defn sdl/with-font* [path size body-fn]
  "Open a font, call (body-fn font), close on exit."
  (let [font (sdl/open-font path size)]
    (defer (sdl/close-font font)
      (body-fn font))))

(defn sdl/with-texture* [tex body-fn]
  "Call (body-fn tex), destroy texture on exit."
  (defer (sdl/destroy-texture tex)
    (body-fn tex)))

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

 # Textures
 :create-texture    sdl/create-texture
 :destroy-texture   sdl/destroy-texture
 :texture-size      sdl/texture-size
 :set-texture-blend-mode sdl/set-texture-blend-mode
 :set-texture-alpha sdl/set-texture-alpha
 :set-texture-color sdl/set-texture-color
 :set-texture-scale-mode sdl/set-texture-scale-mode
 :render-texture    sdl/render-texture
 :render-texture-rotated sdl/render-texture-rotated
 :set-render-target sdl/set-render-target
 :create-texture-from-surface sdl/create-texture-from-surface
 :lock-texture      sdl/lock-texture
 :unlock-texture    sdl/unlock-texture
 :destroy-surface   sdl/destroy-surface

 # Geometry
 :vertex            sdl/vertex
 :render-geometry   sdl/render-geometry

 # Images
 :load-texture      sdl/load-texture
 :save-png          sdl/save-png

 # TTF
 :ttf-init          sdl/ttf-init
 :ttf-quit          sdl/ttf-quit
 :open-font         sdl/open-font
 :close-font        sdl/close-font
 :set-font-size     sdl/set-font-size
 :set-font-style    sdl/set-font-style
 :text-size         sdl/text-size
 :render-text-blended sdl/render-text-blended
 :draw-text         sdl/draw-text

 # Resource helpers
 :with-window*      sdl/with-window*
 :with-font*        sdl/with-font*
 :with-texture*     sdl/with-texture*

 # Audio
 :open-audio        sdl/open-audio
 :resume-audio      sdl/resume-audio
 :pause-audio       sdl/pause-audio
 :put-audio         sdl/put-audio
 :clear-audio       sdl/clear-audio
 :destroy-audio     sdl/destroy-audio
 :load-wav          sdl/load-wav
 :audio-playback-devices sdl/audio-playback-devices

 # Input
 :key-pressed?      sdl/key-pressed?
 :mouse-state       sdl/mouse-state
 :warp-mouse        sdl/warp-mouse
 :show-cursor       sdl/show-cursor
 :hide-cursor       sdl/hide-cursor
 :set-relative-mouse sdl/set-relative-mouse
 :start-text-input  sdl/start-text-input
 :stop-text-input   sdl/stop-text-input
 :set-keyboard-grab sdl/set-keyboard-grab
 :set-mouse-grab    sdl/set-mouse-grab

 # Clipboard
 :set-clipboard     sdl/set-clipboard
 :get-clipboard     sdl/get-clipboard
 :has-clipboard?    sdl/has-clipboard?

 # Misc
 :open-url          sdl/open-url
 :message-box       sdl/message-box
 :displays          sdl/displays
 :display-bounds    sdl/display-bounds
 :disable-screensaver sdl/disable-screensaver
 :enable-screensaver  sdl/enable-screensaver
 :set-bordered      sdl/set-bordered
 :set-opacity       sdl/set-opacity
 :flash-window      sdl/flash-window
 :set-icon          sdl/set-icon

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
 :kmod-alt     kmod-alt
 :texture-static    texture-static
 :texture-streaming texture-streaming
 :texture-target    texture-target
 :pixfmt-unknown    pixfmt-unknown
 :pixfmt-rgba8888   pixfmt-rgba8888
 :pixfmt-argb8888   pixfmt-argb8888
 :pixfmt-rgba32     pixfmt-rgba32
 :pixfmt-argb32     pixfmt-argb32
 :scalemode-nearest scalemode-nearest
 :scalemode-linear  scalemode-linear
 :flip-none         flip-none
 :flip-horizontal   flip-horizontal
 :flip-vertical     flip-vertical
 :font-normal       font-normal
 :font-bold         font-bold
 :font-italic       font-italic
 :font-underline    font-underline
 :font-strikethrough font-strikethrough
 :audio-u8          audio-u8
 :audio-s16         audio-s16
 :audio-s32         audio-s32
 :audio-f32         audio-f32
 :audio-device-default-playback  audio-device-default-playback
 :audio-device-default-recording audio-device-default-recording
 :msgbox-error      msgbox-error
 :msgbox-warning    msgbox-warning
 :msgbox-information msgbox-information
 :flash-cancel      flash-cancel
 :flash-briefly     flash-briefly
 :flash-until-focused flash-until-focused}

) # end (fn [])
