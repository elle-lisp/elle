(elle/epoch 18)
# audited: 2026-11-24
# A closure one arm hands to the branch's result is released on entry to each arm that never names it.
# docs/impl/region/compensate.md
#
# `k` captures `x`, and the branch hands `k` to `r` on one arm alone. Every
# later use of `k` reads `r`, so `k`'s release sits at `r`'s last use. Where
# that use is a tail call, the release lands past the `TailCall`, and the call
# defers the release of the callee's own region alone. An arm that never
# names `k` releases it on entry instead.
#
# The counter-factual: without the head release, each call that takes a dead
# arm behind the tail call strands `k`'s closure and the array it captures.
#
# The subjects run through a mutable binding with mutable flags, so no graft
# folds the branch away. A per-call leak reads at least `window` objects and a
# one-off reads about 0. The sidecar's guardfree turns an early release on any
# arm into a fault, and the fall-through rows run the release past the join
# after the head release on the same path.

(def window 2000)

(include-file "gauge/window.lisp")

# ── subjects ─────────────────────────────────────────────────────────

(defn tail-if [c]
  (def x @[1])
  (def k (fn [] (length x)))
  (def r (if c k (fn [] 0)))
  (length x)
  (r))

(defn fall-if [c]
  (def x @[1])
  (def k (fn [] (length x)))
  (def r (if c k (fn [] 0)))
  (def v (r))
  (+ v 1))

(defn tail-match [t]
  (def x @[1 2])
  (def k (fn [] (length x)))
  (def r
    (match t
      :hand k
      :one (fn [] 1)
      _ (fn [] 2)))
  (r))

(defn returned [c]
  (def x @[1 2 3])
  (def k (fn [] (length x)))
  (def r (if c k (fn [] 0)))
  r)

(def @yes true)
(def @no false)
(def @hand :hand)
(def @one :one)
(def @other :other)
(def @tail-if-call tail-if)
(def @fall-if-call fall-if)
(def @tail-match-call tail-match)
(def @returned-call returned)

(def rows
  [["tail-if dead       " (fn [] (tail-if-call no))]
   ["tail-if handed     " (fn [] (tail-if-call yes))]
   ["fall-if dead       " (fn [] (fall-if-call no))]
   ["fall-if handed     " (fn [] (fall-if-call yes))]
   ["tail-match one     " (fn [] (tail-match-call one))]
   ["tail-match other   " (fn [] (tail-match-call other))]
   ["tail-match handed  " (fn [] (tail-match-call hand))]
   ["returned dead      " (fn [] ((returned-call no)))]
   ["returned handed    " (fn [] ((returned-call yes)))]])

(println "region-result-dead-arm-leak over " window " calls:")
(each row in rows
  (let [d (measure (get row 1) 100 window)]
    (println "  " (get row 0) " " d)
    (assert (< d 100) (string (get row 0) " leaks per call, delta=" d))))

# Value preservation: each arm still calls what it hands on.
(assert (= (tail-if-call yes) 1) "the handed closure reads its capture")
(assert (= (tail-if-call no) 0) "the dead arm's closure")
(assert (= (fall-if-call yes) 2) "the handed closure past the join")
(assert (= (fall-if-call no) 1) "the dead arm's closure past the join")
(assert (= (tail-match-call hand) 2) "the handed match arm")
(assert (= (tail-match-call one) 1) "the first dead match arm")
(assert (= (tail-match-call other) 2) "the wildcard match arm")
(assert (= ((returned-call yes)) 3) "the returned closure reads its capture")
(assert (= ((returned-call no)) 0) "the dead arm's returned closure")

(println "region-result-dead-arm-leak: ok")
