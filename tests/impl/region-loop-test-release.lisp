(elle/epoch 18)
# audited: 2026-11-24
# What a loop's test makes is released on each pass, not once past the loop.
# docs/impl/region/lastuse.md
#
# Each subject's test makes or reads a value with a region on every pass, and
# the caller hands each call a fresh argument. A release past the test's branch
# runs on the loop's exit path alone, so each pass that loops on strands one
# region.
#
# The counter-factual: with the branch's node as the test's last use,
# `element-test` strands the argument's region once per call, and the window
# reads one region per call.
#
# The subjects run through mutable bindings, so no graft folds a call. The
# sidecar's guardfree turns an early release on any path into a fault.

(def window 2000)

(include-file "gauge/window.lisp")

(def kept "x")
(def @yes true)

# ── subjects ─────────────────────────────────────────────────────────

(defn element-test [gs x]
  (var k 0)
  (while (= (gs k) x) (assign k (inc k)))
  k)

(defn bound-element-test [gs x]
  (var k 0)
  (while (let [e (gs k)]
           (if yes (= e x) (= e "y")))
    (assign k (inc k)))
  k)

(defn made-in-test [n]
  (var k 0)
  (while (and (< k n) [kept k]) (assign k (inc k)))
  k)

(defn handed-on-by-if [n]
  (var k 0)
  (while (if (< k n) [kept k] nil) (assign k (inc k)))
  k)

(defn literal-test [gs x]
  (var k 0)
  (while (= (gs k) "-") (assign k (inc k)))
  k)

(def @element-call element-test)
(def @bound-call bound-element-test)
(def @made-call made-in-test)
(def @handed-call handed-on-by-if)
(def @literal-call literal-test)
(def @dashes "--x")
(def @three 3)

(def rows
  [["(= (gs k) x)              " (fn [] (element-call (->array dashes) "-"))]
   ["a let in the test         " (fn [] (bound-call (->array dashes) "-"))]
   ["an array the test makes   " (fn [] (made-call three))]
   ["an if hands the array on  " (fn [] (handed-call three))]
   ["control: (= (gs k) \"-\")   " (fn [] (literal-call (->array dashes) "-"))]])

(println "region-loop-test-release over " window " calls:")
(each row in rows
  (let [d (measure-regions (get row 1) 100 window)]
    (println "  " (get row 0) " " d)
    (assert (< d 100) (string (get row 0) " strands regions, delta=" d))))

# Value preservation: each loop stops where its test turns false.
(assert (= (element-call (->array dashes) "-") 2) "two dashes match")
(assert (= (bound-call (->array dashes) "-") 2) "two dashes match through a let")
(assert (= (made-call three) 3) "three passes make an array")
(assert (= (handed-call three) 3) "three passes hand an array on")
(assert (= (literal-call (->array dashes) "-") 2) "two dashes match a literal")

(println "region-loop-test-release: ok")
