(elle/epoch 14)
# audited: 2026-09-30
## SDL3 textures, surfaces, triangle geometry, image files (SDL3_image) and text (SDL3_ttf).
## lib/overview.md

(fn [&named libsdl libimg libttf core constants]
  (def c constants)
  (def check-bool core:check-bool)
  (def check-ptr core:check-ptr)

  (def fpoint-type (ffi/struct @[:float :float]))
  (def color-type (ffi/struct @[:u8 :u8 :u8 :u8]))
  # SDL_Vertex = SDL_FPoint position + SDL_FColor color + SDL_FPoint tex_coord
  (def vertex-type
    (ffi/struct @[:float :float :float :float :float :float :float :float]))
  (def vertex-size (ffi/size vertex-type))

  # ── C bindings ────────────────────────────────────────────────────────

  # Textures
  (ffi/defbind sdl-create-texture libsdl "SDL_CreateTexture"
               :ptr [:ptr :u32 :int :int :int])
  (ffi/defbind sdl-destroy-texture libsdl "SDL_DestroyTexture" :void [:ptr])
  (ffi/defbind sdl-render-texture libsdl "SDL_RenderTexture"
               :bool [:ptr :ptr :ptr :ptr])
  (ffi/defbind sdl-render-texture-rotated libsdl "SDL_RenderTextureRotated"
               :bool [:ptr :ptr :ptr :ptr :double :ptr :int])
  (ffi/defbind sdl-set-texture-blend libsdl "SDL_SetTextureBlendMode"
               :bool [:ptr :u32])
  (ffi/defbind sdl-set-texture-alpha libsdl "SDL_SetTextureAlphaMod"
               :bool [:ptr :u8])
  (ffi/defbind sdl-set-texture-color libsdl "SDL_SetTextureColorMod"
               :bool [:ptr :u8 :u8 :u8])
  (ffi/defbind sdl-set-texture-scale-mode libsdl "SDL_SetTextureScaleMode"
               :bool [:ptr :int])
  (ffi/defbind sdl-get-texture-size libsdl "SDL_GetTextureSize"
               :bool [:ptr :ptr :ptr])
  (ffi/defbind sdl-set-render-target libsdl "SDL_SetRenderTarget"
               :bool [:ptr :ptr])
  (ffi/defbind sdl-create-texture-from-surface libsdl
               "SDL_CreateTextureFromSurface" :ptr [:ptr :ptr])
  (ffi/defbind sdl-lock-texture libsdl "SDL_LockTexture"
               :bool [:ptr :ptr :ptr :ptr])
  (ffi/defbind sdl-unlock-texture libsdl "SDL_UnlockTexture" :void [:ptr])

  # Surface
  (ffi/defbind sdl-destroy-surface libsdl "SDL_DestroySurface" :void [:ptr])

  # Geometry
  (ffi/defbind sdl-render-geometry libsdl "SDL_RenderGeometry"
               :bool [:ptr :ptr :ptr :int :ptr :int])

  # Images (SDL3_image)
  (ffi/defbind img-load-texture libimg "IMG_LoadTexture" :ptr [:ptr :string])
  (ffi/defbind img-save-png libimg "IMG_SavePNG" :bool [:ptr :string])

  # TTF (SDL3_ttf)
  (ffi/defbind ttf-init libttf "TTF_Init" :bool [])
  (ffi/defbind ttf-quit libttf "TTF_Quit" :void [])
  (ffi/defbind ttf-open-font libttf "TTF_OpenFont" :ptr [:string :float])
  (ffi/defbind ttf-close-font libttf "TTF_CloseFont" :void [:ptr])
  (ffi/defbind ttf-set-font-size libttf "TTF_SetFontSize" :bool [:ptr :float])
  (ffi/defbind ttf-set-font-style libttf "TTF_SetFontStyle" :void [:ptr :int])
  (ffi/defbind ttf-get-string-size libttf "TTF_GetStringSize"
               :bool [:ptr :string :size :ptr :ptr])
  (ffi/defbind ttf-render-blended libttf "TTF_RenderText_Blended"
               :ptr [:ptr :string :size color-type])

  # Scratch memory, reused across calls to avoid per-frame allocation
  (def rect-buf (ffi/malloc (ffi/size core:frect-type)))
  (def rect-buf-2 (ffi/malloc (ffi/size core:frect-type)))
  (def point-buf (ffi/malloc (ffi/size fpoint-type)))

  # ── Textures ──────────────────────────────────────────────────────────

  (defn sdl/create-texture [ren format access w h]
    "Create a texture. format: pixel format constant, access: texture-static/streaming/target."
    (check-ptr (sdl-create-texture ren format access w h) "sdl/create-texture"))

  (defn sdl/destroy-texture [tex]
    "Destroy a texture."
    (sdl-destroy-texture tex))

  (defn sdl/with-texture* [tex body-fn]
    "Call (body-fn tex), destroy texture on exit."
    (defer
      (sdl/destroy-texture tex)
      (body-fn tex)))

  (defn sdl/texture-size [tex]
    "Get texture size as {:width w :height h}."
    (ffi/with-stack [[wp :float 0.0] [hp :float 0.0]]
                    (check-bool (sdl-get-texture-size tex wp hp)
                                "sdl/texture-size")
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
    (check-bool (sdl-set-texture-scale-mode tex mode)
                "sdl/set-texture-scale-mode"))

  (defn sdl/render-texture [ren tex &named src dst]
    "Render a texture. src/dst are {:x :y :w :h} or nil for full."
    (when src (core:write-frect rect-buf (src :x) (src :y) (src :w) (src :h)))
    (when dst
      (ffi/write rect-buf-2 core:frect-type
                 @[(dst :x) (dst :y) (dst :w) (dst :h)]))
    (check-bool (sdl-render-texture ren tex (if src rect-buf nil)
                                    (if dst rect-buf-2 nil))
                "sdl/render-texture"))

  (defn sdl/render-texture-rotated [ren tex angle &named src dst center flip]
    "Render a texture with rotation. angle in degrees.
   center is {:x :y} or nil for texture center. flip: flip-none/horizontal/vertical."
    (when src (core:write-frect rect-buf (src :x) (src :y) (src :w) (src :h)))
    (when dst
      (ffi/write rect-buf-2 core:frect-type
                 @[(dst :x) (dst :y) (dst :w) (dst :h)]))
    (when center
      (ffi/write point-buf fpoint-type @[(center :x) (center :y)]))
    (check-bool (sdl-render-texture-rotated ren tex (if src rect-buf nil)
                (if dst rect-buf-2 nil) (float angle) (if center point-buf nil)
                (if flip flip c:flip-none)) "sdl/render-texture-rotated"))

  (defn sdl/set-render-target [ren tex]
    "Set render target to a texture, or nil to reset to default."
    (check-bool (sdl-set-render-target ren tex) "sdl/set-render-target"))

  (defn sdl/create-texture-from-surface [ren surface]
    "Create a texture from an SDL_Surface. Caller must destroy the surface separately."
    (check-ptr (sdl-create-texture-from-surface ren surface)
               "sdl/create-texture-from-surface"))

  (defn sdl/lock-texture [tex &named rect]
    "Lock a streaming texture for pixel access. Returns {:pixels ptr :pitch int}.
   rect is {:x :y :w :h} or nil for entire texture."
    (def @irect-buf nil)
    (when rect
      (assign irect-buf (ffi/malloc (ffi/size core:irect-type)))
      (ffi/write irect-buf core:irect-type
                 @[(rect :x) (rect :y) (rect :w) (rect :h)]))
    (ffi/with-stack [[pix-ptr :ptr nil] [pitch-ptr :int 0]]
                    (check-bool (sdl-lock-texture tex (if rect irect-buf nil)
                                pix-ptr pitch-ptr) "sdl/lock-texture")
                    (when irect-buf (ffi/free irect-buf))
                    {:pixels (ffi/read pix-ptr :ptr)
                     :pitch (ffi/read pitch-ptr :int)}))

  (defn sdl/unlock-texture [tex]
    "Unlock a previously locked texture."
    (sdl-unlock-texture tex))

  (defn sdl/destroy-surface [surface]
    "Destroy an SDL surface."
    (sdl-destroy-surface surface))

  # ── Geometry ──────────────────────────────────────────────────────────

  (defn sdl/vertex [x y r g b a &named tx ty]
    "Create a vertex struct for render-geometry.
   x,y = position; r,g,b,a = color (0.0-1.0 floats); tx,ty = tex coords (0.0-1.0)."
    {:x x :y y :r r :g g :b b :a a :tx (if tx tx 0.0) :ty (if ty ty 0.0)})

  (defn sdl/render-geometry [ren vertices &named texture indices]
    "Render triangles. vertices is an array of vertex structs.
   texture is optional. indices is an optional array of ints."
    (let* [nv (length vertices)
           buf (ffi/malloc (* nv vertex-size))]
      (def @i 0)
      (while (< i nv)
        (let [v (get vertices i)
              off (* i vertex-size)]
          (ffi/write (ptr/add buf off) vertex-type
                     @[(v :x) (v :y) (v :r) (v :g) (v :b) (v :a) (v :tx) (v :ty)]))
        (assign i (+ i 1)))
      (def @idx-buf nil)
      (def @ni 0)
      (when indices
        (assign ni (length indices))
        (assign idx-buf (ffi/malloc (* ni (ffi/size :int))))
        (def @j 0)
        (while (< j ni)
          (ffi/write (ptr/add idx-buf (* j (ffi/size :int)))
                     :int (get indices j))
          (assign j (+ j 1))))
      (let [result (sdl-render-geometry ren (if texture texture nil) buf nv
                                        (if idx-buf idx-buf nil) ni)]
        (ffi/free buf)
        (when idx-buf (ffi/free idx-buf))
        (check-bool result "sdl/render-geometry"))))

  # ── Images (SDL3_image) ───────────────────────────────────────────────

  (defn sdl/load-texture [ren path]
    "Load an image file as a texture (PNG, JPG, BMP, etc.)."
    (check-ptr (img-load-texture ren path) "sdl/load-texture"))

  (defn sdl/save-png [surface path]
    "Save an SDL_Surface to a PNG file."
    (check-bool (img-save-png surface path) "sdl/save-png"))

  # ── TTF (SDL3_ttf) ────────────────────────────────────────────────────

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

  (defn sdl/with-font* [path size body-fn]
    "Open a font, call (body-fn font), close on exit."
    (let [font (sdl/open-font path size)]
      (defer
        (sdl/close-font font)
        (body-fn font))))

  (defn sdl/set-font-size [font size]
    "Set font point size."
    (check-bool (ttf-set-font-size font (float size)) "sdl/set-font-size"))

  (defn sdl/set-font-style [font style]
    "Set font style. Use font-* constants (can be OR'd together)."
    (ttf-set-font-style font style))

  (defn sdl/text-size [font text]
    "Measure text dimensions. Returns {:width w :height h}."
    (ffi/with-stack [[wp :int 0] [hp :int 0]]
                    (check-bool (ttf-get-string-size font text (length text) wp
                                hp) "sdl/text-size")
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
           tex (sdl/create-texture-from-surface ren surf)]
      (sdl/destroy-surface surf)
      (let [sz (sdl/texture-size tex)]
        (sdl/render-texture ren tex
                            :dst {:x (float x)
                                  :y (float y)
                                  :w (sz :width)
                                  :h (sz :height)}))
      (sdl/destroy-texture tex)))

  {# Textures
   :create-texture sdl/create-texture
   :destroy-texture sdl/destroy-texture
   :with-texture* sdl/with-texture*
   :texture-size sdl/texture-size
   :set-texture-blend-mode sdl/set-texture-blend-mode
   :set-texture-alpha sdl/set-texture-alpha
   :set-texture-color sdl/set-texture-color
   :set-texture-scale-mode sdl/set-texture-scale-mode
   :render-texture sdl/render-texture
   :render-texture-rotated sdl/render-texture-rotated
   :set-render-target sdl/set-render-target
   :create-texture-from-surface sdl/create-texture-from-surface
   :lock-texture sdl/lock-texture
   :unlock-texture sdl/unlock-texture
   :destroy-surface sdl/destroy-surface

   # Geometry
   :vertex sdl/vertex
   :render-geometry sdl/render-geometry

   # Images
   :load-texture sdl/load-texture
   :save-png sdl/save-png

   # TTF
   :ttf-init sdl/ttf-init
   :ttf-quit sdl/ttf-quit
   :open-font sdl/open-font
   :close-font sdl/close-font
   :with-font* sdl/with-font*
   :set-font-size sdl/set-font-size
   :set-font-style sdl/set-font-style
   :text-size sdl/text-size
   :render-text-blended sdl/render-text-blended
   :draw-text sdl/draw-text})
