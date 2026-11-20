(elle/epoch 18)
# audited: 2026-11-20
# An abort raises in a fiber waiting on an I/O operation only once that operation has ended.
# docs/scheduler.md
#
# Each case parks a fiber in a long sleep and aborts it. The abort returns at
# once, and the fiber's cleanup runs later, when the sleep's cancel answers.
#
# The counter-factual is an abort that raises at once. The cleanup then runs
# inside `ev/abort`, beside an operation that has not ended, and has already
# run when the abort returns.

(defn sleeper [cleanups]
  "A fiber that sleeps for thirty seconds, and counts its cleanup in the box
   `cleanups`."
  (ev/spawn (fn []
              (defer
                (rebox cleanups (+ 1 (unbox cleanups)))
                (ev/sleep 30)))))

(defn cleaned-within [cleanups seconds]
  "Wait up to `seconds` for a cleanup to count itself, and answer the count."
  (let [until (+ (clock/monotonic) seconds)]
    (while (and (= 0 (unbox cleanups)) (< (clock/monotonic) until))
      (ev/sleep 0.001))
    (unbox cleanups)))

# ── 1. The cleanup waits for the operation's answer ────────────────────

(let* [cleanups (box 0)
       f (sleeper cleanups)]
  (ev/sleep 0.01)
  (ev/abort f)
  (assert (= 0 (unbox cleanups))
          "the abort returns before the fiber's cleanup runs")
  (assert (= [false {:error :aborted}] (ev/join-protected f))
          "a join on the aborted fiber answers the abort's error")
  (assert (= 1 (unbox cleanups)) "the join waited for the cleanup"))

# ── 2. A second abort changes nothing ──────────────────────────────────

(let* [cleanups (box 0)
       f (sleeper cleanups)]
  (ev/sleep 0.01)
  (ev/abort f)
  (ev/abort f)
  (assert (= 0 (unbox cleanups)) "neither abort ran the cleanup")
  (assert (= [false {:error :aborted}] (ev/join-protected f))
          "the fiber ends on the abort's error")
  (assert (= 1 (unbox cleanups)) "and its cleanup ran once"))

# ── 3. ev/timeout returns without waiting for its loser ────────────────

(let [cleanups (box 0)]
  (assert (nil? (ev/timeout 0.01
                            (fn []
                              (defer
                                (rebox cleanups (+ 1 (unbox cleanups)))
                                (ev/sleep 30))))) "the timer wins")
  (assert (= 0 (unbox cleanups))
          "ev/timeout returns before its loser's cleanup runs")
  (assert (= 1 (cleaned-within cleanups 5))
          "the loser's cleanup runs once its sleep answers"))

(println "tests/lang/abort-waits-for-operation.lisp: all tests passed")
