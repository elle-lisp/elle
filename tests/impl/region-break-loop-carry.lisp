(elle/epoch 18)
# audited: 2026-11-24
# A value a loop makes and a break carries out is released on each pass that loops on, and once more past the block.
# docs/impl/region/breakloop.md
#
# Each subject makes a string per pass and breaks with the string of pass `m`.
# Every pass before it loops on. One release past the block reaches only the
# carried string, so each pass that loops on strands its region.
#
# The counter-factual: with the transfer pin alone, `(carried 5)` strands 5
# regions per call, and a window of calls reads 5 times the window.
#
# The subjects run through mutable bindings, so no graft folds a call. The
# sidecar's guardfree turns an early release on any path into a fault.

(def window 2000)

(include-file "gauge/window.lisp")

# ── subjects ─────────────────────────────────────────────────────────

(defn carried [m]
  (var k 0)
  (forever
    (let [b (string/repeat "x" (inc k))]
      (when (>= k m) (break b)))
    (assign k (inc k))))

(defn while-carried [m]
  (var k 0)
  (block (while true
           (let [b (string/repeat "x" (inc k))]
             (when (>= k m) (break b)))
           (assign k (inc k)))))

(defn bound-carried [m]
  (var k 0)
  (let [r (forever
            (let [b (string/repeat "x" (inc k))]
              (when (>= k m) (break b)))
            (assign k (inc k)))]
    (length r)))

(defn used-after [m]
  (var k 0)
  (forever
    (let [b (string/repeat "x" (inc k))]
      (when (>= k m) (break b))
      (length b))
    (assign k (inc k))))

(defn immediate [m]
  (var k 0)
  (forever
    (let [b (string/repeat "x" (inc k))]
      (when (>= k m) (break k)))
    (assign k (inc k))))

(def @carried-call carried)
(def @while-call while-carried)
(def @bound-call bound-carried)
(def @used-call used-after)
(def @immediate-call immediate)
(def @five 5)
(def @zero 0)

(def rows
  [["forever, 5 passes loop on " (fn [] (carried-call five))]
   ["while, 5 passes loop on   " (fn [] (while-call five))]
   ["bound to a name           " (fn [] (bound-call five))]
   ["used after the branch     " (fn [] (used-call five))]
   ["control: first pass breaks" (fn [] (carried-call zero))]
   ["control: breaks an integer" (fn [] (immediate-call five))]])

(println "region-break-loop-carry over " window " calls:")
(each row in rows
  (let [d (measure-regions (get row 1) 100 window)]
    (println "  " (get row 0) " " d)
    (assert (< d 100) (string (get row 0) " strands regions, delta=" d))))

# Value preservation: the break carries the value of the pass that broke.
(assert (= (carried-call five) "xxxxxx") "forever carries pass 5's string")
(assert (= (while-call five) "xxxxxx") "while carries pass 5's string")
(assert (= (bound-call five) 6) "the bound value reads after the block")
(assert (= (used-call five) "xxxxxx") "a use after the branch")
(assert (= (carried-call zero) "x") "the first pass breaks")
(assert (= (immediate-call five) 5) "an integer carried out")

(println "region-break-loop-carry: ok")
