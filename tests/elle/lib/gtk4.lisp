(elle/epoch 9)

# ── GTK4 test suite ──────────────────────────────────────────────
#
# Pure spec-parsing tests run unconditionally.
# Integration tests (widget lifecycle, set/get) require GTK4 libs
# and a display; skipped gracefully when unavailable.

(def [ok gtk] (protect ((import "std/gtk4"))))
(unless ok
  (println "gtk4: skipping — GTK4 libraries not available")
  (exit 0))

# ── parse-spec ───────────────────────────────────────────────────

(println "gtk4: testing parse-spec")

# bare tag
(let [[tag props specs text] (gtk:parse-spec [:button])]
  (assert (= tag :button) "parse-spec: bare tag")
  (assert (= props {}) "parse-spec: bare tag → empty props")
  (assert (empty? specs) "parse-spec: bare tag → no children")
  (assert (nil? text) "parse-spec: bare tag → no text"))

# tag + text (no props)
(let [[tag props specs text] (gtk:parse-spec [:label "hello"])]
  (assert (= tag :label) "parse-spec: tag+text → tag")
  (assert (= props {}) "parse-spec: tag+text → empty props")
  (assert (empty? specs) "parse-spec: tag+text → no child specs")
  (assert (= text "hello") "parse-spec: tag+text → text"))

# tag + props (no text, no children)
(let [[tag props specs text] (gtk:parse-spec [:slider {:min 0 :max 100}])]
  (assert (= tag :slider) "parse-spec: tag+props → tag")
  (assert (= props:min 0) "parse-spec: tag+props → min")
  (assert (= props:max 100) "parse-spec: tag+props → max")
  (assert (empty? specs) "parse-spec: tag+props → no children")
  (assert (nil? text) "parse-spec: tag+props → no text"))

# tag + props + text
(let [[tag props specs text] (gtk:parse-spec [:button {:id :ok} "OK"])]
  (assert (= tag :button) "parse-spec: full → tag")
  (assert (= props:id :ok) "parse-spec: full → props:id")
  (assert (empty? specs) "parse-spec: full → no child specs")
  (assert (= text "OK") "parse-spec: full → text"))

# tag + props + children
(let [[tag props specs text] (gtk:parse-spec [:v-box {:spacing 8}
                                               [:label "a"]
                                               [:label "b"]])]
  (assert (= tag :v-box) "parse-spec: container → tag")
  (assert (= props:spacing 8) "parse-spec: container → spacing")
  (assert (= (length specs) 2) "parse-spec: container → 2 children")
  (assert (nil? text) "parse-spec: container → no text"))

# tag + children (no props)
(let [[tag props specs text] (gtk:parse-spec [:v-box [:label "a"]])]
  (assert (= tag :v-box) "parse-spec: no-props container → tag")
  (assert (= props {}) "parse-spec: no-props container → empty props")
  (assert (= (length specs) 1) "parse-spec: no-props container → 1 child")
  (assert (nil? text) "parse-spec: no-props container → no text"))

# mixed text and child specs
(let [[tag props specs text] (gtk:parse-spec [:expander {} "Details"
                                               [:label "body"]])]
  (assert (= tag :expander) "parse-spec: mixed → tag")
  (assert (= text "Details") "parse-spec: mixed → text")
  (assert (= (length specs) 1) "parse-spec: mixed → 1 child spec"))

(println "gtk4: parse-spec OK")

# ── json-escape ──────────────────────────────────────────────────

(println "gtk4: testing json-escape")

(assert (= (gtk:json-escape "hello") "\"hello\"")
  "json-escape: plain string")

(assert (= (gtk:json-escape "") "\"\"")
  "json-escape: empty string")

(assert (= (gtk:json-escape "he\"llo") "\"he\\\"llo\"")
  "json-escape: embedded quotes")

(assert (= (gtk:json-escape "a\\b") "\"a\\\\b\"")
  "json-escape: backslash")

(assert (= (gtk:json-escape "line1\nline2") "\"line1\\nline2\"")
  "json-escape: newline")

(assert (= (gtk:json-escape "a\rb") "\"a\\rb\"")
  "json-escape: carriage return")

(assert (= (gtk:json-escape "a\"b\\c\nd") "\"a\\\"b\\\\c\\nd\"")
  "json-escape: multiple escapes")

(println "gtk4: json-escape OK")

# ── Integration tests (require display) ──────────────────────────

(def [live-ok win] (protect (gtk:open {:title "Elle GTK4 Test"
                                        :width 200 :height 200})))
(unless live-ok
  (println "gtk4: pure tests passed (no display for integration)")
  (exit 0))

(println "gtk4: testing window lifecycle")
(assert (gtk:open? win) "open? true after open")

# ── Build widget tree ────────────────────────────────────────────

(println "gtk4: testing build")

(gtk:build win
  [:v-box {:id :root :spacing 4}
    [:label {:id :lbl} "Hello"]
    [:heading {:id :hdg} "Title"]
    [:button {:id :btn} "Click"]
    [:checkbox {:id :chk} "Check me"]
    [:switch {:id :sw}]
    [:toggle-button {:id :tgl} "Toggle"]
    [:slider {:id :sld :min 0.0 :max 100.0 :value 50.0}]
    [:spin-button {:id :spin :min 0.0 :max 100.0 :value 25.0}]
    [:text-input {:id :inp :value "initial"}]
    [:text-edit {:id :edit}]
    [:progress-bar {:id :prog :value 0.5}]
    [:separator {}]
    [:spacer {}]
    [:spinner {:id :spnr}]])

# ── set/get roundtrip ────────────────────────────────────────────

(println "gtk4: testing set/get")

# label
(gtk:set win :lbl "Updated")
(assert (= (gtk:get win :lbl) "Updated") "label set/get")

# heading
(gtk:set win :hdg "New Title")
(assert (= (gtk:get win :hdg) "New Title") "heading set/get")

# button
(gtk:set win :btn "New Label")
(assert (= (gtk:get win :btn) "New Label") "button set/get")

# text-input
(gtk:set win :inp "edited")
(assert (= (gtk:get win :inp) "edited") "text-input set/get")

# text-edit
(gtk:set win :edit "multi\nline")
(assert (= (gtk:get win :edit) "multi\nline") "text-edit set/get")

# checkbox
(gtk:set win :chk true)
(assert (= (gtk:get win :chk) true) "checkbox set true")
(gtk:set win :chk false)
(assert (= (gtk:get win :chk) false) "checkbox set false")

# switch
(gtk:set win :sw true)
(assert (= (gtk:get win :sw) true) "switch set true")
(gtk:set win :sw false)
(assert (= (gtk:get win :sw) false) "switch set false")

# toggle-button
(gtk:set win :tgl true)
(assert (= (gtk:get win :tgl) true) "toggle-button set true")
(gtk:set win :tgl false)
(assert (= (gtk:get win :tgl) false) "toggle-button set false")

# slider
(gtk:set win :sld 75.0)
(assert (= (gtk:get win :sld) 75.0) "slider set/get")

# spin-button
(gtk:set win :spin 42.0)
(assert (= (gtk:get win :spin) 42.0) "spin-button set/get")

# progress-bar
(gtk:set win :prog 0.8)
(assert (= (gtk:get win :prog) 0.8) "progress-bar set/get")

# spinner (set true starts, set false stops — no getter)
(gtk:set win :spnr true)
(gtk:set win :spnr false)

(println "gtk4: set/get OK")

# ── poll ─────────────────────────────────────────────────────────

(println "gtk4: testing poll")

(def events (gtk:poll win))
(assert (array? events) "poll returns array")

# ── nonexistent widget ───────────────────────────────────────────

(assert (nil? (gtk:get win :nonexistent)) "get nonexistent → nil")

# ── visibility and sensitivity ───────────────────────────────────

(println "gtk4: testing visibility/sensitivity")

(gtk:set-visible win :lbl false)
(gtk:set-visible win :lbl true)
(gtk:set-sensitive win :btn false)
(gtk:set-sensitive win :btn true)

# ── set-title ────────────────────────────────────────────────────

(gtk:set-title win "Updated Title")

# ── load-css ─────────────────────────────────────────────────────

(println "gtk4: testing CSS")

(gtk:load-css win "button { color: red; }")
# loading CSS again reuses the provider
(gtk:load-css win "label { font-weight: bold; }")

# ── add / remove ─────────────────────────────────────────────────

(println "gtk4: testing add/remove")

(gtk:add win :root [:label {:id :added} "New"])
(assert (= (gtk:get win :added) "New") "added widget accessible")

(gtk:remove win :added)
(assert (nil? (gtk:get win :added)) "removed widget gone")

# ── rebuild ──────────────────────────────────────────────────────

(println "gtk4: testing rebuild")

(gtk:rebuild win :root [:label {:id :rebuilt} "Rebuilt"])
(assert (= (gtk:get win :rebuilt) "Rebuilt") "rebuild adds widget")

# ── nested containers ────────────────────────────────────────────

(println "gtk4: testing nested containers")

(gtk:add win :root
  [:h-box {:id :nested :spacing 4}
    [:label {:id :n1} "Left"]
    [:label {:id :n2} "Right"]])
(assert (= (gtk:get win :n1) "Left") "nested child 1")
(assert (= (gtk:get win :n2) "Right") "nested child 2")

# ── scroll-area ──────────────────────────────────────────────────

(gtk:add win :root
  [:scroll-area {:id :scr :height 100}
    [:label {:id :scr-child} "Scrollable"]])
(assert (= (gtk:get win :scr-child) "Scrollable") "scroll-area child")

# ── frame ────────────────────────────────────────────────────────

(gtk:add win :root
  [:frame {:id :frm} "Frame Title"
    [:label {:id :frm-child} "Framed"]])
(assert (= (gtk:get win :frm-child) "Framed") "frame child")

# ── expander ─────────────────────────────────────────────────────

(gtk:add win :root
  [:expander {:id :exp :expanded true} "Expand"
    [:label {:id :exp-child} "Hidden"]])
(assert (= (gtk:get win :exp-child) "Hidden") "expander child")

# ── revealer ─────────────────────────────────────────────────────

(gtk:add win :root
  [:revealer {:id :rev :revealed true}
    [:label {:id :rev-child} "Revealed"]])
(assert (= (gtk:get win :rev-child) "Revealed") "revealer child")

# ── grid ─────────────────────────────────────────────────────────

(gtk:add win :root
  [:grid {:id :grd :columns 2 :row-spacing 4 :col-spacing 4}
    [:label {:id :g1} "A"]
    [:label {:id :g2} "B"]
    [:label {:id :g3} "C"]
    [:label {:id :g4} "D"]])
(assert (= (gtk:get win :g1) "A") "grid child 1")
(assert (= (gtk:get win :g4) "D") "grid child 4")

# ── paned ────────────────────────────────────────────────────────

(gtk:add win :root
  [:paned {:id :pnd}
    [:label {:id :pnd-start} "Start"]
    [:label {:id :pnd-end} "End"]])
(assert (= (gtk:get win :pnd-start) "Start") "paned start")
(assert (= (gtk:get win :pnd-end) "End") "paned end")

# ── center-box ───────────────────────────────────────────────────

(gtk:add win :root
  [:center-box {:id :cb}
    [:label {:id :cb-s} "S"]
    [:label {:id :cb-c} "C"]
    [:label {:id :cb-e} "E"]])
(assert (= (gtk:get win :cb-s) "S") "center-box start")
(assert (= (gtk:get win :cb-c) "C") "center-box center")
(assert (= (gtk:get win :cb-e) "E") "center-box end")

# ── overlay ──────────────────────────────────────────────────────

(gtk:add win :root
  [:overlay {:id :ovl}
    [:label {:id :ovl-main} "Main"]
    [:label {:id :ovl-over} "Over"]])
(assert (= (gtk:get win :ovl-main) "Main") "overlay main child")
(assert (= (gtk:get win :ovl-over) "Over") "overlay overlay child")

# ── close ────────────────────────────────────────────────────────

(println "gtk4: testing close")

(gtk:close win)
(assert (not (gtk:open? win)) "open? false after close")

(println "gtk4: all tests passed")
