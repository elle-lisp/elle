(elle/epoch 14)
# audited: 2026-09-30
## SDL3 bindings over FFI: windows, rendering, textures, text, audio and input, with no plugin.
## lib/overview.md
##
## Needs libSDL3.so, libSDL3_image.so and libSDL3_ttf.so on the system.
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
## demos/conway/conway.lisp is a complete program. The submodules under
## lib/sdl3/ each hold one subject, with its C bindings beside its wrappers.

(fn []
  (def libsdl (ffi/native "libSDL3.so"))
  (def libimg (ffi/native "libSDL3_image.so"))
  (def libttf (ffi/native "libSDL3_ttf.so"))

  (def c ((import "std/sdl3/constants")))
  (def event ((import "std/sdl3/event")))
  (def core ((import "std/sdl3/core") :libsdl libsdl))
  (def window
    ((import "std/sdl3/window") :libsdl libsdl :core core :constants c
                                :event event))
  (def texture
    ((import "std/sdl3/texture") :libsdl libsdl :libimg libimg :libttf libttf
                                 :core core :constants c))
  (def device
    ((import "std/sdl3/device") :libsdl libsdl :core core :constants c))

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

  (def constructors
    {:rgb sdl/rgb :rgba sdl/rgba :rect sdl/rect :point sdl/point})

  # The constants a caller passes; the rest stay internal.
  (def public-constants
    {:init-audio c:init-audio
     :init-video c:init-video
     :init-gamepad c:init-gamepad
     :init-events c:init-events
     :window-fullscreen c:window-fullscreen
     :window-resizable c:window-resizable
     :window-borderless c:window-borderless
     :window-hidden c:window-hidden
     :window-maximized c:window-maximized
     :window-high-dpi c:window-high-dpi
     :window-always-on-top c:window-always-on-top
     :blend-none c:blend-none
     :blend-blend c:blend-blend
     :blend-add c:blend-add
     :blend-mod c:blend-mod
     :blend-mul c:blend-mul
     :scancode-escape c:scancode-escape
     :scancode-return c:scancode-return
     :scancode-space c:scancode-space
     :scancode-tab c:scancode-tab
     :scancode-backspace c:scancode-backspace
     :scancode-up c:scancode-up
     :scancode-down c:scancode-down
     :scancode-left c:scancode-left
     :scancode-right c:scancode-right
     :scancode-f1 c:scancode-f1
     :scancode-f2 c:scancode-f2
     :scancode-f3 c:scancode-f3
     :scancode-f4 c:scancode-f4
     :scancode-f5 c:scancode-f5
     :scancode-f6 c:scancode-f6
     :scancode-f7 c:scancode-f7
     :scancode-f8 c:scancode-f8
     :scancode-f9 c:scancode-f9
     :scancode-f10 c:scancode-f10
     :scancode-f11 c:scancode-f11
     :scancode-f12 c:scancode-f12
     :kmod-shift c:kmod-shift
     :kmod-ctrl c:kmod-ctrl
     :kmod-alt c:kmod-alt
     :texture-static c:texture-static
     :texture-streaming c:texture-streaming
     :texture-target c:texture-target
     :pixfmt-unknown c:pixfmt-unknown
     :pixfmt-rgba8888 c:pixfmt-rgba8888
     :pixfmt-argb8888 c:pixfmt-argb8888
     :pixfmt-rgba32 c:pixfmt-rgba32
     :pixfmt-argb32 c:pixfmt-argb32
     :scalemode-nearest c:scalemode-nearest
     :scalemode-linear c:scalemode-linear
     :flip-none c:flip-none
     :flip-horizontal c:flip-horizontal
     :flip-vertical c:flip-vertical
     :font-normal c:font-normal
     :font-bold c:font-bold
     :font-italic c:font-italic
     :font-underline c:font-underline
     :font-strikethrough c:font-strikethrough
     :audio-u8 c:audio-u8
     :audio-s16 c:audio-s16
     :audio-s32 c:audio-s32
     :audio-f32 c:audio-f32
     :audio-device-default-playback c:audio-device-default-playback
     :audio-device-default-recording c:audio-device-default-recording
     :msgbox-error c:msgbox-error
     :msgbox-warning c:msgbox-warning
     :msgbox-information c:msgbox-information
     :flash-cancel c:flash-cancel
     :flash-briefly c:flash-briefly
     :flash-until-focused c:flash-until-focused})

  (-> window
      (merge texture)
      (merge device)
      (merge constructors)
      (merge public-constants)))
