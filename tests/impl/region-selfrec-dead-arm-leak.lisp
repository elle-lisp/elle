(elle/epoch 15)
# audited: 2026-11-10
# A self-recursive closure one branch arm tail-calls is released on the arm that never names it.
# docs/impl/selfrec.md
# docs/impl/region/compensate.md
#
# A self-recursive binding refers to itself through the executing closure, so
# its environment holds nil for itself and no environment holds the closure.
# Its one release sits in the arm that tail-calls the binding, past that
# `TailCall`, where the deferred release supplies it. A sibling arm that never
# names the binding owes a release of its own: branch compensation's head
# release.
#
# The counter-factual: the region capture graph counted the self-reference as a
# capture, which excluded the closure's region from compensation, so every call
# that took the dead arm leaked the closure and its environment.
#
# Each row runs the same call over a fixed window after a warmup, so a per-call
# leak reads at least `window` objects and a one-off reads about 0. The sidecar's
# guardfree turns an early release on either arm into a fault.

(def window 2000)

(defn measure [thunk warm window]
  (def @i 0)
  (while (< i warm)
    (thunk)
    (assign i (+ i 1)))
  (def before (arena/count))
  (def @j 0)
  (while (< j window)
    (thunk)
    (assign j (+ j 1)))
  (- (arena/count) before))

# ── subjects ─────────────────────────────────────────────────────────

(defn def-arm [n t]
  (def go (fn [m] (if (< m 1) 0 (go (- m 1)))))
  (if t 7 (go n)))

(defn def-cond [n t]
  (def go (fn [m] (if (< m 1) 0 (go (- m 1)))))
  (cond
    (= t 0) 7
    (= t 1) (go n)
    8))

(defn letrec-arm [n t]
  (letrec [go (fn [m] (if (< m 1) 0 (go (- m 1))))]
    (if t 7 (go n))))

# The stdlib's `distinct` binds a self-recursive list walker and reaches it only
# when the argument is a list, so an array takes the dead arm.
(defn distinct-array []
  (distinct @[2 1 2]))

(def rows
  [["def-arm dead     " (fn [] (def-arm 3 true))]
   ["def-arm tail     " (fn [] (def-arm 3 false))]
   ["def-cond dead    " (fn [] (def-cond 3 0))]
   ["def-cond tail    " (fn [] (def-cond 3 1))]
   ["def-cond else    " (fn [] (def-cond 3 2))]
   ["letrec-arm dead  " (fn [] (letrec-arm 3 true))]
   ["letrec-arm tail  " (fn [] (letrec-arm 3 false))]
   ["distinct-array   " distinct-array]])

(println "region-selfrec-dead-arm-leak over " window " calls:")
(each row in rows
  (let [d (measure (get row 1) 100 window)]
    (println "  " (get row 0) " " d)
    (assert (< d 100) (string (get row 0) " leaks per call, delta=" d))))

# Value preservation: each arm still runs what it names.
(assert (= (def-arm 3 true) 7) "the def's dead arm")
(assert (= (def-arm 3 false) 0) "the def's tail arm recurses")
(assert (= (def-cond 3 0) 7) "the def's dead clause")
(assert (= (def-cond 3 1) 0) "the def's tail clause recurses")
(assert (= (def-cond 3 2) 8) "the def's else clause")
(assert (= (letrec-arm 3 true) 7) "the letrec's dead arm")
(assert (= (letrec-arm 3 false) 0) "the letrec's tail arm recurses")
(assert (= (distinct @[2 1 2]) @[2 1]) "distinct over an array")
(assert (= (distinct (list 2 1 2)) (list 2 1)) "distinct over a list")

(println "region-selfrec-dead-arm-leak: ok")
