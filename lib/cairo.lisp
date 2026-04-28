(elle/epoch 9)
## lib/cairo.lisp — Cairo 2D graphics bindings
##
## Standalone module for Cairo rendering. Not GTK-specific — usable
## with any surface target (image buffers, PDF, SVG, X11, Wayland).
##
## Usage:
##   (def cairo ((import "std/cairo")))
##   (def surf (cairo:image-surface-for-data buf cairo:FORMAT_ARGB32 w h stride))
##   (cairo:scale cr sx sy)
##   (cairo:set-source-surface cr surf 0.0 0.0)
##   (cairo:paint cr)
##   (cairo:surface-destroy surf)

(fn []
  (def lib (ffi/native "libcairo.so.2"))

  # ── Constants ────────────────────────────────────────────────────

  (def FORMAT_ARGB32 0)
  (def FORMAT_RGB24 1)
  (def FORMAT_A8 2)
  (def FORMAT_A1 3)

  # ── Surfaces ─────────────────────────────────────────────────────

  (ffi/defbind image-surface
               lib
               "cairo_image_surface_create"
               :ptr [:int :int :int])
  (ffi/defbind image-surface-for-data
               lib
               "cairo_image_surface_create_for_data"
               :ptr [:ptr :int :int :int :int])
  (ffi/defbind surface-destroy lib "cairo_surface_destroy" :void [:ptr])
  (ffi/defbind surface-flush lib "cairo_surface_flush" :void [:ptr])

  # ── Context lifecycle ────────────────────────────────────────────

  (ffi/defbind create lib "cairo_create" :ptr [:ptr])
  (ffi/defbind destroy lib "cairo_destroy" :void [:ptr])
  (ffi/defbind save lib "cairo_save" :void [:ptr])
  (ffi/defbind restore lib "cairo_restore" :void [:ptr])

  # ── Transforms ───────────────────────────────────────────────────

  (ffi/defbind scale lib "cairo_scale" :void [:ptr :double :double])
  (ffi/defbind translate lib "cairo_translate" :void [:ptr :double :double])
  (ffi/defbind rotate lib "cairo_rotate" :void [:ptr :double])

  # ── Source ───────────────────────────────────────────────────────

  (ffi/defbind set-source-rgb
               lib
               "cairo_set_source_rgb"
               :void [:ptr :double :double :double])
  (ffi/defbind set-source-rgba
               lib
               "cairo_set_source_rgba"
               :void [:ptr :double :double :double :double])
  (ffi/defbind set-source-surface
               lib
               "cairo_set_source_surface"
               :void [:ptr :ptr :double :double])

  # ── Drawing ──────────────────────────────────────────────────────

  (ffi/defbind paint lib "cairo_paint" :void [:ptr])
  (ffi/defbind paint-with-alpha
               lib
               "cairo_paint_with_alpha"
               :void [:ptr :double])
  (ffi/defbind stroke lib "cairo_stroke" :void [:ptr])
  (ffi/defbind fill lib "cairo_fill" :void [:ptr])
  (ffi/defbind set-line-width lib "cairo_set_line_width" :void [:ptr :double])

  # ── Path ─────────────────────────────────────────────────────────

  (ffi/defbind move-to lib "cairo_move_to" :void [:ptr :double :double])
  (ffi/defbind line-to lib "cairo_line_to" :void [:ptr :double :double])
  (ffi/defbind curve-to
               lib
               "cairo_curve_to"
               :void [:ptr :double :double :double :double :double :double])
  (ffi/defbind arc
               lib
               "cairo_arc"
               :void [:ptr :double :double :double :double :double])
  (ffi/defbind rectangle
               lib
               "cairo_rectangle"
               :void [:ptr :double :double :double :double])
  (ffi/defbind close-path lib "cairo_close_path" :void [:ptr])
  (ffi/defbind new-path lib "cairo_new_path" :void [:ptr])

  # ── Export ───────────────────────────────────────────────────────

  {
   # constants
   :FORMAT_ARGB32 FORMAT_ARGB32
   :FORMAT_RGB24 FORMAT_RGB24
   :FORMAT_A8 FORMAT_A8
   :FORMAT_A1 FORMAT_A1  # surfaces
   :image-surface image-surface
   :image-surface-for-data image-surface-for-data
   :surface-destroy surface-destroy
   :surface-flush surface-flush  # context
   :create create
   :destroy destroy
   :save save
   :restore restore  # transforms
   :scale scale
   :translate translate
   :rotate rotate  # source
   :set-source-rgb set-source-rgb
   :set-source-rgba set-source-rgba
   :set-source-surface set-source-surface  # drawing
   :paint paint
   :paint-with-alpha paint-with-alpha
   :stroke stroke
   :fill fill
   :set-line-width set-line-width  # path
   :move-to move-to
   :line-to line-to
   :curve-to curve-to
   :arc arc
   :rectangle rectangle
   :close-path close-path
   :new-path new-path})
# end (fn [])
