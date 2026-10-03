(elle/epoch 14)
# audited: 2026-09-30
## The SDL3 constants the bindings pass to C: flags, key codes, modes and formats.
## lib/overview.md
##
## Pure data: loading this module needs no libSDL3. The event types live
## beside their decoder in lib/sdl3/event.lisp.

(fn []
  {# Init flags
   :init-audio 0x00000010
   :init-video 0x00000020
   :init-joystick 0x00000200
   :init-haptic 0x00001000
   :init-gamepad 0x00002000
   :init-events 0x00004000
   :init-sensor 0x00008000
   :init-camera 0x00010000

   # Window flags
   :window-fullscreen 0x01
   :window-opengl 0x02
   :window-hidden 0x08
   :window-borderless 0x10
   :window-resizable 0x20
   :window-minimized 0x40
   :window-maximized 0x80
   :window-high-dpi 0x2000
   :window-always-on-top 0x10000

   # Scancodes
   :scancode-return 40
   :scancode-escape 41
   :scancode-backspace 42
   :scancode-tab 43
   :scancode-space 44
   :scancode-f1 58
   :scancode-f2 59
   :scancode-f3 60
   :scancode-f4 61
   :scancode-f5 62
   :scancode-f6 63
   :scancode-f7 64
   :scancode-f8 65
   :scancode-f9 66
   :scancode-f10 67
   :scancode-f11 68
   :scancode-f12 69
   :scancode-insert 73
   :scancode-home 74
   :scancode-pageup 75
   :scancode-delete 76
   :scancode-end 77
   :scancode-pagedown 78
   :scancode-right 79
   :scancode-left 80
   :scancode-down 81
   :scancode-up 82

   # Key modifiers
   :kmod-none 0x0000
   :kmod-lshift 0x0001
   :kmod-rshift 0x0002
   :kmod-lctrl 0x0040
   :kmod-rctrl 0x0080
   :kmod-lalt 0x0100
   :kmod-ralt 0x0200
   :kmod-lgui 0x0400
   :kmod-rgui 0x0800
   :kmod-num 0x1000
   :kmod-caps 0x2000
   :kmod-scroll 0x8000
   :kmod-shift 0x0003
   :kmod-ctrl 0x00c0
   :kmod-alt 0x0300

   # Blend modes
   :blend-none 0x00000000
   :blend-blend 0x00000001
   :blend-add 0x00000002
   :blend-mod 0x00000004
   :blend-mul 0x00000008

   # Texture access
   :texture-static 0
   :texture-streaming 1
   :texture-target 2

   # Pixel formats; the 32 names are the little-endian aliases
   :pixfmt-unknown 0
   :pixfmt-rgba8888 0x16462004
   :pixfmt-argb8888 0x16362004
   :pixfmt-rgba32 0x16462004
   :pixfmt-argb32 0x16362004

   # Scale modes
   :scalemode-nearest 0
   :scalemode-linear 1

   # Flip modes
   :flip-none 0
   :flip-horizontal 1
   :flip-vertical 2

   # Font styles
   :font-normal 0x00
   :font-bold 0x01
   :font-italic 0x02
   :font-underline 0x04
   :font-strikethrough 0x08

   # Audio
   :audio-u8 0x0008
   :audio-s16 0x8010
   :audio-s32 0x8020
   :audio-f32 0x8120
   :audio-device-default-playback 0xFFFFFFFF
   :audio-device-default-recording 0xFFFFFFFE

   # Message box
   :msgbox-error 0x00000010
   :msgbox-warning 0x00000020
   :msgbox-information 0x00000040

   # Flash operation
   :flash-cancel 0
   :flash-briefly 1
   :flash-until-focused 2})
