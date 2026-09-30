(elle/epoch 14)
# audited: 2026-09-30
## SDL3 audio streams, keyboard and mouse input, the clipboard, displays and desktop calls.
## lib/overview.md

(fn [&named libsdl core constants]
  (def c constants)
  (def check-bool core:check-bool)
  (def check-ptr core:check-ptr)

  # SDL_AudioSpec = {SDL_AudioFormat(u32), channels(int), freq(int)} = 12 bytes
  (def audio-spec-type (ffi/struct @[:u32 :int :int]))

  # ── C bindings ────────────────────────────────────────────────────────

  # Audio
  (ffi/defbind sdl-open-audio-device-stream libsdl "SDL_OpenAudioDeviceStream"
               :ptr [:u32 :ptr :ptr :ptr])
  (ffi/defbind sdl-resume-audio libsdl "SDL_ResumeAudioStreamDevice"
               :bool [:ptr])
  (ffi/defbind sdl-pause-audio libsdl "SDL_PauseAudioStreamDevice" :bool [:ptr])
  (ffi/defbind sdl-put-audio-data libsdl "SDL_PutAudioStreamData"
               :bool [:ptr :ptr :int])
  (ffi/defbind sdl-clear-audio libsdl "SDL_ClearAudioStream" :bool [:ptr])
  (ffi/defbind sdl-destroy-audio libsdl "SDL_DestroyAudioStream" :void [:ptr])
  (ffi/defbind sdl-load-wav libsdl "SDL_LoadWAV" :bool [:string :ptr :ptr :ptr])
  (ffi/defbind sdl-get-audio-playback-devices libsdl
               "SDL_GetAudioPlaybackDevices" :ptr [:ptr])

  # Input
  (ffi/defbind sdl-get-keyboard-state libsdl "SDL_GetKeyboardState" :ptr [:ptr])
  (ffi/defbind sdl-get-mouse-state libsdl "SDL_GetMouseState" :u32 [:ptr :ptr])
  (ffi/defbind sdl-warp-mouse libsdl "SDL_WarpMouseInWindow"
               :void [:ptr :float :float])
  (ffi/defbind sdl-show-cursor libsdl "SDL_ShowCursor" :bool [])
  (ffi/defbind sdl-hide-cursor libsdl "SDL_HideCursor" :bool [])
  (ffi/defbind sdl-set-relative-mouse libsdl "SDL_SetWindowRelativeMouseMode"
               :bool [:ptr :bool])
  (ffi/defbind sdl-start-text-input libsdl "SDL_StartTextInput" :bool [:ptr])
  (ffi/defbind sdl-stop-text-input libsdl "SDL_StopTextInput" :bool [:ptr])
  (ffi/defbind sdl-set-keyboard-grab libsdl "SDL_SetWindowKeyboardGrab"
               :bool [:ptr :bool])
  (ffi/defbind sdl-set-mouse-grab libsdl "SDL_SetWindowMouseGrab"
               :bool [:ptr :bool])

  # Clipboard
  (ffi/defbind sdl-set-clipboard libsdl "SDL_SetClipboardText" :bool [:string])
  (ffi/defbind sdl-get-clipboard libsdl "SDL_GetClipboardText" :ptr [])
  (ffi/defbind sdl-has-clipboard libsdl "SDL_HasClipboardText" :bool [])

  # Desktop
  (ffi/defbind sdl-open-url libsdl "SDL_OpenURL" :bool [:string])
  (ffi/defbind sdl-show-message-box libsdl "SDL_ShowSimpleMessageBox"
               :bool [:u32 :string :string :ptr])
  (ffi/defbind sdl-get-displays libsdl "SDL_GetDisplays" :ptr [:ptr])
  (ffi/defbind sdl-get-display-bounds libsdl "SDL_GetDisplayBounds"
               :bool [:u32 :ptr])
  (ffi/defbind sdl-disable-screensaver libsdl "SDL_DisableScreenSaver" :bool [])
  (ffi/defbind sdl-enable-screensaver libsdl "SDL_EnableScreenSaver" :bool [])

  (defn read-u32-array [ptr count]
    "The count u32 values at ptr, as a mutable array."
    (def @result @[])
    (def @i 0)
    (while (< i count)
      (push result (ffi/read (ptr/add ptr (* i 4)) :u32))
      (assign i (+ i 1)))
    result)

  # ── Audio ─────────────────────────────────────────────────────────────

  (defn sdl/open-audio [&named device format channels freq]
    "Open an audio playback stream. Returns an audio stream pointer.
   :device defaults to default playback. :format defaults to audio-f32.
   :channels defaults to 2 (stereo). :freq defaults to 48000."
    (ffi/with-stack [[spec audio-spec-type
                      @[(if format format c:audio-f32) (if channels channels 2)
                        (if freq freq 48000)]]]
                    (check-ptr (sdl-open-audio-device-stream (if device
                                 device
                                 c:audio-device-default-playback) spec nil nil)
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
      (defer
        (ffi/free ptr)
        (check-bool (sdl-put-audio-data stream ptr (length data))
                    "sdl/put-audio"))))

  (defn sdl/clear-audio [stream]
    "Clear buffered audio data."
    (check-bool (sdl-clear-audio stream) "sdl/clear-audio"))

  (defn sdl/destroy-audio [stream]
    "Destroy an audio stream."
    (sdl-destroy-audio stream))

  (defn sdl/load-wav [path]
    "Load a WAV file. Returns {:spec {:format f :channels c :freq f} :data bytes :length n}."
    (ffi/with-stack [[spec-buf audio-spec-type @[0 0 0]] [audio-ptr :ptr nil]
                     [audio-len :u32 0]]
                    (check-bool (sdl-load-wav path spec-buf audio-ptr audio-len)
                                "sdl/load-wav")
                    (let* [sp (ffi/read spec-buf audio-spec-type)
                           ptr (ffi/read audio-ptr :ptr)
                           len (ffi/read audio-len :u32)
                           data (if (> len 0)
                                  (ffi/read ptr (ffi/array :u8 len))
                                  (bytes))]
                      {:spec {:format (get sp 0)
                              :channels (get sp 1)
                              :freq (get sp 2)}
                       :data data
                       :length len})))

  (defn sdl/audio-playback-devices []
    "Get list of audio playback device IDs."
    (ffi/with-stack [[count-ptr :int 0]]
                    (let* [ptr (sdl-get-audio-playback-devices count-ptr)
                           count (ffi/read count-ptr :int)]
                      (when (and (core:null? ptr) (not (= count 0)))
                        (core:sdl-error "sdl/audio-playback-devices"))
                      (read-u32-array ptr count))))

  # ── Input ─────────────────────────────────────────────────────────────

  (defn sdl/key-pressed? [scancode]
    "Check if a key is currently pressed (by scancode)."
    (ffi/with-stack [[n-ptr :int 0]]
                    (let [state-ptr (sdl-get-keyboard-state n-ptr)]
                      (not (= (ffi/read (ptr/add state-ptr scancode) :u8) 0)))))

  (defn sdl/mouse-state []
    "Get mouse position and button state. Returns {:x f :y f :buttons u32}."
    (ffi/with-stack [[xp :float 0.0] [yp :float 0.0]]
                    (let [buttons (sdl-get-mouse-state xp yp)]
                      {:x (ffi/read xp :float)
                       :y (ffi/read yp :float)
                       :buttons buttons})))

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

  # ── Clipboard ─────────────────────────────────────────────────────────

  (defn sdl/set-clipboard [text]
    "Set clipboard text."
    (check-bool (sdl-set-clipboard text) "sdl/set-clipboard"))

  (defn sdl/get-clipboard []
    "Get clipboard text. Returns a string."
    (let [ptr (sdl-get-clipboard)]
      (if (core:null? ptr) "" (ffi/string ptr))))

  (defn sdl/has-clipboard? []
    "Check if clipboard has text."
    (sdl-has-clipboard))

  # ── Desktop ───────────────────────────────────────────────────────────

  (defn sdl/open-url [url]
    "Open a URL in the default browser."
    (check-bool (sdl-open-url url) "sdl/open-url"))

  (defn sdl/message-box [title message &named flags window]
    "Show a simple message box. :flags msgbox-error/warning/information."
    (check-bool (sdl-show-message-box (if flags flags c:msgbox-information)
                                      title message (if window window nil))
                "sdl/message-box"))

  (defn sdl/displays []
    "Get list of display IDs."
    (ffi/with-stack [[count-ptr :int 0]]
                    (let* [ptr (sdl-get-displays count-ptr)
                           count (ffi/read count-ptr :int)]
                      (when (and (core:null? ptr) (not (= count 0)))
                        (core:sdl-error "sdl/displays"))
                      (read-u32-array ptr count))))

  (defn sdl/display-bounds [display-id]
    "Get display bounds as {:x :y :w :h}."
    (ffi/with-stack [[rect-ptr core:irect-type @[0 0 0 0]]]
                    (check-bool (sdl-get-display-bounds display-id rect-ptr)
                                "sdl/display-bounds")
                    (let [r (ffi/read rect-ptr core:irect-type)]
                      {:x (get r 0) :y (get r 1) :w (get r 2) :h (get r 3)})))

  (defn sdl/disable-screensaver []
    "Disable the screen saver."
    (check-bool (sdl-disable-screensaver) "sdl/disable-screensaver"))

  (defn sdl/enable-screensaver []
    "Enable the screen saver."
    (check-bool (sdl-enable-screensaver) "sdl/enable-screensaver"))

  {# Audio
   :open-audio sdl/open-audio
   :resume-audio sdl/resume-audio
   :pause-audio sdl/pause-audio
   :put-audio sdl/put-audio
   :clear-audio sdl/clear-audio
   :destroy-audio sdl/destroy-audio
   :load-wav sdl/load-wav
   :audio-playback-devices sdl/audio-playback-devices

   # Input
   :key-pressed? sdl/key-pressed?
   :mouse-state sdl/mouse-state
   :warp-mouse sdl/warp-mouse
   :show-cursor sdl/show-cursor
   :hide-cursor sdl/hide-cursor
   :set-relative-mouse sdl/set-relative-mouse
   :start-text-input sdl/start-text-input
   :stop-text-input sdl/stop-text-input
   :set-keyboard-grab sdl/set-keyboard-grab
   :set-mouse-grab sdl/set-mouse-grab

   # Clipboard
   :set-clipboard sdl/set-clipboard
   :get-clipboard sdl/get-clipboard
   :has-clipboard? sdl/has-clipboard?

   # Desktop
   :open-url sdl/open-url
   :message-box sdl/message-box
   :displays sdl/displays
   :display-bounds sdl/display-bounds
   :disable-screensaver sdl/disable-screensaver
   :enable-screensaver sdl/enable-screensaver})
