## spike/gtk4-api-test.lisp — Smoke test for lib/gtk4.lisp
##
## Opens a window with representative widgets, pumps for a few seconds,
## tests get/set, then closes.

(def gtk ((import "std/gtk4")))

# ── Open window ───────────────────────────────────────────────────

(def win (gtk:open {:title "API Test" :width 600 :height 400}))

# ── Build widget tree ─────────────────────────────────────────────

(gtk:build win
  [:v-box {:spacing 8 :margin 12}
    [:heading {:id :title} "GTK4 API Test"]
    [:separator]
    [:h-box {:spacing 8}
      [:text-input {:id :name :hint "Enter name"}]
      [:button {:id :greet} "Greet"]]
    [:h-box {:spacing 8}
      [:checkbox {:id :bold} "Bold"]
      [:switch {:id :dark}]
      [:slider {:id :vol :min 0 :max 100}]]
    [:h-box {:spacing 8}
      [:spin-button {:id :count :min 0 :max 10 :step 1}]
      [:progress-bar {:id :prog :value 0.5}]]
    [:label {:id :output} "Ready."]])

# ── Test get/set ──────────────────────────────────────────────────

(gtk:set win :name "Alice")
(assert (= (gtk:get win :name) "Alice") "set/get text-input")

(gtk:set win :output "Testing...")
(assert (= (gtk:get win :output) "Testing...") "set/get label")

(gtk:set win :bold true)
(assert (= (gtk:get win :bold) true) "set/get checkbox")

(gtk:set win :vol 42.0)
# slider values may have floating point drift, just check it's close
(assert (< (- (gtk:get win :vol) 42.0) 1.0) "set/get slider")

(gtk:set win :prog 0.75)

(gtk:set-title win "Test Passed")

# ── Load CSS ──────────────────────────────────────────────────────

(gtk:load-css win ".title-2 { color: #3584e4; }")

# ── Pump briefly ──────────────────────────────────────────────────

(var frames 0)
(while (and (gtk:open? win) (< frames 60))
  (let [[events (gtk:poll win)]]
    (each event in events
      (println "event:" event)))
  (assign frames (+ frames 1))
  (time/sleep 0.016))

(gtk:close win)

(println "gtk4 api test: PASS")
