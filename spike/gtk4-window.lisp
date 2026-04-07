## spike/gtk4-window.lisp — Can we open a GTK4 window and pump events?
##
## Tests: Window appears, event loop pumps, Elle stays responsive.

(def libgtk  (ffi/native "libgtk-4.so.1"))
(def libglib (ffi/native "libglib-2.0.so"))

(ffi/defbind gtk-init                libgtk  "gtk_init"                    :void @[])
(ffi/defbind gtk-window-new          libgtk  "gtk_window_new"              :ptr  @[])
(ffi/defbind gtk-window-present      libgtk  "gtk_window_present"          :void @[:ptr])
(ffi/defbind gtk-window-set-title    libgtk  "gtk_window_set_title"        :void @[:ptr :string])
(ffi/defbind gtk-window-set-default-size libgtk "gtk_window_set_default_size" :void @[:ptr :int :int])
(ffi/defbind g-main-context-default  libglib "g_main_context_default"      :ptr  @[])
(ffi/defbind g-main-context-iteration libglib "g_main_context_iteration"   :int  @[:ptr :int])
(ffi/defbind g-main-context-pending  libglib "g_main_context_pending"      :int  @[:ptr])

(gtk-init)
(def win (gtk-window-new))
(gtk-window-set-title win "Elle Spike 1")
(gtk-window-set-default-size win 400 300)
(gtk-window-present win)

(def ctx (g-main-context-default))
(var frames 0)
(while (< frames 300)
  (while (not (zero? (g-main-context-pending ctx)))
    (g-main-context-iteration ctx 0))
  (assign frames (+ frames 1))
  (time/sleep 0.016))

(println "spike 1: window opened and pumped" frames "frames")
