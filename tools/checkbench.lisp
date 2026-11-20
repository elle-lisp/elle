(elle/epoch 18)
# audited: 2026-11-20
# Times std/check through its public interface: a replay, random cases, the enumeration, or a capped shrink.
# tools/overview.md
# docs/stdlib/check.md
#
# Usage: elle tools/checkbench.lisp MEASURE [MODULE]
#
# MEASURE is replay, random, enumerate or shrink. MODULE is what the tool
# imports, std/check by default, or the path of another tree's check.lisp.
# Run one measure per process: the JIT compiles what the first measure ran,
# and a later measure in the same process reads that state, not its own.
#
# A reading is the fastest of ten runs, after three that warm the tiers. A
# replay and the random cases are priced per draw, the enumeration per case,
# and the shrink per candidate.

(def args (sys/args))
(def measure (get args 0))
(def check ((import (or (get args 1) "std/check"))))

(defn timed [f]
  "The fewest seconds one of ten calls of F takes, after three that warm the
   tiers. The JIT compiles in the background, so one warming call can leave
   a reading before its code is ready."
  (repeat 3 (f))
  (var best nil)
  (repeat 10
    (let [t0 (clock/monotonic)]
      (f)
      (let [t (- (clock/monotonic) t0)]
        (when (or (nil? best) (< t best)) (assign best t)))))
  best)

(defn report [seconds per unit]
  (println (string/format "{:<12} {:>9.3} us per {}" measure
                          (/ (* 1e6 seconds) per) unit)))

(def draws 1000)
(def wide
  (fn [draw]
    (each _ in (range draws)
      (draw 1001))
    nil))

(case measure
  "replay"
    (let [bits (get (check:run wide (fn [_] false) :depth 0 :shrinks 0) :bits)]
      (report (timed (fn [] (check:run wide (fn [_] true) :bits bits))) draws
              "draw"))
  "random"
    (report (timed (fn [] (check:sample wide :count 10))) (* 10 draws) "draw")
  "enumerate"
    (let [digits (check:array-of (check:int 0 9) 6)
          cases (length (check:enumerate digits 6))]
      (report (timed (fn [] (check:enumerate digits 6))) cases "case"))
  # Every deletion of a run of the 45 elements passes, so the shrinker runs
  # its default cap of 1000 candidates.
  "shrink"
    (let [bits-45 @[1 0 1 1 0 1]]
      (repeat 45 (push bits-45 0))
      (report (timed (fn []
                       (check:run (check:array-of (check:bool) 45)
                                  (fn [xs] (< (length xs) 45)) :bits bits-45)))
              1000 "candidate"))
  (error {:error :argument-error
          :message (string "checkbench: expected a measure of replay, random, "
                           "enumerate or shrink, got " measure)}))
