## lib/gtk4/widgets.lisp — Per-widget constructors and signal wiring
##
## Each make-* function creates a GTK widget, applies props, connects
## signals, and returns the widget pointer. The win-handle is threaded
## through for event queue access and callback storage.

(fn []

(def b ((import "std/gtk4/bind")))

# ── Signal signatures ─────────────────────────────────────────────

(def sig-clicked   (ffi/signature :void [:ptr :ptr]))
(def sig-state-set (ffi/signature :int  [:ptr :int :ptr]))

# ── Helpers ───────────────────────────────────────────────────────

(defn null? (ptr) (zero? (ptr/to-int ptr)))
(defn bool->int (v) (if v 1 0))

(defn connect (win-handle ptr signal cb)
  "Connect a GObject signal keeping the callback alive."
  (push win-handle:callbacks cb)
  (b:g-signal-connect-data ptr signal cb nil nil 0))

(defn emit (win-handle event)
  "Push an event to the window's event queue."
  (push win-handle:events event))

(defn apply-css-class (ptr props)
  "Apply :css-class prop (string or array of strings)."
  (when props:css-class
    (if (string? props:css-class)
      (b:gtk-widget-add-css-class ptr props:css-class)
      (each c in props:css-class
        (b:gtk-widget-add-css-class ptr c)))))

(defn apply-common-props (ptr props)
  "Apply universal widget props: css-class, expand, margin, align."
  (apply-css-class ptr props)
  (when props:hexpand (b:gtk-widget-set-hexpand ptr 1))
  (when props:vexpand (b:gtk-widget-set-vexpand ptr 1))
  (when props:margin
    (let [[m props:margin]]
      (b:gtk-widget-set-margin-start ptr m)
      (b:gtk-widget-set-margin-end ptr m)
      (b:gtk-widget-set-margin-top ptr m)
      (b:gtk-widget-set-margin-bottom ptr m)))
  (when props:width  (b:gtk-widget-set-size-request ptr props:width -1))
  (when props:height (b:gtk-widget-set-size-request ptr -1 props:height)))

(defn register-widget (win-handle id ptr type)
  "Store widget in the registry by id."
  (when id
    (put win-handle:widgets id @{:ptr ptr :type type})))

(defn make-widget (win-handle props ptr type)
  "Apply common props, register, return ptr."
  (apply-common-props ptr props)
  (register-widget win-handle props:id ptr type)
  ptr)

# ── Display widgets ───────────────────────────────────────────────

(defn make-label (win-handle props text)
  (let [[ptr (b:gtk-label-new (or text ""))]]
    (when props:wrap (b:gtk-label-set-wrap ptr 1))
    (make-widget win-handle props ptr :label)))

(defn make-heading (win-handle props text)
  (let [[ptr (b:gtk-label-new (or text ""))]]
    (b:gtk-widget-add-css-class ptr "title-2")
    (make-widget win-handle props ptr :heading)))

(defn make-image (win-handle props)
  (let [[ptr (if props:icon
               (b:gtk-image-new-from-icon-name props:icon)
               (b:gtk-image-new-from-file (or props:file "")))]]
    (when props:size (b:gtk-image-set-pixel-size ptr props:size))
    (make-widget win-handle props ptr :image)))

(defn make-progress-bar (win-handle props)
  (let [[ptr (b:gtk-progress-bar-new)]]
    (when props:value (b:gtk-progress-bar-set-fraction ptr props:value))
    (make-widget win-handle props ptr :progress-bar)))

(defn make-separator (win-handle props)
  (make-widget win-handle props
    (b:gtk-separator-new b:GTK_ORIENTATION_HORIZONTAL) :separator))

(defn make-spacer (win-handle props)
  (let [[ptr (b:gtk-label-new "")]]
    (b:gtk-widget-set-hexpand ptr 1)
    (b:gtk-widget-set-vexpand ptr 1)
    (make-widget win-handle props ptr :spacer)))

(defn make-spinner (win-handle props)
  (let [[ptr (b:gtk-spinner-new)]]
    (when props:active (b:gtk-spinner-start ptr))
    (make-widget win-handle props ptr :spinner)))

# ── Input widgets ─────────────────────────────────────────────────

(defn on-clicked (win-handle ptr id)
  "Wire a 'clicked' signal that emits {:type :click :id id}."
  (let [[cb (ffi/callback sig-clicked
              (fn (widget data) (emit win-handle {:type :click :id id})))]]
    (connect win-handle ptr "clicked" cb)))

(defn on-toggled (win-handle ptr id type getter)
  "Wire a 'toggled' signal that emits {:type type :id id :active bool}."
  (let [[cb (ffi/callback sig-clicked
              (fn (widget data)
                (emit win-handle
                  {:type type :id id :active (nonzero? (getter ptr))})))]]
    (connect win-handle ptr "toggled" cb)))

(defn on-changed (win-handle ptr id type getter)
  "Wire a change signal that emits {:type type :id id :value string}."
  (let [[cb (ffi/callback sig-clicked
              (fn (widget data)
                (emit win-handle
                  {:type type :id id :value (getter ptr)})))]]
    (connect win-handle ptr "changed" cb)))

(defn on-value-changed (win-handle ptr id type getter)
  "Wire a 'value-changed' signal."
  (let [[cb (ffi/callback sig-clicked
              (fn (widget data)
                (emit win-handle
                  {:type type :id id :value (getter ptr)})))]]
    (connect win-handle ptr "value-changed" cb)))

(defn make-button (win-handle props text)
  (let [[ptr (b:gtk-button-new-with-label (or text ""))]]
    (on-clicked win-handle ptr props:id)
    (make-widget win-handle props ptr :button)))

(defn make-toggle-button (win-handle props text)
  (let [[ptr (if text
               (b:gtk-toggle-button-new-with-label text)
               (b:gtk-toggle-button-new))]]
    (when props:active (b:gtk-toggle-button-set-active ptr 1))
    (on-toggled win-handle ptr props:id :toggle b:gtk-toggle-button-get-active)
    (make-widget win-handle props ptr :toggle-button)))

(defn make-text-input (win-handle props)
  (let [[ptr (b:gtk-entry-new)]]
    (when props:hint  (b:gtk-entry-set-placeholder-text ptr props:hint))
    (when props:value (b:gtk-editable-set-text ptr props:value))
    (on-changed win-handle ptr props:id :text
      (fn (p) (ffi/string (b:gtk-editable-get-text p))))
    (make-widget win-handle props ptr :text-input)))

(defn make-text-edit (win-handle props)
  (let [[ptr (b:gtk-text-view-new)]]
    (b:gtk-text-view-set-wrap-mode ptr b:GTK_WRAP_WORD_CHAR)
    (when props:rows
      (b:gtk-widget-set-size-request ptr -1 (* props:rows 20)))
    (when (not (nil? props:editable))
      (b:gtk-text-view-set-editable ptr (bool->int props:editable)))
    (make-widget win-handle props ptr :text-edit)))

(defn make-checkbox (win-handle props text)
  (let [[ptr (b:gtk-check-button-new-with-label (or text ""))]]
    (when props:active (b:gtk-check-button-set-active ptr 1))
    (on-toggled win-handle ptr props:id :check b:gtk-check-button-get-active)
    (make-widget win-handle props ptr :checkbox)))

(defn make-switch (win-handle props)
  (let [[ptr (b:gtk-switch-new)]
        [id  props:id]]
    (when props:active (b:gtk-switch-set-active ptr 1))
    (let [[cb (ffi/callback sig-state-set
                (fn (widget state data)
                  (emit win-handle
                    {:type :switch :id id :active (nonzero? state)})
                  0))]]
      (connect win-handle ptr "state-set" cb))
    (make-widget win-handle props ptr :switch)))

(defn make-slider (win-handle props)
  (let* [[mn  (or props:min 0.0)]
         [mx  (or props:max 100.0)]
         [step (or props:step 1.0)]
         [ptr (b:gtk-scale-new-with-range b:GTK_ORIENTATION_HORIZONTAL mn mx step)]]
    (when props:value (b:gtk-range-set-value ptr props:value))
    (on-value-changed win-handle ptr props:id :slider b:gtk-range-get-value)
    (make-widget win-handle props ptr :slider)))

(defn make-spin-button (win-handle props)
  (let* [[mn  (or props:min 0.0)]
         [mx  (or props:max 100.0)]
         [step (or props:step 1.0)]
         [ptr (b:gtk-spin-button-new-with-range mn mx step)]]
    (when props:value (b:gtk-spin-button-set-value ptr props:value))
    (on-value-changed win-handle ptr props:id :spin b:gtk-spin-button-get-value)
    (make-widget win-handle props ptr :spin-button)))

(defn make-combo-box (win-handle props items)
  (let* [[id    props:id]
         [count (length items)]
         # build null-terminated string array for gtk_string_list_new
         [ptrs  (ffi/malloc (* (+ count 1) (ffi/size :ptr)))]
         [_     (each i in (range count)
                  (let [[s (ffi/pin (bytes (string (items i) "\0")))]]
                    (ffi/write (ptr/add ptrs (* i (ffi/size :ptr))) :ptr s)))]
         [_     (ffi/write (ptr/add ptrs (* count (ffi/size :ptr))) :ptr nil)]
         [model (b:gtk-string-list-new ptrs)]
         [ptr   (b:gtk-drop-down-new model nil)]]
    (ffi/free ptrs)
    (let [[cb (ffi/callback sig-clicked
                (fn (widget data)
                  (let [[idx (b:gtk-drop-down-get-selected ptr)]]
                    (emit win-handle
                      {:type :combo :id id
                       :value (if (< idx count) (items idx) nil)}))))]]
      (connect win-handle ptr "notify::selected" cb))
    (make-widget win-handle props ptr :combo-box)))

(defn make-search-entry (win-handle props)
  (let [[ptr (b:gtk-search-entry-new)]]
    (let [[cb (ffi/callback sig-clicked
                (fn (widget data)
                  (emit win-handle
                    {:type :search :id props:id
                     :value (ffi/string (b:gtk-editable-get-text ptr))})))]]
      (connect win-handle ptr "search-changed" cb))
    (make-widget win-handle props ptr :search-entry)))

(defn make-calendar (win-handle props)
  (let [[ptr (b:gtk-calendar-new)]]
    (let [[cb (ffi/callback sig-clicked
                (fn (widget data)
                  (emit win-handle {:type :calendar :id props:id})))]]
      (connect win-handle ptr "day-selected" cb))
    (make-widget win-handle props ptr :calendar)))

# ── Layout widgets ────────────────────────────────────────────────

(defn make-box (win-handle props orientation)
  (let [[ptr (b:gtk-box-new orientation (or props:spacing 0))]]
    (make-widget win-handle props ptr :box)))

(defn make-scroll-area (win-handle props)
  (let [[ptr (b:gtk-scrolled-window-new)]]
    (when props:height (b:gtk-scrolled-window-set-min-content-height ptr props:height))
    (when props:width  (b:gtk-scrolled-window-set-min-content-width ptr props:width))
    (make-widget win-handle props ptr :scroll-area)))

(defn make-expander (win-handle props text)
  (let [[ptr (b:gtk-expander-new (or text ""))]]
    (when props:expanded (b:gtk-expander-set-expanded ptr 1))
    (make-widget win-handle props ptr :expander)))

(defn make-frame (win-handle props text)
  (make-widget win-handle props (b:gtk-frame-new text) :frame))

(defn make-grid (win-handle props)
  (let [[ptr (b:gtk-grid-new)]]
    (when props:row-spacing (b:gtk-grid-set-row-spacing ptr props:row-spacing))
    (when props:col-spacing (b:gtk-grid-set-column-spacing ptr props:col-spacing))
    (make-widget win-handle props ptr :grid)))

(defn make-stack (win-handle props)
  (let [[ptr (b:gtk-stack-new)]
        [id  props:id]]
    (when id
      (let [[cb (ffi/callback sig-clicked
                  (fn (widget data)
                    (let [[name (b:gtk-stack-get-visible-child-name ptr)]]
                      (unless (null? name)
                        (emit win-handle
                          {:type :stack-changed :id id
                           :value (ffi/string name)})))))]]
        (connect win-handle ptr "notify::visible-child-name" cb)))
    (make-widget win-handle props ptr :stack)))

(defn make-notebook (win-handle props)
  (let [[ptr (b:gtk-notebook-new)]
        [id  props:id]]
    (when id
      (let [[cb (ffi/callback sig-clicked
                  (fn (widget data)
                    (emit win-handle
                      {:type :tab-changed :id id
                       :value (b:gtk-notebook-get-current-page ptr)})))]]
        (connect win-handle ptr "switch-page" cb)))
    (make-widget win-handle props ptr :notebook)))

(defn make-paned (win-handle props orientation)
  (make-widget win-handle props (b:gtk-paned-new orientation) :paned))

(defn make-center-box (win-handle props)
  (make-widget win-handle props (b:gtk-center-box-new) :center-box))

(defn make-overlay (win-handle props)
  (make-widget win-handle props (b:gtk-overlay-new) :overlay))

(defn make-revealer (win-handle props)
  (let [[ptr (b:gtk-revealer-new)]]
    (when props:revealed (b:gtk-revealer-set-reveal-child ptr 1))
    (make-widget win-handle props ptr :revealer)))

# ── Container child helpers (called by tree builder) ──────────────

(defn box-append (parent child) (b:gtk-box-append parent child))
(defn scroll-set-child (p c)   (b:gtk-scrolled-window-set-child p c))
(defn expander-set-child (p c) (b:gtk-expander-set-child p c))
(defn frame-set-child (p c)    (b:gtk-frame-set-child p c))
(defn revealer-set-child (p c) (b:gtk-revealer-set-child p c))
(defn grid-attach (p c col row cs rs) (b:gtk-grid-attach p c col row cs rs))

# ── Export ────────────────────────────────────────────────────────

{:null? null? :bool->int bool->int
 :connect connect :emit emit
 :apply-common-props apply-common-props
 :register-widget register-widget :make-widget make-widget
 # display
 :make-label make-label :make-heading make-heading
 :make-image make-image :make-progress-bar make-progress-bar
 :make-separator make-separator :make-spacer make-spacer
 :make-spinner make-spinner
 # input
 :make-button make-button :make-toggle-button make-toggle-button
 :make-text-input make-text-input :make-text-edit make-text-edit
 :make-checkbox make-checkbox :make-switch make-switch
 :make-slider make-slider :make-spin-button make-spin-button
 :make-combo-box make-combo-box :make-search-entry make-search-entry
 :make-calendar make-calendar
 # layout
 :make-box make-box :make-scroll-area make-scroll-area
 :make-expander make-expander :make-frame make-frame
 :make-grid make-grid :make-stack make-stack
 :make-notebook make-notebook :make-paned make-paned
 :make-center-box make-center-box :make-overlay make-overlay
 :make-revealer make-revealer
 # child ops
 :box-append box-append :scroll-set-child scroll-set-child
 :expander-set-child expander-set-child :frame-set-child frame-set-child
 :revealer-set-child revealer-set-child :grid-attach grid-attach}

) # end (fn [])
