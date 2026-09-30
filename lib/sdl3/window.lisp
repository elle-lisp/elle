(elle/epoch 14)
# audited: 2026-09-30
## SDL3 lifecycle, windows, the renderer's drawing calls, events and timing.
## lib/overview.md

(fn [&named libsdl core constants event]
  (def c constants)
  (def check-bool core:check-bool)
  (def check-ptr core:check-ptr)

  # ── C bindings ────────────────────────────────────────────────────────

  # Init / quit
  (ffi/defbind sdl-init libsdl "SDL_Init" :bool [:u32])
  (ffi/defbind sdl-quit libsdl "SDL_Quit" :void [])

  # Window
  (ffi/defbind sdl-create-window libsdl "SDL_CreateWindow"
               :ptr [:string :int :int :u64])
  (ffi/defbind sdl-destroy-window libsdl "SDL_DestroyWindow" :void [:ptr])
  (ffi/defbind sdl-set-window-title libsdl "SDL_SetWindowTitle"
               :bool [:ptr :string])
  (ffi/defbind sdl-get-window-size libsdl "SDL_GetWindowSize"
               :bool [:ptr :ptr :ptr])
  (ffi/defbind sdl-set-window-size libsdl "SDL_SetWindowSize"
               :bool [:ptr :int :int])
  (ffi/defbind sdl-get-window-pos libsdl "SDL_GetWindowPosition"
               :bool [:ptr :ptr :ptr])
  (ffi/defbind sdl-set-window-pos libsdl "SDL_SetWindowPosition"
               :bool [:ptr :int :int])
  (ffi/defbind sdl-set-fullscreen libsdl "SDL_SetWindowFullscreen"
               :bool [:ptr :bool])
  (ffi/defbind sdl-set-window-bordered libsdl "SDL_SetWindowBordered"
               :bool [:ptr :bool])
  (ffi/defbind sdl-set-window-opacity libsdl "SDL_SetWindowOpacity"
               :bool [:ptr :float])
  (ffi/defbind sdl-flash-window libsdl "SDL_FlashWindow" :bool [:ptr :int])
  (ffi/defbind sdl-set-window-icon libsdl "SDL_SetWindowIcon" :bool [:ptr :ptr])

  # Renderer
  (ffi/defbind sdl-create-renderer libsdl "SDL_CreateRenderer"
               :ptr [:ptr :string])
  (ffi/defbind sdl-destroy-renderer libsdl "SDL_DestroyRenderer" :void [:ptr])
  (ffi/defbind sdl-set-draw-color libsdl "SDL_SetRenderDrawColor"
               :bool [:ptr :u8 :u8 :u8 :u8])
  (ffi/defbind sdl-render-clear libsdl "SDL_RenderClear" :bool [:ptr])
  (ffi/defbind sdl-render-present libsdl "SDL_RenderPresent" :bool [:ptr])
  (ffi/defbind sdl-render-point libsdl "SDL_RenderPoint"
               :bool [:ptr :float :float])
  (ffi/defbind sdl-render-line libsdl "SDL_RenderLine"
               :bool [:ptr :float :float :float :float])
  (ffi/defbind sdl-render-rect libsdl "SDL_RenderRect" :bool [:ptr :ptr])
  (ffi/defbind sdl-render-fill-rect libsdl "SDL_RenderFillRect"
               :bool [:ptr :ptr])
  (ffi/defbind sdl-set-blend-mode libsdl "SDL_SetRenderDrawBlendMode"
               :bool [:ptr :u32])
  (ffi/defbind sdl-set-scale libsdl "SDL_SetRenderScale"
               :bool [:ptr :float :float])
  (ffi/defbind sdl-set-vsync libsdl "SDL_SetRenderVSync" :bool [:ptr :int])
  (ffi/defbind sdl-debug-text libsdl "SDL_RenderDebugText"
               :bool [:ptr :float :float :string])

  # Events
  (ffi/defbind sdl-poll-event libsdl "SDL_PollEvent" :bool [:ptr])
  (ffi/defbind sdl-wait-event libsdl "SDL_WaitEvent" :bool [:ptr])
  (ffi/defbind sdl-wait-event-timeout libsdl "SDL_WaitEventTimeout"
               :bool [:ptr :i32])

  # Timing
  (ffi/defbind sdl-get-ticks libsdl "SDL_GetTicks" :u64 [])
  (ffi/defbind sdl-delay libsdl "SDL_Delay" :void [:u32])
  (ffi/defbind sdl-delay-ns libsdl "SDL_DelayNS" :void [:u64])
  (ffi/defbind sdl-perf-counter libsdl "SDL_GetPerformanceCounter" :u64 [])
  (ffi/defbind sdl-perf-frequency libsdl "SDL_GetPerformanceFrequency" :u64 [])

  # Scratch memory, reused across calls to avoid per-frame allocation
  (def rect-buf (ffi/malloc (ffi/size core:frect-type)))
  (def event-buf (ffi/malloc 128))

  # ── Lifecycle ─────────────────────────────────────────────────────────

  (defn
    sdl/init
    [&named audio video joystick haptic gamepad events sensor camera]
    "Initialize SDL subsystems. Pass keyword flags, e.g. (sdl/init :video true).
   With no arguments, initializes video (which implies events)."
    (let [flags (+ (if audio c:init-audio 0) (if video c:init-video 0)
                   (if joystick c:init-joystick 0) (if haptic c:init-haptic 0)
                   (if gamepad c:init-gamepad 0) (if events c:init-events 0)
                   (if sensor c:init-sensor 0) (if camera c:init-camera 0))]
      (let [f (if (zero? flags) c:init-video flags)]
        (check-bool (sdl-init f) "sdl/init"))))

  (defn sdl/quit []
    "Shut down all SDL subsystems."
    (sdl-quit))

  # ── Window ────────────────────────────────────────────────────────────

  (defn sdl/create-window [title width height &named flags]
    "Create a window. Returns a window pointer.
   :flags is a u64 window flags bitmask (default 0)."
    (check-ptr (sdl-create-window title width height (if flags flags 0))
               "sdl/create-window"))

  (defn sdl/destroy-window [win]
    "Destroy a window."
    (sdl-destroy-window win))

  (defn sdl/with-window* [title w h opts body-fn]
    "Open a window, call (body-fn win), destroy on exit. opts: {:flags n}."
    (let [win (sdl/create-window title w h :flags (if opts (opts :flags) nil))]
      (defer
        (sdl/destroy-window win)
        (body-fn win))))

  (defn sdl/set-title [win title]
    "Set window title."
    (check-bool (sdl-set-window-title win title) "sdl/set-title"))

  (defn sdl/window-size [win]
    "Get window size as {:width w :height h}."
    (ffi/with-stack [[wp :int 0] [hp :int 0]]
                    (check-bool (sdl-get-window-size win wp hp)
                                "sdl/window-size")
                    {:width (ffi/read wp :int) :height (ffi/read hp :int)}))

  (defn sdl/set-window-size [win w h]
    "Set window size."
    (check-bool (sdl-set-window-size win w h) "sdl/set-window-size"))

  (defn sdl/window-position [win]
    "Get window position as {:x x :y y}."
    (ffi/with-stack [[xp :int 0] [yp :int 0]]
                    (check-bool (sdl-get-window-pos win xp yp)
                                "sdl/window-position")
                    {:x (ffi/read xp :int) :y (ffi/read yp :int)}))

  (defn sdl/set-window-position [win x y]
    "Set window position."
    (check-bool (sdl-set-window-pos win x y) "sdl/set-window-position"))

  (defn sdl/set-fullscreen [win fullscreen]
    "Set fullscreen mode."
    (check-bool (sdl-set-fullscreen win fullscreen) "sdl/set-fullscreen"))

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

  # ── Renderer ──────────────────────────────────────────────────────────

  (defn sdl/create-renderer [win &named name]
    "Create a renderer for a window. Returns a renderer pointer.
   :name selects a specific backend (default nil = auto)."
    (check-ptr (sdl-create-renderer win name) "sdl/create-renderer"))

  (defn sdl/destroy-renderer [ren]
    "Destroy a renderer."
    (sdl-destroy-renderer ren))

  (defn sdl/set-color [ren r g b &named a]
    "Set the draw color. Alpha defaults to 255."
    (check-bool (sdl-set-draw-color ren r g b (if a a 255)) "sdl/set-color"))

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

  (defn sdl/draw-rect [ren x y w h]
    "Draw a rectangle outline."
    (core:write-frect rect-buf x y w h)
    (check-bool (sdl-render-rect ren rect-buf) "sdl/draw-rect"))

  (defn sdl/fill-rect [ren x y w h]
    "Draw a filled rectangle."
    (core:write-frect rect-buf x y w h)
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

  # ── Events ────────────────────────────────────────────────────────────

  (defn sdl/poll-events []
    "Poll all pending events. Returns an array of event structs.
   Each event has at minimum :type and :timestamp."
    (let [events @[]]
      (while (sdl-poll-event event-buf)
        (push events (event:marshal-event event-buf)))
      events))

  (defn sdl/wait-event []
    "Wait for an event (blocks). Returns a single event struct."
    (check-bool (sdl-wait-event event-buf) "sdl/wait-event")
    (event:marshal-event event-buf))

  (defn sdl/wait-event-timeout [timeout-ms]
    "Wait up to a number of seconds for an event. Returns the event struct, or
   nil when none arrives in time. SDL counts the wait in whole milliseconds,
   so a wait between two of them rounds up. A wait that is negative, not
   finite, or longer than 2147483.647 seconds is an :argument-error."
    (if (sdl-wait-event-timeout event-buf timeout-ms)
      (event:marshal-event event-buf)
      nil))

  # ── Timing ────────────────────────────────────────────────────────────

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

  {# Lifecycle
   :init sdl/init
   :quit sdl/quit
   :error-string core:error-string

   # Window
   :create-window sdl/create-window
   :destroy-window sdl/destroy-window
   :with-window* sdl/with-window*
   :set-title sdl/set-title
   :window-size sdl/window-size
   :set-window-size sdl/set-window-size
   :window-position sdl/window-position
   :set-window-position sdl/set-window-position
   :set-fullscreen sdl/set-fullscreen
   :set-bordered sdl/set-bordered
   :set-opacity sdl/set-opacity
   :flash-window sdl/flash-window
   :set-icon sdl/set-icon

   # Renderer
   :create-renderer sdl/create-renderer
   :destroy-renderer sdl/destroy-renderer
   :set-color sdl/set-color
   :clear sdl/clear
   :present sdl/present
   :draw-point sdl/draw-point
   :draw-line sdl/draw-line
   :draw-rect sdl/draw-rect
   :fill-rect sdl/fill-rect
   :debug-text sdl/debug-text
   :set-blend-mode sdl/set-blend-mode
   :set-scale sdl/set-scale
   :set-vsync sdl/set-vsync

   # Events
   :poll-events sdl/poll-events
   :wait-event sdl/wait-event
   :wait-event-timeout sdl/wait-event-timeout

   # Timing
   :ticks sdl/ticks
   :delay sdl/delay
   :delay-ns sdl/delay-ns
   :perf-counter sdl/perf-counter
   :perf-frequency sdl/perf-frequency})
