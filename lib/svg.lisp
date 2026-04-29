(elle/epoch 9)
## lib/svg.lisp — SVG construction and emission (pure Elle)
##
## Build SVG documents as struct trees, emit as XML strings.
## Optionally pass the svg plugin for rasterization.
##
## Usage:
##   (def svg ((import "std/svg")))
##   (def doc (svg:svg 400 300
##              (svg:rect 10 20 100 50 {:fill "blue"})
##              (svg:circle 50 50 30 {:fill "red"})))
##   (println (svg:emit doc))
##
## With rendering:
##   (def svgr (import "plugin/svg"))
##   (def svg ((import "std/svg") svgr))
##   (spit "out.png" (svg:render doc))

(fn [& opts]
  (def renderer (if (empty? opts) nil (first opts)))

  # ── Element constructor ────────────────────────────────────────────

  (defn element [tag attrs children]
    {:tag tag :attrs attrs :children (->array children)})

  # ── Attribute merge ────────────────────────────────────────────────

  (defn opt-attrs [opts]
    (if (empty? opts) nil (first opts)))

  (defn merge-attrs [base user]
    (if (nil? user) base (merge base user)))

  # ── Document root ──────────────────────────────────────────────────

  (defn svg [w h & children]
    (element :svg {:xmlns "http://www.w3.org/2000/svg"
                   :width (float w)
                   :height (float h)
                   :viewBox (string "0 0 " w " " h)} children))

  # ── Shape elements ─────────────────────────────────────────────────

  (defn rect [x y w h & opts]
    (element :rect (merge-attrs {:x (float x)
                                 :y (float y)
                                 :width (float w)
                                 :height (float h)} (opt-attrs opts)) []))

  (defn circle [cx cy r & opts]
    (element :circle (merge-attrs {:cx (float cx) :cy (float cy) :r (float r)}
                                  (opt-attrs opts)) []))

  (defn ellipse [cx cy rx ry & opts]
    (element :ellipse (merge-attrs {:cx (float cx)
                                    :cy (float cy)
                                    :rx (float rx)
                                    :ry (float ry)} (opt-attrs opts)) []))

  (defn line [x1 y1 x2 y2 & opts]
    (element :line (merge-attrs {:x1 (float x1)
                                 :y1 (float y1)
                                 :x2 (float x2)
                                 :y2 (float y2)} (opt-attrs opts)) []))

  (defn path [d & opts]
    (element :path (merge-attrs {:d d} (opt-attrs opts)) []))

  # ── Points helpers ─────────────────────────────────────────────────

  (defn points->string [pts]
    (string/join (map (fn [p] (string (p 0) "," (p 1))) pts) " "))

  (defn polyline [pts & opts]
    (element :polyline (merge-attrs {:points (points->string pts)}
                                    (opt-attrs opts)) []))

  (defn polygon [pts & opts]
    (element :polygon (merge-attrs {:points (points->string pts)}
                                   (opt-attrs opts)) []))

  # ── Text ───────────────────────────────────────────────────────────

  (defn text [x y & rest]
    (let [@attrs {:x (float x) :y (float y)}
          children @[]]
      (each item in rest
        (cond  ## Attrs struct (not an SVG element)
          (and (struct? item) (nil? (get item :tag))) (assign
            attrs
            (merge attrs item))  ## String content
          (string? item) (push children item)  ## SVG element child (tspan etc.)
          true (push children item)))
      (element :text attrs (freeze children))))

  (defn tspan [content & opts]
    (element :tspan (or (first opts) {}) [content]))

  # ── Grouping and transforms ───────────────────────────────────────

  (defn group [& args]
    (if (and (not (empty? args)) (struct? (first args))
             (nil? (get (first args) :tag)))
      (element :g (first args) (slice args 1))
      (element :g {} args)))

  (defn translate [dx dy & children]
    (element :g {:transform (string "translate(" dx "," dy ")")} children))

  (defn rotate [deg & children]
    (element :g {:transform (string "rotate(" deg ")")} children))

  (defn scale [sx & rest]
    (if (and (not (empty? rest)) (number? (first rest)))
      (let [sy (first rest)]
        (element :g {:transform (string "scale(" sx "," sy ")")} (slice rest 1)))
      (element :g {:transform (string "scale(" sx "," sx ")")} rest)))

  # ── Definitions ────────────────────────────────────────────────────

  (defn defs [& children]
    (element :defs {} children))

  (defn linear-gradient [id attrs & stops]
    (element :linearGradient (merge {:id id} attrs) stops))

  (defn radial-gradient [id attrs & stops]
    (element :radialGradient (merge {:id id} attrs) stops))

  (defn stop [offset color & opts]
    (element :stop (merge-attrs {:offset (string (* offset 100.0) "%")
                                 :stop-color color} (opt-attrs opts)) []))

  (defn clip-path [id & children]
    (element :clipPath {:id id} children))

  (defn mask [id & children]
    (element :mask {:id id} children))

  # ── Element manipulation ──────────────────────────────────────────

  (defn set-attr [elem key value]
    (put elem :attrs (put (get elem :attrs) key value)))

  (defn add-child [elem child]
    (let [kids (thaw (get elem :children))]
      (push kids child)
      (put elem :children (freeze kids))))

  (defn wrap [elem tag & opts]
    (element tag (or (first opts) {}) [elem]))

  # ── XML emission ──────────────────────────────────────────────────

  (defn xml-escape [s]
    (-> s
        (string/replace "&" "&amp;")
        (string/replace "<" "&lt;")
        (string/replace ">" "&gt;")))

  (defn xml-escape-attr [s]
    (-> s
        (string/replace "&" "&amp;")
        (string/replace "<" "&lt;")
        (string/replace ">" "&gt;")
        (string/replace "\"" "&quot;")))

  (defn emit-attr-value [v]
    (cond
      (string? v) (xml-escape-attr v)
      (int? v) (string v)
      (float? v) (string v)
      (keyword? v) (string v)
      true nil))

  (defn emit-element [elem]
    (cond
      (string? elem) (xml-escape elem)
      (struct? elem)
        (let* [tag (get elem :tag)
               attrs (get elem :attrs)
               children (get elem :children)
               out @""]
          (append out (string "<" tag))
          (when attrs
            (each [k v] in attrs
              (let [val-str (emit-attr-value v)]
                (when val-str (append out (string " " k "=\"" val-str "\""))))))
          (if (or (nil? children) (empty? children))
            (append out "/>")
            (begin
              (append out ">")
              (each child in children
                (append out (emit-element child)))
              (append out (string "</" tag ">"))))
          (freeze out))
      true ""))

  (defn emit [doc]
    (string "<?xml version=\"1.0\" encoding=\"UTF-8\"?>" (emit-element doc)))

  # ── Export ─────────────────────────────────────────────────────────

  (def exports
    {:svg svg
     :rect rect
     :circle circle
     :ellipse ellipse
     :line line
     :path path
     :polyline polyline
     :polygon polygon
     :text text
     :tspan tspan
     :group group
     :translate translate
     :rotate rotate
     :scale scale
     :defs defs
     :linear-gradient linear-gradient
     :radial-gradient radial-gradient
     :stop stop
     :clip-path clip-path
     :mask mask
     :set-attr set-attr
     :add-child add-child
     :wrap wrap
     :emit emit
     :element element})

  ## If renderer plugin provided, add rendering functions
  (if renderer
    (merge exports
           {:render (get renderer :render)
            :render-raw (get renderer :render-raw)
            :render-to-file (get renderer :render-to-file)
            :dimensions (get renderer :dimensions)})
    exports))
