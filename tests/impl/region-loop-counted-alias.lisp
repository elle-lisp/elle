(elle/epoch 19)
# audited: 2026-11-25
# A name a loop binds to a read of a reassigned binding releases its reference on every pass.
# docs/impl/region/lastuse.md
#
# On each pass `least` binds `seen` to a read of `best`, then assigns `best` a
# new array. The read takes a counted reference, because the assign releases
# what `best` held (docs/impl/region/reads.md). So the release of `seen` must
# run on every pass.
#
# The counter-factual: with the loop node as the read's last use, the release
# runs once past the loop. Each call then strands one region for every pass
# but the last, and the window reads two regions per call at three passes.
#
# The loop sits inside a `begin`. The same loop directly in the function's
# body stranded nothing, so the `begin` is part of the shape.
#
# `least` runs through a mutable binding, so no graft folds the call. The
# sidecar's guardfree turns a release that runs too early into a fault.

(def window 500)

(include-file "gauge/window.lisp")

(defn fresh [k]
  [k (+ k 1)])

(defn least [passes]
  (var best (fresh 9))
  (begin
    (var i 0)
    (while (< i passes)
      (let [seen best]
        (assign best (fresh (min (seen 0) i))))
      (assign i (+ i 1))))
  best)

(def @least-call least)

(def d (measure-regions (fn [] (least-call 3)) 50 window))
(println "region-loop-counted-alias over " window " calls: " d)
(assert (< d 50) (string "a counted alias strands regions, delta=" d))

# Value preservation: each pass reads the array the last pass stored.
(assert (= (least-call 3) [0 1]) "three passes keep the least first element")
(assert (= (least-call 0) [9 10]) "no pass keeps the first array")

(println "region-loop-counted-alias: ok")
