(elle/epoch 18)
# audited: 2026-11-20
# Times std/check through its public interface: a replay, random cases, the enumeration, and a capped shrink.
# tools/overview.md
# docs/stdlib/check.md
#
# Each reading is the fastest of five runs, after one that warms the tiers. A
# replay and the random cases are priced per draw, the enumeration per case,
# and the shrink per candidate.

(def check ((import "std/check")))

(defn timed [f]
  "The fewest seconds one of five calls of F takes, after a call that warms
   the tiers."
  (f)
  (var best nil)
  (repeat 5
    (let [t0 (clock/monotonic)]
      (f)
      (let [t (- (clock/monotonic) t0)]
        (when (or (nil? best) (< t best)) (assign best t)))))
  best)

(defn report [what seconds per unit]
  (println (string/format "{:<12} {:>9.3} us per {}" what
                          (/ (* 1e6 seconds) per) unit)))

(def draws 1000)
(def wide
  (fn [draw]
    (each _ in (range draws)
      (draw 1001))
    nil))
(def bits (get (check:run wide (fn [_] false) :depth 0 :shrinks 0) :bits))

(report "replay" (timed (fn [] (check:run wide (fn [_] true) :bits bits))) draws
        "draw")
(report "random" (timed (fn [] (check:sample wide :count 10))) (* 10 draws)
        "draw")

(def digits (check:array-of (check:int 0 9) 6))
(def cases (length (check:enumerate digits 6)))
(report "enumerate" (timed (fn [] (check:enumerate digits 6))) cases "case")

# Every deletion of a run of the 45 elements passes, so the shrinker runs
# its default cap of 1000 candidates.
(def bits-45 @[1 0 1 1 0 1])
(repeat 45 (push bits-45 0))
(report "shrink"
        (timed (fn []
                 (check:run (check:array-of (check:bool) 45)
                            (fn [xs] (< (length xs) 45)) :bits bits-45))) 1000
        "candidate")
