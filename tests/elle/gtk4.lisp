(elle/epoch 12)

# ── GTK4 test suite ──────────────────────────────────────────────
#
# Spec parsing, widget lifecycle, and set/get. Every test needs the
# `std/gtk4` module, which needs FFI and the GTK4 shared libraries, so
# gate the module binding itself. The `def` is eager, so it gates during
# barrier-module setup, before any test thunk runs.
#
# `gate!`, never `(exit 0)`: under the runner an exit would kill the
# process mid-run, taking the other 24 files in the batch with it. An
# unmet gate instead records a file-level skip carrying the reason, and
# run directly it exits 0 with the reason on stderr
# (tests/fixtures/gated-toplevel.lisp pins that contract).
(def gtk
  (let [r (protect ((import "std/gtk4")))]
    (gate! (get r 0) "gtk4: FFI or the GTK4 shared libraries are unavailable"
           (get r 1))))

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
(let [[tag props specs text] (gtk:parse-spec [:v-box {:spacing 8} [:label "a"]
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

(assert (= (gtk:json-escape "hello") "\"hello\"") "json-escape: plain string")

(assert (= (gtk:json-escape "") "\"\"") "json-escape: empty string")

(assert (= (gtk:json-escape "he\"llo") "\"he\\\"llo\"")
        "json-escape: embedded quotes")

(assert (= (gtk:json-escape "a\\b") "\"a\\\\b\"") "json-escape: backslash")

(assert (= (gtk:json-escape "line1\nline2") "\"line1\\nline2\"")
        "json-escape: newline")

(assert (= (gtk:json-escape "a\rb") "\"a\\rb\"") "json-escape: carriage return")

(assert (= (gtk:json-escape "a\"b\\c\nd") "\"a\\\"b\\\\c\\nd\"")
        "json-escape: multiple escapes")

(println "gtk4: json-escape OK")

# ── exported surface (pure — no display) ─────────────────────────
# on-key-release pairs with on-key for hold-to-act keys (push-to-talk): the
# console must see the key go UP to stop recording. Guard the export so a
# refactor that drops it fails here, not silently at the call site.

(println "gtk4: testing exported handlers")

(assert (not (nil? gtk:on-key)) "on-key exported")
(assert (not (nil? gtk:on-key-release)) "on-key-release exported")

(println "gtk4: exported handlers OK")

(println "gtk4: all pure tests passed")
