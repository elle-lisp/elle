## spike/gtk4-callback.lisp — Can we connect a GTK signal via ffi/callback?
##
## Tests: Button appears, clicking increments counter, label updates.
## Close button fires close-request signal.

(def libgtk  (ffi/native "libgtk-4.so.1"))
(def libglib (ffi/native "libglib-2.0.so"))
(def libgobj (ffi/native "libgobject-2.0.so"))

# ── GTK init + window ──────────────────────────────────────────────

(ffi/defbind gtk-init                libgtk  "gtk_init"                    :void @[])
(ffi/defbind gtk-window-new          libgtk  "gtk_window_new"              :ptr  @[])
(ffi/defbind gtk-window-present      libgtk  "gtk_window_present"          :void @[:ptr])
(ffi/defbind gtk-window-set-title    libgtk  "gtk_window_set_title"        :void @[:ptr :string])
(ffi/defbind gtk-window-set-default-size libgtk "gtk_window_set_default_size" :void @[:ptr :int :int])
(ffi/defbind gtk-window-set-child    libgtk  "gtk_window_set_child"        :void @[:ptr :ptr])

# ── Widgets ────────────────────────────────────────────────────────

(ffi/defbind gtk-box-new             libgtk  "gtk_box_new"                 :ptr  @[:int :int])
(ffi/defbind gtk-box-append          libgtk  "gtk_box_append"              :void @[:ptr :ptr])
(ffi/defbind gtk-button-new-with-label libgtk "gtk_button_new_with_label"  :ptr  @[:string])
(ffi/defbind gtk-label-new           libgtk  "gtk_label_new"               :ptr  @[:string])
(ffi/defbind gtk-label-set-text      libgtk  "gtk_label_set_text"          :void @[:ptr :string])

# ── GLib main loop ─────────────────────────────────────────────────

(ffi/defbind g-main-context-default  libglib "g_main_context_default"      :ptr  @[])
(ffi/defbind g-main-context-iteration libglib "g_main_context_iteration"   :int  @[:ptr :int])
(ffi/defbind g-main-context-pending  libglib "g_main_context_pending"      :int  @[:ptr])

# ── GObject signals ────────────────────────────────────────────────

(ffi/defbind g-signal-connect-data   libgobj "g_signal_connect_data"
  :ulong @[:ptr :string :ptr :ptr :ptr :int])

# ── Build UI ───────────────────────────────────────────────────────

(gtk-init)
(def win (gtk-window-new))
(gtk-window-set-title win "Callback Spike")
(gtk-window-set-default-size win 400 200)

(def vbox (gtk-box-new 1 8))   # GTK_ORIENTATION_VERTICAL = 1
(def label (gtk-label-new "Click the button"))
(def btn (gtk-button-new-with-label "Click me"))
(gtk-box-append vbox label)
(gtk-box-append vbox btn)
(gtk-window-set-child win vbox)

# ── Connect click signal ───────────────────────────────────────────

(var click-count 0)
(def click-cb-sig (ffi/signature :void @[:ptr :ptr]))
(def click-cb (ffi/callback click-cb-sig
  (fn (widget data)
    (assign click-count (+ click-count 1))
    (gtk-label-set-text label (string "Clicked " click-count " times")))))

(g-signal-connect-data btn "clicked" click-cb nil nil 0)

# ── Connect close-request signal ──────────────────────────────────

(def close-sig (ffi/signature :int @[:ptr]))
(var close-requested false)
(def close-cb (ffi/callback close-sig
  (fn (widget)
    (assign close-requested true)
    1)))   # return 1 = we handle it, don't destroy

(g-signal-connect-data win "close-request" close-cb nil nil 0)

# ── Present and pump ──────────────────────────────────────────────

(gtk-window-present win)

(def ctx (g-main-context-default))
(var frames 0)
(while (and (not close-requested) (< frames 500))
  (while (not (zero? (g-main-context-pending ctx)))
    (g-main-context-iteration ctx 0))
  (assign frames (+ frames 1))
  (time/sleep 0.016))

(println "spike 2: click-count =" click-count "close-requested =" close-requested)
