(elle/epoch 18)
# audited: 2026-11-24
# A callee an exiting arm tail-calls is released once on every path, and the value it returns outlives that release.
# docs/impl/region/window.md
# docs/impl/region/relocate.md
#
# The window anchors a callee's region at the merge, and each tail call that
# names it records the release for the deferred callee channel. A struct callee
# runs on the native path, which falls through to the anchored release. A
# closure callee replaces the frame, and the new activation runs the release
# once the callee completes.
#
# The counter-factual: with the release left in one arm, each call that takes
# another arm strands the callee's region and everything its free cascade
# reclaims.
#
# The subjects run through a mutable binding with mutable flags, so no graft
# folds the branch away. A per-call leak reads at least `window` objects and a
# one-off reads about 0. The sidecar's guardfree turns an early release on any
# path into a fault, and each row reads the value its callee returned.

(def window 2000)

(include-file "gauge/window.lisp")

# ── subjects ─────────────────────────────────────────────────────────

# The else arm tail-calls `go`, a closure the body builds; the then arm falls
# through.
(defn bound-callee [t]
  (letrec [go (fn [a b] a)]
    (if (%eq t 0) 0 (go t 1))))

# Both arms tail-call the parameter `b`.
(defn both-callee [b t]
  (if (%eq t 0) (b :a) (b :b)))

(def @zero 0)
(def @one 1)
(def @bound-call bound-callee)
(def @both-call both-callee)

(defn fresh-struct []
  {:a (string "x" 1) :b (string "yy" 1)})

(defn fresh-closure []
  (let [s (list (string "x" 1) 1)]
    (fn [k] s)))

(def rows
  [["bound-callee exit   " (fn [] (bound-call one))]
   ["bound-callee fall   " (fn [] (bound-call zero))]
   ["struct callee then  " (fn [] (length (both-call (fresh-struct) zero)))]
   ["struct callee else  " (fn [] (length (both-call (fresh-struct) one)))]
   ["closure callee then "
    (fn [] (length (first (both-call (fresh-closure) zero))))]
   ["closure callee else "
    (fn [] (length (first (both-call (fresh-closure) one))))]])

(println "region-branch-arm-callee over " window " calls:")
(each row in rows
  (let [d (measure (get row 1) 100 window)]
    (println "  " (get row 0) " " d)
    (assert (< d 100) (string (get row 0) " leaks per call, delta=" d))))

# Value preservation: each arm still answers what its callee returns.
(assert (= (bound-call one) 1) "the tail-called closure's result")
(assert (= (bound-call zero) 0) "the fall-through arm's result")
(assert (= (both-call {:a "x" :b "yy"} zero) "x") "the struct callee, then")
(assert (= (both-call {:a "x" :b "yy"} one) "yy") "the struct callee, else")
(assert (= (both-call (fn [k] k) zero) :a) "the closure callee, then")
(assert (= (both-call (fn [k] k) one) :b) "the closure callee, else")

(println "region-branch-arm-callee: ok")
