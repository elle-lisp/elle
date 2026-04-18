(elle/epoch 8)
## lib/gtk4.lisp — GTK4 bindings for Elle via FFI
##
## Pure Elle module. No plugin, no subprocess. Calls GTK4's C API
## directly through ffi/native, ffi/defbind, and ffi/callback.
##
## Usage:
##   (def gtk ((import "std/gtk4")))
##   (def win (gtk:open {:title "App" :width 800 :height 600}))
##   (gtk:build win [:v-box {:spacing 8}
##     [:button {:id :ok} "OK"]])
##   (while (gtk:open? win)
##     (each event in (gtk:poll win)
##       (match event:type
##         (:click (println "clicked" event:id))
##         (_      nil))))
##   (gtk:close win)

(fn []

(def b  ((import "std/gtk4/bind")))
(def w  ((import "std/gtk4/widgets")))
(def wv ((import "std/gtk4/webview")))

# ── Callback signatures ──────────────────────────────────────────

(def sig-close (ffi/signature :int [:ptr]))

# ── Window lifecycle ──────────────────────────────────────────────

(defn gtk4/open (opts)
  "Initialize GTK and create a window. Returns a mutable window handle."
  (b:gtk-init)
  (let* [win-ptr (b:gtk-window-new)
         handle @{:window win-ptr
                   :widgets @{}
                   :events @[]
                   :close-requested false
                   :callbacks @[]
                   :css-provider nil}]
    (b:gtk-window-set-title win-ptr (or opts:title "Elle"))
    (when (and opts:width opts:height)
      (b:gtk-window-set-default-size win-ptr opts:width opts:height))
    (let [cb (ffi/callback sig-close
                (fn (widget)
                  (put handle :close-requested true)
                  1))]
      (push handle:callbacks cb)
      (b:g-signal-connect-data win-ptr "close-request" cb nil nil 0))
    (b:gtk-window-present win-ptr)
    handle))

(defn gtk4/open? (handle)
  "Check if the window is still open."
  (not handle:close-requested))

(defn gtk4/close (handle)
  "Destroy the window."
  (b:gtk-window-destroy handle:window)
  (put handle :close-requested true)
  nil)

(defn gtk4/poll (handle)
  "Pump the GLib main loop, drain and return events."
  (let [ctx (b:g-main-context-default)]
    (while (nonzero? (b:g-main-context-pending ctx))
      (b:g-main-context-iteration ctx 0)))
  (let [events (freeze handle:events)]
    (while (nonempty? handle:events)
      (pop handle:events))
    events))

# ── Tree builder ──────────────────────────────────────────────────

(defn parse-spec (spec)
  "Parse [:tag {props} children...] into [tag props child-specs text]."
  (let* [tag      (spec 0)
         rest     (slice spec 1)
         has-props (and (nonempty? rest) (struct? (rest 0)))
         props    (if has-props (rest 0) {})
         children (if has-props (slice rest 1) rest)
         text     (find string? children)
         specs    (filter (complement string?) children)]
    [tag props specs text]))

(defn build-widget (handle spec)
  "Recursively build a widget from a spec. Returns the GTK widget pointer."
  (let* [[tag props children text] (parse-spec spec)
         widget (match tag
           # display
           (:label        (w:make-label handle props text))
           (:heading      (w:make-heading handle props text))
           (:image        (w:make-image handle props))
           (:progress-bar (w:make-progress-bar handle props))
           (:separator    (w:make-separator handle props))
           (:spacer       (w:make-spacer handle props))
           (:spinner      (w:make-spinner handle props))
           # input
           (:button        (w:make-button handle props text))
           (:toggle-button (w:make-toggle-button handle props text))
           (:text-input    (w:make-text-input handle props))
           (:text-edit     (w:make-text-edit handle props))
           (:checkbox      (w:make-checkbox handle props text))
           (:switch        (w:make-switch handle props))
           (:slider        (w:make-slider handle props))
           (:spin-button   (w:make-spin-button handle props))
           (:combo-box     (w:make-combo-box handle props children))
           (:search-entry  (w:make-search-entry handle props))
           (:calendar      (w:make-calendar handle props))
           # layout
           (:v-box       (w:make-box handle props b:GTK_ORIENTATION_VERTICAL))
           (:h-box       (w:make-box handle props b:GTK_ORIENTATION_HORIZONTAL))
           (:scroll-area (w:make-scroll-area handle props))
           (:expander    (w:make-expander handle props text))
           (:frame       (w:make-frame handle props text))
           (:grid        (w:make-grid handle props))
           (:stack       (w:make-stack handle props))
           (:notebook    (w:make-notebook handle props))
           (:paned       (w:make-paned handle props b:GTK_ORIENTATION_HORIZONTAL))
           (:center-box  (w:make-center-box handle props))
           (:overlay     (w:make-overlay handle props))
           (:revealer    (w:make-revealer handle props))
           # special
           (:webview     (wv:make-webview handle props))
           (_            (error (string "gtk4:build: unknown widget type " tag))))]
    (add-children handle tag widget props children)
    widget))

(defn add-children (handle tag widget props children)
  "Attach children to a container widget."
  (match tag
    ((or :v-box :h-box)
      (each child in children
        (w:box-append widget (build-widget handle child))))
    (:scroll-area
      (when (nonempty? children)
        (w:scroll-set-child widget (build-widget handle (children 0)))))
    (:expander
      (when (nonempty? children)
        (w:expander-set-child widget (build-widget handle (children 0)))))
    (:frame
      (when (nonempty? children)
        (w:frame-set-child widget (build-widget handle (children 0)))))
    (:revealer
      (when (nonempty? children)
        (w:revealer-set-child widget (build-widget handle (children 0)))))
    (:grid
      (add-grid-children handle widget props children))
    (:stack
      (each child in children (add-stack-child handle widget child)))
    (:notebook
      (each child in children (add-notebook-child handle widget child)))
    (:paned
      (begin
        (when (nonempty? children)
          (b:gtk-paned-set-start-child widget (build-widget handle (children 0))))
        (when (> (length children) 1)
          (b:gtk-paned-set-end-child widget (build-widget handle (children 1))))))
    (:overlay
      (begin
        (when (nonempty? children)
          (b:gtk-overlay-set-child widget (build-widget handle (children 0))))
        (each i in (range 1 (length children))
          (b:gtk-overlay-add-overlay widget (build-widget handle (children i))))))
    (:center-box
      (begin
        (when (nonempty? children)
          (b:gtk-center-box-set-start-widget widget (build-widget handle (children 0))))
        (when (> (length children) 1)
          (b:gtk-center-box-set-center-widget widget (build-widget handle (children 1))))
        (when (> (length children) 2)
          (b:gtk-center-box-set-end-widget widget (build-widget handle (children 2))))))
    (_ nil)))

(defn add-grid-children (handle grid props children)
  "Attach children to a grid with auto-flow."
  (let [cols (or props:columns 1)]
    (def @auto-col 0)
    (def @auto-row 0)
    (each child in children
      (let* [cw    (build-widget handle child)
             cp    (if (and (> (length child) 1) (struct? (child 1))) (child 1) {})
             col   (or cp:col auto-col)
             row   (or cp:row auto-row)
             cs    (or cp:col-span 1)
             rs    (or cp:row-span 1)]
        (w:grid-attach grid cw col row cs rs)
        (assign auto-col (+ col cs))
        (when (>= auto-col cols)
          (assign auto-col 0)
          (assign auto-row (+ auto-row 1)))))))

(defn add-stack-child (handle stack child)
  "Add a child to a GtkStack."
  (let* [[_ props children text] (parse-spec child)
         name    (or (string props:name) (string props:id))
         title   (or props:title name)
         content (if (nonempty? children)
                    (build-widget handle (children 0))
                    (b:gtk-label-new (or text "")))]
    (b:gtk-stack-add-titled stack content name title)))

(defn add-notebook-child (handle notebook child)
  "Add a tab to a GtkNotebook."
  (let* [[_ props children text] (parse-spec child)
         title   (or props:title text "Tab")
         label   (b:gtk-label-new title)
         content (if (nonempty? children)
                    (build-widget handle (children 0))
                    (b:gtk-label-new ""))]
    (b:gtk-notebook-append-page notebook content label)))

# ── Public API ────────────────────────────────────────────────────

(defn gtk4/build (handle spec)
  "Build a widget tree from a declarative spec and set as window child."
  (b:gtk-window-set-child handle:window (build-widget handle spec))
  handle)

(defn gtk4/rebuild (handle id spec)
  "Replace a container's children by rebuilding from spec."
  (when-let [{:ptr ptr :type type} (handle:widgets id)]
    (let [child (build-widget handle spec)]
      (match type
        ((or :box :scroll-area :frame :expander :revealer)
          (match type
            (:box         (b:gtk-box-append ptr child))
            (:scroll-area (b:gtk-scrolled-window-set-child ptr child))
            (:frame       (b:gtk-frame-set-child ptr child))
            (:expander    (b:gtk-expander-set-child ptr child))
            (:revealer    (b:gtk-revealer-set-child ptr child))
            (_            nil)))
        (_ nil)))))

# ── Widget access ─────────────────────────────────────────────────

(defn gtk4/set (handle id value)
  "Set a widget's text or value by id."
  (when-let [{:ptr ptr :type type} (handle:widgets id)]
    (match type
      ((or :label :heading) (b:gtk-label-set-text ptr (string value)))
      (:button        (b:gtk-button-set-label ptr (string value)))
      (:text-input    (b:gtk-editable-set-text ptr (string value)))
      (:text-edit     (-> (b:gtk-text-view-get-buffer ptr)
                       (b:gtk-text-buffer-set-text (string value) -1)))
      (:checkbox      (b:gtk-check-button-set-active ptr (w:bool->int value)))
      (:switch        (b:gtk-switch-set-active ptr (w:bool->int value)))
      (:toggle-button (b:gtk-toggle-button-set-active ptr (w:bool->int value)))
      (:slider        (b:gtk-range-set-value ptr value))
      (:spin-button   (b:gtk-spin-button-set-value ptr value))
      (:combo-box     (b:gtk-drop-down-set-selected ptr value))
      (:progress-bar  (b:gtk-progress-bar-set-fraction ptr value))
      (:spinner       (if value (b:gtk-spinner-start ptr) (b:gtk-spinner-stop ptr)))
      (_ nil))))

(defn gtk4/get (handle id)
  "Get a widget's current text or value by id."
  (when-let [{:ptr ptr :type type} (handle:widgets id)]
    (match type
      ((or :label :heading) (ffi/string (b:gtk-label-get-text ptr)))
      (:button        (ffi/string (b:gtk-button-get-label ptr)))
      (:text-input    (ffi/string (b:gtk-editable-get-text ptr)))
      (:text-edit     (let [buf (b:gtk-text-view-get-buffer ptr)]
                        (ffi/with-stack [[start 80] [end 80]]
                          (b:gtk-text-buffer-get-start-iter buf start)
                          (b:gtk-text-buffer-get-end-iter buf end)
                          (ffi/string (b:gtk-text-buffer-get-text buf start end 0)))))
      (:checkbox      (nonzero? (b:gtk-check-button-get-active ptr)))
      (:switch        (nonzero? (b:gtk-switch-get-active ptr)))
      (:toggle-button (nonzero? (b:gtk-toggle-button-get-active ptr)))
      (:slider        (b:gtk-range-get-value ptr))
      (:spin-button   (b:gtk-spin-button-get-value ptr))
      (:combo-box     (b:gtk-drop-down-get-selected ptr))
      (:progress-bar  (b:gtk-progress-bar-get-fraction ptr))
      (_ nil))))

(defn gtk4/set-visible (handle id visible)
  "Show or hide a widget."
  (when-let [{:ptr ptr} (handle:widgets id)]
    (b:gtk-widget-set-visible ptr (w:bool->int visible))))

(defn gtk4/set-sensitive (handle id sensitive)
  "Enable or disable a widget."
  (when-let [{:ptr ptr} (handle:widgets id)]
    (b:gtk-widget-set-sensitive ptr (w:bool->int sensitive))))

(defn gtk4/set-title (handle title)
  "Change the window title."
  (b:gtk-window-set-title handle:window title))

(defn gtk4/load-css (handle css)
  "Load a CSS string for the whole application."
  (let [provider (or handle:css-provider
                      (let [p (b:gtk-css-provider-new)]
                        (b:gtk-style-context-add-provider-for-display
                          (b:gdk-display-get-default) p
                          b:GTK_STYLE_PROVIDER_PRIORITY_APPLICATION)
                        (put handle :css-provider p)
                        p))]
    (b:gtk-css-provider-load-from-string provider css)))

(defn gtk4/set-stack-page (handle id page-name)
  "Switch the visible page of a GtkStack."
  (when-let [{:ptr ptr} (handle:widgets id)]
    (b:gtk-stack-set-visible-child-name ptr (string page-name))))

# ── WebView passthrough ───────────────────────────────────────────

(defn gtk4/eval (handle id js)      (wv:eval handle id js))
(defn gtk4/send (handle id msg)     (wv:send handle id msg))
(defn gtk4/load-html (handle id h)  (wv:load-html handle id h))
(defn gtk4/load-url (handle id url) (wv:load-url handle id url))

# ── Add / Remove ──────────────────────────────────────────────────

(defn gtk4/add (handle parent-id spec)
  "Add a widget to a container."
  (when-let [{:ptr ptr :type type} (handle:widgets parent-id)]
    (let [child (build-widget handle spec)]
      (match type
        (:box         (b:gtk-box-append ptr child))
        (:scroll-area (b:gtk-scrolled-window-set-child ptr child))
        (_ nil)))))

(defn gtk4/remove (handle id)
  "Remove a widget from its parent."
  (when-let [{:ptr ptr} (handle:widgets id)]
    (b:gtk-widget-unparent ptr)
    (put handle:widgets id nil)))

# ── Export ────────────────────────────────────────────────────────

{:open       gtk4/open
 :close      gtk4/close
 :open?      gtk4/open?
 :poll       gtk4/poll
 :build      gtk4/build
 :rebuild    gtk4/rebuild
 :set        gtk4/set
 :get        gtk4/get
 :set-visible   gtk4/set-visible
 :set-sensitive gtk4/set-sensitive
 :set-title     gtk4/set-title
 :load-css      gtk4/load-css
 :set-stack-page gtk4/set-stack-page
 :add        gtk4/add
 :remove     gtk4/remove
 :eval       gtk4/eval
 :send       gtk4/send
 :load-html  gtk4/load-html
 :load-url   gtk4/load-url}

) # end (fn [])
