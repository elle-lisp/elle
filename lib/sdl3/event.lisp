(elle/epoch 14)
# audited: 2026-09-30
## Decodes an SDL_Event into a struct, and gives SDL a wait in seconds as whole milliseconds.
## lib/overview.md
##
## Loading this module needs no libSDL3: it reads memory through ffi/read.

(fn []

  # ── Event types ───────────────────────────────────────────────────────

  (def event-quit 0x100)
  (def event-key-down 0x300)
  (def event-key-up 0x301)
  (def event-text-input 0x303)
  (def event-mouse-motion 0x400)
  (def event-mouse-button-down 0x401)
  (def event-mouse-button-up 0x402)
  (def event-mouse-wheel 0x403)
  (def event-window-first 0x202)
  (def event-window-last 0x21a)

  (def window-event-names
    {0x202 :shown
     0x203 :hidden
     0x204 :exposed
     0x205 :moved
     0x206 :resized
     0x207 :pixel-size-changed
     0x209 :minimized
     0x20a :maximized
     0x20b :restored
     0x20c :mouse-enter
     0x20d :mouse-leave
     0x20e :focus-gained
     0x20f :focus-lost
     0x210 :close-requested
     0x213 :display-changed
     0x214 :display-scale-changed
     0x216 :occluded
     0x217 :enter-fullscreen
     0x218 :leave-fullscreen
     0x219 :destroyed})

  # ── Layout ────────────────────────────────────────────────────────────
  #
  # SDL_Event is a 128-byte union. Offsets verified against SDL3 headers via
  # offsetof().
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

  (defn read-u8 [buf off]
    (ffi/read (ptr/add buf off) :u8))
  (defn read-u16 [buf off]
    (ffi/read (ptr/add buf off) :u16))
  (defn read-u32 [buf off]
    (ffi/read (ptr/add buf off) :u32))
  (defn read-i32 [buf off]
    (ffi/read (ptr/add buf off) :i32))
  (defn read-u64 [buf off]
    (ffi/read (ptr/add buf off) :u64))
  (defn read-f32 [buf off]
    (ffi/read (ptr/add buf off) :float))
  (defn read-ptr [buf off]
    (ffi/read (ptr/add buf off) :ptr))

  # ── Decoders ──────────────────────────────────────────────────────────

  (defn marshal-keyboard [buf etype]
    {:type (if (= etype event-key-down) :key-down :key-up)
     :timestamp (read-u64 buf 8)
     :window-id (read-u32 buf 16)
     :scancode (read-u32 buf 24)
     :key (read-u32 buf 28)
     :mod (read-u16 buf 32)
     :down (not (= (read-u8 buf 36) 0))
     :repeat (not (= (read-u8 buf 37) 0))})

  (defn marshal-mouse-motion [buf]
    {:type :mouse-motion
     :timestamp (read-u64 buf 8)
     :window-id (read-u32 buf 16)
     :state (read-u32 buf 24)
     :x (read-f32 buf 28)
     :y (read-f32 buf 32)
     :xrel (read-f32 buf 36)
     :yrel (read-f32 buf 40)})

  (defn marshal-mouse-button [buf etype]
    {:type (if (= etype event-mouse-button-down) :mouse-down :mouse-up)
     :timestamp (read-u64 buf 8)
     :window-id (read-u32 buf 16)
     :button (read-u8 buf 24)
     :down (not (= (read-u8 buf 25) 0))
     :clicks (read-u8 buf 26)
     :x (read-f32 buf 28)
     :y (read-f32 buf 32)})

  (defn marshal-mouse-wheel [buf]
    {:type :mouse-wheel
     :timestamp (read-u64 buf 8)
     :window-id (read-u32 buf 16)
     :x (read-f32 buf 24)
     :y (read-f32 buf 28)
     :direction (read-u32 buf 32)
     :mouse-x (read-f32 buf 36)
     :mouse-y (read-f32 buf 40)})

  (defn marshal-window [buf etype]
    (let [subtype (get window-event-names etype)]
      {:type :window
       :subtype (if (nil? subtype) :unknown subtype)
       :timestamp (read-u64 buf 8)
       :window-id (read-u32 buf 16)
       :data1 (read-i32 buf 20)
       :data2 (read-i32 buf 24)}))

  (defn marshal-text-input [buf]
    (let [text-ptr (read-ptr buf 24)]
      {:type :text-input
       :timestamp (read-u64 buf 8)
       :window-id (read-u32 buf 16)
       :text (if (= (ptr/to-int text-ptr) 0) "" (ffi/string text-ptr))}))

  (defn marshal-event [buf]
    "Read one event from a 128-byte buffer. Returns a struct."
    (let [etype (read-u32 buf 0)]
      (case etype
        event-quit {:type :quit :timestamp (read-u64 buf 8)}
        event-key-down (marshal-keyboard buf etype)
        event-key-up (marshal-keyboard buf etype)
        event-mouse-motion (marshal-mouse-motion buf)
        event-mouse-button-down (marshal-mouse-button buf etype)
        event-mouse-button-up (marshal-mouse-button buf etype)
        event-mouse-wheel (marshal-mouse-wheel buf)
        event-text-input (marshal-text-input buf)
        (if (and (>= etype event-window-first) (<= etype event-window-last))
          (marshal-window buf etype)
          {:type :unknown :raw-type etype :timestamp (read-u64 buf 8)}))))

  # ── Waits ─────────────────────────────────────────────────────────────

  (defn timeout-ms [seconds]
    "The whole milliseconds SDL waits for a wait of `seconds`, rounded up.
   Raises :type-error when seconds is not a number, and :argument-error when
   it is negative, not finite, or longer than 2147483.647 seconds."
    seconds)

  {:marshal-event marshal-event :timeout-ms timeout-ms})
