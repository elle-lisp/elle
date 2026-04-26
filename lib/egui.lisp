(elle/epoch 9)
## lib/egui.lisp — immediate-mode GUI for Elle
##
## Wraps the elle-egui plugin (egui + winit + glow) with fiber-friendly
## event waiting and widget constructors. All I/O awareness lives here;
## the plugin is a thin wrapper with zero async knowledge.
##
## Dependencies:
##   - elle-egui plugin loaded via (import "plugin/egui")
##   - ev/poll-fd from stdlib (fiber scheduler)
##
## Usage:
##   (def egui-plugin (import "plugin/egui"))
##   (def ui ((import "std/egui") egui-plugin))
##
##   (var count 0)
##   (def win (ui:open :title "Counter"))
##   (ui:run win (fn [ix]
##     (when (ui:clicked? ix :inc) (assign count (inc count)))
##     (when (ui:clicked? ix :dec) (assign count (dec count)))
##     (ui:v-layout
##       (ui:heading (string "Count: " count))
##       (ui:h-layout
##         (ui:button :dec "-")
##         (ui:button :inc "+")))))

## ── Entry-point thunk ─────────────────────────────────────────────────

(fn [plugin]

  ## ── Lifecycle ─────────────────────────────────────────────────────

  (defn open [&named @title @width @height]
    "Open a GUI window. Returns a handle.
     Options: :title string, :width int, :height int."
    (default title "Elle")
    (default width 800)
    (default height 600)
    (plugin:open {:title title :width width :height height}))

  (defn close [handle]
    "Close the window and release resources."
    (plugin:close handle))

  (defn open? [handle]
    "Check if the window is still open."
    (plugin:open? handle))

  ## ── Wait + frame ──────────────────────────────────────────────────

  (defn wait-event [handle &named timeout]
    "Block until display events. On Linux, yields via io_uring poll on
     the display fd. On macOS (no display fd), yields for 16ms."
    (def fd (plugin:display-fd handle))
    (if (nil? fd)
      (ev/sleep (or timeout 0.016))
      (if (nil? timeout)
        (ev/poll-fd fd :read)
        (ev/poll-fd fd :read timeout))))

  (defn frame [handle tree]
    "Render one frame. Synchronous — pumps events, renders, returns interactions."
    (plugin:frame handle tree))

  ## ── Main loop ─────────────────────────────────────────────────────

  (defn empty-ix []
    "Empty interactions struct for the first render-fn call."
    {:clicks || :text {} :checks {} :sliders {}
     :combos {} :collapsed {} :closed false :size [0 0]})

  (defn run [handle render-fn]
    "Immediate-mode GUI loop. render-fn: (fn [interactions] tree).
     Renders first frame immediately (Wayland needs a buffer commit
     before the window is visible), then waits for events between frames."
    (def @ix (frame handle (render-fn (empty-ix))))
    (forever
      (when ix:closed (break))
      (unless (open? handle) (break))
      (wait-event handle)
      (assign ix (frame handle (render-fn ix)))))

  ## ── Display widgets ───────────────────────────────────────────────

  (defn label [text]
    "Static text label."
    [:label text])

  (defn heading [text]
    "Large heading text."
    [:heading text])

  (defn progress-bar [fraction &named text]
    "Progress bar (0.0 to 1.0)."
    [:progress-bar {:fraction fraction :text text}])

  (defn separator []
    "Horizontal line separator."
    [:separator])

  (defn spacer [&named @size]
    "Vertical space."
    (default size 8.0)
    [:spacer {:size size}])

  ## ── Input widgets ─────────────────────────────────────────────────

  (defn button [id text]
    "Clickable button. Check (clicked? ix id) for presses."
    [:button {:id id} text])

  (defn text-input [id &named hint]
    "Single-line text input."
    [:text-input {:id id :hint hint}])

  (defn text-edit [id &named @rows]
    "Multi-line text editor."
    (default rows 4)
    [:text-edit {:id id :rows rows}])

  (defn checkbox [id text]
    "Boolean toggle checkbox."
    [:checkbox {:id id} text])

  (defn slider [id &named @min @max]
    "Numeric slider."
    (default min 0)
    (default max 100)
    [:slider {:id id :min min :max max}])

  (defn combo-box [id options]
    "Dropdown selection. options: [\"a\" \"b\" \"c\"]."
    [:combo-box {:id id} options])

  ## ── Layout ────────────────────────────────────────────────────────

  (defn v-layout [& children]
    "Vertical layout."
    (concat [:v-layout] (apply array children)))

  (defn h-layout [& children]
    "Horizontal layout."
    (concat [:h-layout] (apply array children)))

  (defn centered [& children]
    "Center children horizontally."
    (concat [:centered] (apply array children)))

  (defn centered-justified [& children]
    "Center children horizontally, justify width to fill."
    (concat [:centered-justified] (apply array children)))

  (defn scroll-area [id & children]
    "Scrollable container."
    (concat [:scroll-area {:id id}] (apply array children)))

  (defn collapsing [id title & children]
    "Expandable/collapsible section."
    (concat [:collapsing {:id id} title] (apply array children)))

  (defn group [& children]
    "Visually grouped container (frame/border)."
    (concat [:group] (apply array children)))

  (defn grid [id columns & children]
    "Grid layout with N columns."
    (concat [:grid {:id id :columns columns}] (apply array children)))

  ## ── Compound widgets ──────────────────────────────────────────────

  (defn labeled [text widget]
    "Label + widget in a horizontal row."
    (h-layout (label text) widget))

  ## ── Interaction readers ───────────────────────────────────────────

  (defn clicked? [ix id]
    "Was button id clicked this frame?"
    (contains? ix:clicks id))

  (defn text-val [ix id]
    "Current text input value for id."
    (ix:text id))

  (defn check-val [ix id]
    "Current checkbox state for id."
    (ix:checks id))

  (defn slider-val [ix id]
    "Current slider value for id."
    (ix:sliders id))

  (defn combo-val [ix id]
    "Current combo box selection for id."
    (ix:combos id))

  (defn collapsed? [ix id]
    "Is collapsing section id collapsed?"
    (ix:collapsed id))

  (defn window-size [ix]
    "Current window size as [width height]."
    ix:size)

  ## ── State setters ─────────────────────────────────────────────────

  (defn set-text [handle id val]
    "Programmatically set text input value."
    (plugin:set-text handle id val))

  (defn set-check [handle id val]
    "Programmatically set checkbox state."
    (plugin:set-check handle id val))

  (defn set-slider [handle id val]
    "Programmatically set slider value."
    (plugin:set-slider handle id val))

  (defn set-combo [handle id val]
    "Programmatically set combo box selection."
    (plugin:set-combo handle id val))

  (defn set-title [handle title]
    "Change the window title."
    (plugin:set-title handle title))

  ## ── Export struct ──────────────────────────────────────────────────

  {# lifecycle
   :open open :close close :open? open?
   :frame frame :wait-event wait-event :run run
   # display widgets
   :label label :heading heading :progress-bar progress-bar
   :separator separator :spacer spacer
   # input widgets
   :button button :text-input text-input :text-edit text-edit
   :checkbox checkbox :slider slider :combo-box combo-box
   # layout
   :v-layout v-layout :h-layout h-layout
   :centered centered :centered-justified centered-justified
   :scroll-area scroll-area
   :collapsing collapsing :group group :grid grid
   # compound
   :labeled labeled
   # interaction readers
   :clicked? clicked? :text-val text-val :check-val check-val
   :slider-val slider-val :combo-val combo-val
   :collapsed? collapsed? :window-size window-size
   # state setters
   :set-text set-text :set-check set-check :set-slider set-slider
   :set-combo set-combo :set-title set-title})
