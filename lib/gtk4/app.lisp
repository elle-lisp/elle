(elle/epoch 9)
## lib/gtk4/app.lisp — GtkApplication lifecycle wrapper
##
## Alternative to gtk4/open for apps that need GtkApplication features
## (single instance, DBus activation, command-line handling).
##
## Usage:
##   (def app ((import "std/gtk4/app")))
##   (def my-app (app:new "org.example.myapp" (fn [handle]
##     (app:build handle [:v-box {:spacing 8}
##       [:label {:id :msg} "Hello"]
##       [:button {:id :ok} "OK"]])
##     (app:set-title handle "My App"))))
##   (app:run my-app :quit (fn [] done?))

(fn []
  (def b ((import "std/gtk4/bind")))
  (def w ((import "std/gtk4/widgets")))

  # ── Callback signatures ──────────────────────────────────────────

  (def sig-activate (ffi/signature :void [:ptr :ptr]))

  # ── Application lifecycle ────────────────────────────────────────

  (defn app/new (app-id on-activate &named @flags)
    "Create a GtkApplication. on-activate receives a window handle."
    (default flags 0)
    (b:gtk-init)
    (let* [app (b:gtk-application-new app-id flags)
           handle (w:make-handle nil)
           cb (ffi/callback sig-activate
                            (fn (app-ptr data)
                              (let [win (b:gtk-application-window-new app-ptr)]
                                (put handle :window win)
                                (w:register-widget handle :window win :window)
                                (b:gtk-window-present win)
                                (on-activate handle))))]
      (put handle :app app)
      (push handle:callbacks cb)
      (b:g-signal-connect-data app "activate" cb nil nil 0)
      handle))

  (defn app/run (handle &named @quit)
    "Run the GtkApplication event loop. Blocks until quit returns true."
    (default quit (fn [] false))
    (let [ctx (b:g-main-context-default)]
      (b:g-application-register handle:app nil nil)
      (b:g-application-activate handle:app)
      (while (not (quit)) (b:glib-wait ctx))))

  (defn app/window (handle)
    "Get the application window pointer."
    handle:window)

  # ── Export ────────────────────────────────────────────────────────

  {:new app/new :run app/run :window app/window})
# end (fn [])
