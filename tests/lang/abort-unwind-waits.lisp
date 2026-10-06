(elle/epoch 14)
# audited: 2026-10-06
# An aborted fiber that waits again while it unwinds is resumed only by what it waits on now.
# docs/scheduler.md
#
# Each case parks a fiber in one kind of wait, aborts it, and lets its
# `defer` sleep. While the sleep runs, the case fires the wake the old
# wait was owed. The sleep must still answer what a sleep answers.
#
# The counter-factual is the old wake landing in the new wait: the sleep
# returns the wake's payload (`true` from a futex wake, the join's
# `[ok? value]` pair, the selected fiber) and ends early. A cleanup that
# waits twice then leaves its second operation paired with a fiber that
# has finished, and the program crashes at its end on the abort's own
# error, which the abort had already marked observed.

# The deadline every join gets. Two orders of magnitude above what these
# fibers cost, so only a lost wake can reach it.
(def deadline 5)

# What a sleep answers when nothing else resumes it.
(def slept (ev/sleep 0))

(defn finish [f]
  "Join `f` without raising, or return :timed-out when it does not finish."
  (let [r (ev/timeout deadline (fn [] (ev/join-protected f)))]
    (if (nil? r) :timed-out r)))

(defn parked-fiber [key bx]
  "Spawn a fiber that parks on `key` until released, then answers :done."
  (ev/spawn (fn []
              (ev/futex-wait key bx 0)
              :done)))

(defn release [key bx]
  "Release every fiber parked on `key`. Answers how many it woke."
  (rebox bx 1)
  (ev/futex-wake key 64))

# ── 1. A park ────────────────────────────────────────────────────────

(println "an aborted fiber leaves its park queue before it unwinds...")

(let* [key (sys/unique)
       bx (box 0)
       seen @[]
       f (ev/spawn (fn []
                     (defer
                       (begin
                         (push seen (ev/sleep 0.05))
                         (push seen (ev/sleep 0.05)))
                       (ev/futex-wait key bx 0))))]
  (ev/sleep 0.01)
  (ev/abort f)
  (assert (= 0 (release key bx))
          "the wake must find nobody: the aborted fiber no longer waits on the key")
  (assert (= [false {:error :aborted}] (finish f))
          "the aborted fiber must finish on the abort's error")
  (assert (= [slept slept] (freeze seen))
          (string "both cleanup sleeps must answer as sleeps, got "
                  (string (freeze seen)))))

# ── 2. A join ────────────────────────────────────────────────────────

(println "an aborted joiner leaves the waiter list before it unwinds...")

(let* [key (sys/unique)
       bx (box 0)
       seen @[]
       target (parked-fiber key bx)
       waiter (ev/spawn (fn []
                          (defer
                            (push seen (ev/sleep 0.05))
                            (ev/join target))))]
  (ev/sleep 0.01)
  (ev/abort waiter)
  (release key bx)
  (assert (= [true :done] (finish target)) "the target must still finish")
  (assert (= [false {:error :aborted}] (finish waiter))
          "the aborted joiner must finish on the abort's error")
  (assert (= [slept] (freeze seen))
          (string "the cleanup sleep must answer as a sleep, got "
                  (string (freeze seen)))))

# ── 3. A select ──────────────────────────────────────────────────────

(println "an aborted select waiter leaves its select set before it unwinds...")

(let* [key (sys/unique)
       bx (box 0)
       seen @[]
       candidate (parked-fiber key bx)
       waiter (ev/spawn (fn []
                          (defer
                            (push seen (ev/sleep 0.05))
                            (ev/select [candidate]))))]
  (ev/sleep 0.01)
  (ev/abort waiter)
  (release key bx)
  (assert (= [true :done] (finish candidate)) "the candidate must still finish")
  (assert (= [false {:error :aborted}] (finish waiter))
          "the aborted select waiter must finish on the abort's error")
  (assert (= [slept] (freeze seen))
          (string "the cleanup sleep must answer as a sleep, got "
                  (string (freeze seen)))))

(println "abort-unwind-waits: an abort ends the wait it lands in")
