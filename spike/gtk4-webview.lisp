## spike/gtk4-webview.lisp — Can we embed a WebView and do JS→Elle IPC?
##
## Tests: WebView renders HTML, clicking button sends message,
## callback fires and pushes to array.

(def libgtk    (ffi/native "libgtk-4.so.1"))
(def libglib   (ffi/native "libglib-2.0.so"))
(def libgobj   (ffi/native "libgobject-2.0.so"))
(def libwebkit (ffi/native "libwebkitgtk-6.0.so.4"))
(def libjsc    (ffi/native "libjavascriptcoregtk-6.0.so.1"))

# ── GTK init + window ──────────────────────────────────────────────

(ffi/defbind gtk-init                libgtk  "gtk_init"                    :void @[])
(ffi/defbind gtk-window-new          libgtk  "gtk_window_new"              :ptr  @[])
(ffi/defbind gtk-window-present      libgtk  "gtk_window_present"          :void @[:ptr])
(ffi/defbind gtk-window-set-title    libgtk  "gtk_window_set_title"        :void @[:ptr :string])
(ffi/defbind gtk-window-set-default-size libgtk "gtk_window_set_default_size" :void @[:ptr :int :int])
(ffi/defbind gtk-window-set-child    libgtk  "gtk_window_set_child"        :void @[:ptr :ptr])

# ── GLib main loop ─────────────────────────────────────────────────

(ffi/defbind g-main-context-default  libglib "g_main_context_default"      :ptr  @[])
(ffi/defbind g-main-context-iteration libglib "g_main_context_iteration"   :int  @[:ptr :int])
(ffi/defbind g-main-context-pending  libglib "g_main_context_pending"      :int  @[:ptr])

# ── GObject signals ────────────────────────────────────────────────

(ffi/defbind g-signal-connect-data   libgobj "g_signal_connect_data"
  :ulong @[:ptr :string :ptr :ptr :ptr :int])

# ── WebKit ─────────────────────────────────────────────────────────

(ffi/defbind webkit-web-view-new     libwebkit "webkit_web_view_new"       :ptr  @[])
(ffi/defbind webkit-web-view-load-html libwebkit "webkit_web_view_load_html"
  :void @[:ptr :string :ptr])
(ffi/defbind webkit-web-view-evaluate-javascript libwebkit
  "webkit_web_view_evaluate_javascript"
  :void @[:ptr :string :ssize :ptr :ptr :ptr :ptr])
(ffi/defbind webkit-web-view-get-user-content-manager libwebkit
  "webkit_web_view_get_user_content_manager" :ptr @[:ptr])
(ffi/defbind webkit-ucm-register-script-message-handler libwebkit
  "webkit_user_content_manager_register_script_message_handler"
  :int @[:ptr :string :string])

# ── JSC (JavaScriptCore) ──────────────────────────────────────────

(ffi/defbind jsc-value-to-string     libjsc  "jsc_value_to_string"         :ptr  @[:ptr])

# ── Close-request signal ──────────────────────────────────────────

(def close-sig (ffi/signature :int @[:ptr]))
(var close-requested false)
(def close-cb (ffi/callback close-sig
  (fn (widget)
    (assign close-requested true)
    1)))

# ── Build UI ───────────────────────────────────────────────────────

(gtk-init)
(def win (gtk-window-new))
(gtk-window-set-title win "WebView Spike")
(gtk-window-set-default-size win 800 600)

(def webview (webkit-web-view-new))
(gtk-window-set-child win webview)

# ── Set up IPC: JS → Elle via user content manager ────────────────

(def ucm (webkit-web-view-get-user-content-manager webview))
(webkit-ucm-register-script-message-handler ucm "elle" nil)

(var messages @[])
(def msg-sig (ffi/signature :void @[:ptr :ptr :ptr]))
(def msg-cb (ffi/callback msg-sig
  (fn (manager js-value data)
    (let [[cstr (jsc-value-to-string js-value)]]
      (push messages (ffi/string cstr))))))

(g-signal-connect-data ucm "script-message-received::elle" msg-cb nil nil 0)

# ── Connect close-request ─────────────────────────────────────────

(g-signal-connect-data win "close-request" close-cb nil nil 0)

# ── Load HTML with a button that posts a message ──────────────────

(def html (string
  "<html><body style='font-family:sans-serif;padding:20px'>"
  "<h2>WebView Spike</h2>"
  "<p>Click the button to send a message from JS to Elle:</p>"
  "<button onclick=\"window.webkit.messageHandlers.elle.postMessage('hello from JS')\">"
  "Send Message</button>"
  "<p id='status'>Waiting...</p>"
  "<script>"
  "document.querySelector('button').addEventListener('click', function() {"
  "  document.getElementById('status').textContent = 'Message sent!';"
  "});"
  "</script>"
  "</body></html>"))

(webkit-web-view-load-html webview html nil)

# ── Present and pump ──────────────────────────────────────────────

(gtk-window-present win)

(def ctx (g-main-context-default))
(var frames 0)
(while (and (not close-requested) (< frames 500))
  (while (not (zero? (g-main-context-pending ctx)))
    (g-main-context-iteration ctx 0))
  (assign frames (+ frames 1))
  (time/sleep 0.016))

(println "spike 3: webview messages =" messages)
