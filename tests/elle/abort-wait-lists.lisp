(elle/epoch 12)
# audited: 2026-09-16
# A join waiter list and a select set hold only live waiters.
#
# The trap is `protect`. `ev/abort` and an expired `ev/timeout` both
# kill a waiter by injecting an error, and a waiter that catches its own
# injected error runs to :dead — the one state `fiber/resume` refuses.
# Drop the `protect` every case here carries and the waiter ends :error
# instead, where a resume is tolerated, and the file pins nothing.
#
# The counter-factual is a crash a long way from its cause. A dead
# waiter left on the list is resumed when the fiber it waited on
# finishes, and `fiber/resume: cannot resume completed fiber` comes out
# of the event loop at whatever line the program was pumping from,
# naming no fiber.
#
# See docs/scheduler.md § Join waiters and select sets.

# The deadline every join gets. Two orders of magnitude above what these
# fibers cost, so only a lost result can reach it.
(def deadline 5)

(defn join-by-deadline [f]
  "Join `f`, or return :timed-out when it does not finish in time."
  (let [r (ev/timeout deadline (fn [] (ev/join f)))]
    (if (nil? r) :timed-out r)))

(defn parked-fiber [key bx]
  "Spawn a fiber that parks on `key` until released, then answers :done."
  (ev/spawn (fn []
              (ev/futex-wait key bx 0)
              :done)))

(defn release [key bx]
  "Release every fiber parked on `key`."
  (rebox bx 1)
  (ev/futex-wake key 64))

# Both counts below are scheduler-wide, and this file is never the only
# thing its scheduler runs: the corpus runner holds a join on the form it
# is running, so a count this file expects to be one reads as two. Every
# case takes its own baseline and asserts the change from it.

(defn joins []
  "How many fibers have at least one join waiter right now."
  (get (ev/report) :joins))

(defn selects []
  "How many fibers are parked on a select set right now."
  (get (ev/report) :selects))

# ── 1. An aborted joiner leaves the waiter list ──────────────────────

(println "abort removes a joiner from its target's waiter list...")

(let* [base (joins)
       key (sys/unique)
       bx (box 0)
       target (parked-fiber key bx)
       waiter (ev/spawn (fn []
                          (protect (ev/join-protected target))
                          :waited))]
  (ev/sleep 0.05)
  (assert (= (+ base 1) (joins))
          "the joiner must be on the target's waiter list")
  (ev/abort waiter)
  (assert (= :dead (fiber/status waiter))
          "the joiner's own protect must catch the injected error")
  (assert (= base (joins)) "the dead joiner must leave the waiter list")
  # The target finishes now. With the dead joiner still listed, this is
  # where the scheduler resumes it and the loop raises.
  (release key bx)
  (assert (= :done (join-by-deadline target))
          "the target must still deliver its value"))

# ── 2. A live joiner keeps its place beside a dead one ───────────────

(println "aborting one joiner leaves the other its result...")

(let* [base (joins)
       key (sys/unique)
       bx (box 0)
       target (parked-fiber key bx)
       dying (ev/spawn (fn []
                         (protect (ev/join-protected target))
                         :waited))
       living (ev/spawn (fn [] (ev/join-protected target)))]
  (ev/sleep 0.05)
  (assert (= (+ base 1) (joins)) "both joiners must be on one waiter list")
  (ev/abort dying)
  (assert (= (+ base 1) (joins)) "the list must survive the loss of one joiner")
  (release key bx)
  (assert (= [true :done] (join-by-deadline living))
          "the live joiner must receive the target's result"))

# ── 3. A timed-out protected join leaves the waiter list ─────────────
#
# The deadline around a protected join is the shape a program reaches
# this through. `ev/timeout` selects over the body and a timer, so this
# case puts a fiber in both lists at once.

(println "ev/timeout removes the joiner it kills...")

(let* [jbase (joins)
       sbase (selects)
       key (sys/unique)
       bx (box 0)
       target (parked-fiber key bx)]
  (assert (nil? (ev/timeout 0.05
                            (fn []
                              (protect (ev/join-protected target))
                              :waited)))
          "the deadline must beat a join on a parked target")
  (assert (= jbase (joins)) "the killed body must leave the waiter list")
  (assert (= sbase (selects)) "the timeout's own select set must be gone")
  (release key bx)
  (assert (= :done (join-by-deadline target))
          "the target must still deliver after its joiner timed out"))

# ── 4. An aborted select waiter leaves the select set ────────────────

(println "abort removes a waiter from its select set...")

(let* [base (selects)
       key (sys/unique)
       bx (box 0)
       candidate (parked-fiber key bx)
       waiter (ev/spawn (fn []
                          (protect (ev/select [candidate]))
                          :selected))]
  (ev/sleep 0.05)
  (assert (= (+ base 1) (selects)) "the waiter must be parked on a select set")
  (ev/abort waiter)
  (assert (= :dead (fiber/status waiter))
          "the waiter's own protect must catch the injected error")
  (assert (= base (selects)) "the dead waiter must leave the select set")
  # The candidate finishes now. With the dead waiter still in the set,
  # this is where the select wake resumes it and the loop raises.
  (release key bx)
  (assert (= :done (join-by-deadline candidate))
          "the candidate must still deliver its value"))

# ── 5. A select over several candidates leaves none of them ──────────

(println "a dead select waiter leaves a set with several candidates...")

(let* [base (selects)
       key (sys/unique)
       bx (box 0)
       first-cand (parked-fiber key bx)
       second-cand (parked-fiber key bx)
       waiter (ev/spawn (fn []
                          (protect (ev/select [first-cand second-cand]))
                          :selected))]
  (ev/sleep 0.05)
  (ev/abort waiter)
  (assert (= base (selects)) "the dead waiter must leave a multi-candidate set")
  (release key bx)
  (assert (= :done (join-by-deadline first-cand))
          "the first candidate must still deliver")
  (assert (= :done (join-by-deadline second-cand))
          "the second candidate must still deliver"))

# ── 6. A waiter killed during the delivery is never reached ──────────
#
# The trap is reentrancy. Resuming one waiter runs that waiter's own
# code before the next waiter is reached, so an `ev/abort` in there
# takes a sibling off the list the delivery is walking.
#
# Each waiter below aborts the other, so whichever the delivery reaches
# first kills the one it has not reached yet. Neither the waiter list
# nor the select set promises an order, and this case needs none.

(println "a joiner killed by the joiner ahead of it is skipped...")

(let* [base (joins)
       key (sys/unique)
       bx (box 0)
       target (parked-fiber key bx)
       both @[]
       joiner (fn [me]
                (ev/spawn (fn []
                            (let [[ok? _] (protect (ev/join-protected target))]
                              (if ok?
                                (begin
                                  (ev/abort (get both (- 1 me)))
                                  :woken)
                                :killed)))))
       a (joiner 0)
       b (joiner 1)]
  (push both a)
  (push both b)
  (ev/sleep 0.05)
  (assert (= (+ base 1) (joins)) "both joiners must be on one waiter list")
  (release key bx)
  (let [outcome [(join-by-deadline a) (join-by-deadline b)]]
    (assert (or (= outcome [:woken :killed]) (= outcome [:killed :woken]))
            (string "one joiner woken, the other killed mid-delivery, got "
                    (string outcome)))))

(println "a select waiter killed by the one ahead of it is skipped...")

(let* [base (selects)
       key (sys/unique)
       bx (box 0)
       candidate (parked-fiber key bx)
       both @[]
       selector (fn [me]
                  (ev/spawn (fn []
                              (let [[ok? _] (protect (ev/select [candidate]))]
                                (if ok?
                                  (begin
                                    (ev/abort (get both (- 1 me)))
                                    :woken)
                                  :killed)))))
       a (selector 0)
       b (selector 1)]
  (push both a)
  (push both b)
  (ev/sleep 0.05)
  (assert (= (+ base 2) (selects)) "both waiters must be parked on a select set")
  (release key bx)
  (let [outcome [(join-by-deadline a) (join-by-deadline b)]]
    (assert (or (= outcome [:woken :killed]) (= outcome [:killed :woken]))
            (string "one select waiter woken, the other killed, got "
                    (string outcome)))))

# ── 7. Repeated kills leave no residue ───────────────────────────────

(println "twenty killed joiners leave the waiter list clean...")

(let* [base (joins)
       key (sys/unique)
       bx (box 0)
       target (parked-fiber key bx)]
  (each i in (range 0 20)
    (assert (nil? (ev/timeout 0.02
                              (fn []
                                (protect (ev/join-protected target))
                                :waited)))
            (string "the deadline must beat join " i)))
  (assert (= base (joins))
          "twenty killed joiners must leave no waiter list behind")
  (release key bx)
  (assert (= :done (join-by-deadline target))
          "the target must still deliver after twenty killed joiners"))

(println "abort-wait-lists: the join list and the select set hold only live waiters")
