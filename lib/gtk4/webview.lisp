(elle/epoch 7)
## lib/gtk4/webview.lisp — WebKit6 WebView integration
##
## Creates webviews, sets up JS→Elle IPC, provides eval/send/load.

(fn []

(def b ((import "std/gtk4/bind")))
(def w ((import "std/gtk4/widgets")))

# ── Signal signatures ─────────────────────────────────────────────

(def sig-void-ptr-ptr-ptr (ffi/signature :void [:ptr :ptr :ptr]))

# ── Helpers ───────────────────────────────────────────────────────

(defn json-escape (s)
  "Escape a string for embedding in JS. Wraps in double quotes."
  (string "\""
    (-> s
      (string/replace "\\" "\\\\")
      (string/replace "\"" "\\\"")
      (string/replace "\n" "\\n")
      (string/replace "\r" "\\r"))
    "\""))

# ── Constructor ───────────────────────────────────────────────────

(defn make-webview (win-handle props)
  "Create a WebKit WebView with JS→Elle IPC via user content manager."
  (let* [ptr (b:webkit-web-view-new)
         id  props:id
         ucm (b:webkit-web-view-get-user-content-manager ptr)]
    (b:webkit-ucm-register-script-message-handler ucm "elle" nil)
    (let [cb (ffi/callback sig-void-ptr-ptr-ptr
                (fn (manager js-value data)
                  (let [cstr (b:jsc-value-to-string js-value)]
                    (unless (w:null? cstr)
                      (w:emit win-handle
                        {:type :webview :id id
                         :value (ffi/string cstr)})))))]
      (w:connect win-handle ucm "script-message-received::elle"
                 sig-void-ptr-ptr-ptr cb))
    (when props:height (b:gtk-widget-set-size-request ptr -1 props:height))
    (when props:width  (b:gtk-widget-set-size-request ptr props:width -1))
    (b:gtk-widget-set-hexpand ptr 1)
    (b:gtk-widget-set-vexpand ptr 1)
    (w:apply-common-props ptr props)
    (when props:html (b:webkit-web-view-load-html ptr props:html nil))
    (when props:url  (b:webkit-web-view-load-uri ptr props:url))
    (w:register-widget win-handle id ptr :webview)
    ptr))

# ── Operations ────────────────────────────────────────────────────

(defn webview-eval (win-handle id js)
  "Evaluate JavaScript in a webview. Fire-and-forget."
  (when-let [{:ptr ptr} (win-handle:widgets id)]
    (b:webkit-web-view-evaluate-javascript ptr js -1 nil nil nil nil)))

(defn webview-send (win-handle id msg)
  "Send a string message to webview JS via window.elle.onMessage callback."
  (-> (string "if(window.elle&&window.elle.onMessage)window.elle.onMessage("
              (json-escape msg) ")")
    (webview-eval win-handle id)))

(defn webview-load-html (win-handle id html)
  "Load HTML string into webview."
  (when-let [{:ptr ptr} (win-handle:widgets id)]
    (b:webkit-web-view-load-html ptr html nil)))

(defn webview-load-url (win-handle id url)
  "Navigate webview to URL."
  (when-let [{:ptr ptr} (win-handle:widgets id)]
    (b:webkit-web-view-load-uri ptr url)))

# ── Export ────────────────────────────────────────────────────────

{:make-webview make-webview
 :eval webview-eval
 :send webview-send
 :load-html webview-load-html
 :load-url webview-load-url}

) # end (fn [])
