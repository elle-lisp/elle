#!/usr/bin/env elle
(elle/epoch 9)

## Test suite for lib/svg + plugin/svg

(def [ok? svgr] (protect (import "plugin/svg")))
(when (not ok?)
  (println "SKIP: svg plugin not built")
  (exit 0))

## Load library with renderer
(def svg ((import "std/svg") svgr))

# ── Construction ─────────────────────────────────────────────────────

(def r (svg:rect 10 20 100 50 {:fill "blue"}))
(assert (= (get r :tag) :rect) "rect tag")
(assert (= (get (get r :attrs) :x) 10.0) "rect x attr")
(assert (= (get (get r :attrs) :fill) "blue") "rect fill attr")

(def c (svg:circle 50 50 30 {:fill "red"}))
(assert (= (get c :tag) :circle) "circle tag")

(def l (svg:line 0 0 100 100 {:stroke "black"}))
(assert (= (get l :tag) :line) "line tag")

(def p (svg:path "M 0 0 L 100 50"))
(assert (= (get p :tag) :path) "path tag")
(assert (= (get (get p :attrs) :d) "M 0 0 L 100 50") "path d attr")

(def t (svg:text 10 80 "Hello" {:font-size 14}))
(assert (= (get t :tag) :text) "text tag")

# ── Grouping and transforms ─────────────────────────────────────────

(def g (svg:group {:opacity 0.5} r c))
(assert (= (get g :tag) :g) "group tag")
(assert (= (length (get g :children)) 2) "group has 2 children")

(def tr (svg:translate 100 50 r))
(assert (= (get (get tr :attrs) :transform) "translate(100,50)") "translate")

(def rot (svg:rotate 45 r))
(assert (= (get (get rot :attrs) :transform) "rotate(45)") "rotate")

# ── Document ─────────────────────────────────────────────────────────

(def doc (svg:svg 400 300 r c l))
(assert (= (get doc :tag) :svg) "svg tag")
(assert (= (length (get doc :children)) 3) "svg has 3 children")

# ── Gradients ────────────────────────────────────────────────────────

(def grad (svg:linear-gradient "g1" {} (svg:stop 0 "red") (svg:stop 1 "blue")))
(assert (= (get grad :tag) :linearGradient) "linearGradient tag")
(assert (= (length (get grad :children)) 2) "gradient has 2 stops")

# ── Element manipulation ─────────────────────────────────────────────

(def r2 (svg:set-attr r :stroke "black"))
(assert (= (get (get r2 :attrs) :stroke) "black") "set-attr adds attr")
(assert (= (get (get r2 :attrs) :fill) "blue") "set-attr preserves existing")

(def g2 (svg:add-child g p))
(assert (= (length (get g2 :children)) 3) "add-child appends")

(def wrapped (svg:wrap r :g {:opacity 0.8}))
(assert (= (get wrapped :tag) :g) "wrap tag")
(assert (= (length (get wrapped :children)) 1) "wrap has 1 child")

# ── Emission ─────────────────────────────────────────────────────────

(def xml (svg:emit doc))
(assert (not (nil? (string/find xml "<svg"))) "emit contains <svg")
(assert (not (nil? (string/find xml "<rect"))) "emit contains <rect")
(assert (not (nil? (string/find xml "</svg>"))) "emit contains </svg>")
(assert (not (nil? (string/find xml "xmlns"))) "emit contains xmlns")

# ── Rendering (via plugin) ───────────────────────────────────────────

(def simple-doc
  (svg:svg 100 100 (svg:rect 0 0 100 100 {:fill "white"})
    (svg:circle 50 50 40 {:fill "red"})))

(def png-bytes (svg:render simple-doc))
(assert (> (length png-bytes) 0) "render produces bytes")

## PNG magic bytes
(assert (= (png-bytes 0) 137) "PNG magic byte 0")
(assert (= (png-bytes 1) 80) "PNG magic byte 1 (P)")

(def raw (svg:render-raw simple-doc))
(assert (= (get raw :width) 100) "render-raw width")
(assert (= (get raw :height) 100) "render-raw height")
(assert (> (length (get raw :data)) 0) "render-raw has data")

(def dims (svg:dimensions simple-doc))
(assert (= (dims 0) 100.0) "dimensions width")
(assert (= (dims 1) 100.0) "dimensions height")

(println "svg: all tests passed")
