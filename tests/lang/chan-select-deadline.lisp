(elle/epoch 14)
# audited: 2026-09-30
## chan/select and sys/join never end before their :timeout or :deadline, and a past deadline answers what is ready.
## docs/io/timeout.md
##
## The trap: a wait that turns the time left into whole milliseconds rounds it
## down. The park it arms wakes a fraction of a millisecond early, and a check
## that then reads the leftover fraction as "expired" ends the call before its
## bound. Whether one run lands on either side of a millisecond boundary is
## chance, so every "never before" case below repeats.
##
## The upper bounds are generous on purpose. They only separate "ended at its
## bound" from "waited for something else", and a loaded runner stretches a
## wake by hundreds of milliseconds.

(def repeats 20)
(def bound 0.05)  # s, what each repeated wait is given
(def late 2.0)  # s, above this a wait did not end at its own bound

(defn timed [thunk]
  "Run `thunk` under protect. Returns [ok? result elapsed-seconds]."
  (let* [started (clock/monotonic)
         [ok? result] (protect (thunk))
         elapsed (- (clock/monotonic) started)]
    [ok? result elapsed]))

## ── 1. chan/select with :timeout on a channel nothing answers ────────

(let [[tx rx] (chan)]
  (repeat repeats
          (let [[ok? sel elapsed] (timed (fn []
                  (chan/select @[rx] :timeout bound)))]
            (assert ok?
                    (concat "a select with :timeout must not signal, got "
                            (string sel)))
            (assert (= sel [:timeout])
                    (concat "a select nothing answers ends with [:timeout], got "
                            (string sel)))
            (assert (>= elapsed bound)
                    (concat "a select returned " (string elapsed)
                            "s into a :timeout of " (string bound)))
            (assert (< elapsed late)
                    (concat "a select ran " (string elapsed)
                            "s past its :timeout")))))

(println "  1. chan/select never ends before its :timeout")

## ── 2. chan/select with :deadline ────────────────────────────────────

(let [[tx rx] (chan)]
  (repeat repeats
          (let* [until (+ (clock/monotonic) bound)
                 sel (chan/select @[rx] :deadline until)
                 now (clock/monotonic)]
            (assert (= sel [:timeout])
                    (concat "a select nothing answers ends with [:timeout], got "
                            (string sel)))
            (assert (>= now until)
                    (concat "a select returned " (string (- until now))
                            "s before its :deadline"))
            (assert (< (- now until) late)
                    (concat "a select ran " (string (- now until))
                            "s past its :deadline")))))

(println "  2. chan/select never ends before its :deadline")

## ── 3. Both bounds: whichever comes first ends the wait ──────────────

(let [[tx rx] (chan)]
  (let* [until (+ (clock/monotonic) 10)
         [ok? sel elapsed] (timed (fn []
                                    (chan/select @[rx] :timeout bound
                                    :deadline until)))]
    (assert (= sel [:timeout])
            "the :timeout ends a select whose deadline is far")
    (assert (>= elapsed bound) "and not before the :timeout")
    (assert (< elapsed late) "and long before the :deadline"))
  (let* [until (+ (clock/monotonic) bound)
         [ok? sel elapsed] (timed (fn []
                                    (chan/select @[rx] :timeout 10
                                    :deadline until)))]
    (assert (= sel [:timeout])
            "the :deadline ends a select whose timeout is far")
    (assert (>= (clock/monotonic) until) "and not before the :deadline")
    (assert (< elapsed late) "and long before the :timeout")))

(println "  3. with both bounds, the earlier one ends the select")

## ── 4. A deadline already past answers what is ready ─────────────────

(let [[tx rx] (chan)]
  (chan/send tx :ready)
  (assert (= (chan/select @[rx] :deadline (- (clock/monotonic) 1)) [0 :ready])
          "a past deadline still answers the message that is ready")
  (let [[ok? sel elapsed] (timed (fn []
                                   (chan/select @[rx]
                                   :deadline (- (clock/monotonic) 1))))]
    (assert (= sel [:timeout]) "a past deadline with nothing ready times out")
    (assert (< elapsed late) "without waiting"))
  (chan/send tx :again)
  (assert (= (chan/select @[rx] :timeout 0) [0 :again])
          ":timeout 0 answers the message that is ready")
  (assert (= (chan/select @[rx] :timeout 0) [:timeout])
          ":timeout 0 with nothing ready times out"))

(println "  4. a past deadline answers what is ready, else times out at once")

## ── 5. chan/wait-ready takes the same bounds ─────────────────────────
##
## A wake answers nil whatever woke it, so only the shape and the upper bound
## are the call's to promise here.

(let [[tx rx] (chan)]
  (let [[ok? r elapsed] (timed (fn [] (chan/wait-ready @[rx] :timeout bound)))]
    (assert ok? (concat "chan/wait-ready takes :timeout, got " (string r)))
    (assert (nil? r) "an expired chan/wait-ready answers nil")
    (assert (< elapsed late) "and ends at its bound"))
  (let [[ok? r elapsed] (timed (fn []
                                 (chan/wait-ready @[rx]
                                 :deadline (- (clock/monotonic) 1))))]
    (assert ok? (concat "chan/wait-ready takes :deadline, got " (string r)))
    (assert (nil? r) "a chan/wait-ready past its deadline answers nil")
    (assert (< elapsed late) "without waiting")))

(println "  5. chan/wait-ready takes :timeout and :deadline")

## ── 6. sys/join never ends before its bound ──────────────────────────
##
## One worker serves every repetition: it sleeps far longer than all of them
## together, and a join that times out abandons nothing it cannot join again.
## It ships `ev/run` into its thread so it can `ev/sleep`.

(let [slow (sys/spawn-vm (fn []
                           (ev/run (fn []
                                     (ev/sleep 3)
                                     :slow))))]
  (repeat 10
          (let [[ok? err elapsed] (timed (fn [] (sys/join slow :timeout bound)))]
            (assert (not ok?) "a join on a slow worker times out")
            (assert (= (get err :error) :timeout)
                    (concat "expected a :timeout error, got " (string err)))
            (assert (>= elapsed bound)
                    (concat "a join returned " (string elapsed)
                            "s into a :timeout of " (string bound)))
            (assert (< elapsed late) "and ends at its :timeout")))
  (repeat 10
          (let* [until (+ (clock/monotonic) bound)
                 [ok? err] (protect (sys/join slow :deadline until))
                 now (clock/monotonic)]
            (assert (= (get err :error) :timeout)
                    (concat "expected a :timeout error, got " (string err)))
            (assert (>= now until)
                    (concat "a join returned " (string (- until now))
                            "s before its :deadline"))
            (assert (< (- now until) late) "and ends at its :deadline")))
  (let [[ok? err elapsed] (timed (fn []
                                   (os/join slow
                                   :deadline (- (clock/monotonic) 1))))]
    (assert (= (get err :error) :timeout)
            "a past deadline on a running worker times out")
    (assert (< elapsed late) "without waiting")))

(println "  6. sys/join never ends before its :timeout or :deadline")

## ── 7. A join on a finished worker answers past any bound ────────────

(let [done (sys/spawn-vm (fn [] 42))]
  (assert (= (sys/join done) 42) "the worker finishes")
  (assert (= (sys/join done :deadline (- (clock/monotonic) 1)) 42)
          "a past deadline still answers a finished worker")
  (assert (= (os/join done :timeout 0) 42)
          ":timeout 0 still answers a finished worker")
  (assert (= (sys/join done :timeout 5 :deadline (+ (clock/monotonic) 5)) 42)
          "both bounds together still answer a finished worker"))

(println "  7. a finished worker answers past any bound")

## ── 8. Bad bounds are refused, not waited out ────────────────────────
##
## A refusal is an error of its own kind. A :timeout error here would mean the
## call took the value as a bound and waited.

(defn refused? [thunk]
  "True when `thunk` signals at once with an error other than :timeout."
  (let [[ok? err elapsed] (timed thunk)]
    (and (not ok?) (not (= (get err :error) :timeout)) (< elapsed late))))

(let [[tx rx] (chan)
      done (sys/spawn-vm (fn [] 1))]
  (assert (refused? (fn [] (chan/select @[rx] :timeout -1)))
          "chan/select refuses a negative :timeout")
  (assert (refused? (fn [] (chan/select @[rx] :timeout "soon")))
          "chan/select refuses a :timeout that is not a number")
  (assert (refused? (fn [] (chan/select @[rx] :deadline "soon")))
          "chan/select refuses a :deadline that is not a number")
  (assert (refused? (fn [] (sys/join done :timeout -1)))
          "sys/join refuses a negative :timeout")
  (assert (refused? (fn [] (sys/join done :deadline :later)))
          "sys/join refuses a :deadline that is not a number"))

(println "  8. bad bounds are refused")

(println "chan-select-deadline: all tests passed")
