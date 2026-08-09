(elle/epoch 12)
# A completion that arrives after its fiber is gone.
#
# The scheduler pairs a submission with the fiber that asked for it, and
# resumes that fiber when the completion arrives. The pairing outlives
# the fiber whenever a fiber terminates by a path the scheduler did not
# route. `fiber/abort` is such a path: it injects an error the fiber's
# own `protect` may catch, so the fiber runs to `:dead` while its timer
# is still in flight. `fiber/cancel` is another: it leaves the fiber in
# `:error` with the same operation outstanding.
#
# Delivering to such a fiber raises `fiber/resume: cannot resume
# completed fiber` out of the event loop. That error reaches the program
# at whatever line the loop happened to be pumping from, with nothing in
# it naming the fiber that died — so the cases below hold a completion in
# flight across the teardown on purpose, and assert that the loop stays
# quiet and keeps the fiber's result.
#
# The reverse direction matters too: once the scheduler does learn that
# the fiber finished, the operation it was waiting on has no reader, and
# holding it keeps a worker and a descriptor out for the life of the
# loop. `:io` in `ev/report` is what makes that visible.
#
# See docs/scheduler.md § "Completion delivery".

# A deadline no operation here can reach.
(def deadline 5)

# Long enough that the victim is parked in its operation, short enough to
# keep the file quick.
(def settle 0.05)

# The timer the victim parks on, and a wait that outlives it.
(def in-flight 0.2)
(def outlive 0.4)

(defn report-after [seconds]
  "Report from a watchdog fiber that wakes after `seconds`. The
   watchdog's own timer has completed by the time it reports, so it adds
   nothing to the counts."
  (ev/join (ev/spawn (fn []
                       (ev/sleep seconds)
                       (ev/report)))))

# ── 1. A fiber that caught its abort keeps its value ─────────────────

(println "a completion for a fiber that finished is dropped...")

(let [victim (ev/spawn (fn []
                         (protect (ev/sleep in-flight))
                         :caught))]
  (ev/sleep settle)
  (assert (= (fiber/status victim) :paused) "the victim is parked in its timer")
  (protect (fiber/abort victim {:error :external}))
  (assert (= (fiber/status victim) :dead)
          "the victim caught the abort and ran to completion")
  # Nothing told the scheduler the victim is gone, so the timer is still
  # submitted. Outlive it: its completion arrives with the loop pumping.
  (ev/sleep outlive)
  (assert (= (ev/join victim) :caught)
          "the victim's value survives its own late completion"))

# ── 2. A hard-killed fiber reports failure, not a loop error ─────────

(println "a completion for a hard-killed fiber is dropped...")

(let [victim (ev/spawn (fn []
                         (ev/sleep in-flight)
                         :never))]
  (ev/sleep settle)
  (protect (fiber/cancel victim {:error :external}))
  (assert (= (fiber/status victim) :error)
          "the hard kill left the victim in :error")
  (ev/sleep outlive)
  (let [[ok? _] (ev/join-protected victim)]
    (assert (not ok?) "the killed victim joins as a failure")))

# ── 3. A completion that arrives after the join is dropped ───────────

(println "a completion for an already-joined fiber is dropped...")

(let [victim (ev/spawn (fn []
                         (protect (ev/sleep in-flight))
                         :caught))]
  (ev/sleep settle)
  (protect (fiber/abort victim {:error :external}))
  # The join is what tells the scheduler the victim finished.
  (assert (= (ev/join victim) :caught) "the join reports the victim's value")
  (ev/sleep outlive)
  (assert (= (fiber/status victim) :dead) "the victim stayed finished"))

# ── 4. A finished fiber holds no operation ───────────────────────────

(println "the operation of a finished fiber is released...")

(let* [baseline (get (report-after settle) :io)
       victim (ev/spawn (fn []
                          (protect (ev/sleep 30))
                          :caught))]
  (ev/sleep settle)
  (protect (fiber/abort victim {:error :external}))
  (assert (= (ev/join victim) :caught) "the join reports the victim's value")
  (let [outstanding (get (report-after settle) :io)]
    (assert (= outstanding baseline)
            (string "the victim's timer is no longer outstanding: "
                    (string outstanding) " against a baseline of "
                    (string baseline)))))

# ── 5. The loop still ends ───────────────────────────────────────────

(println "the loop finishes with a dropped completion behind it...")

(let [victim (ev/spawn (fn []
                         (protect (ev/sleep in-flight))
                         :caught))]
  (ev/sleep settle)
  (protect (fiber/abort victim {:error :external}))
  (assert (not (nil? (ev/timeout deadline
                                 (fn []
                                   (ev/sleep outlive)
                                   :ran))))
          "the loop kept pumping past the dropped completion"))

(println "io late completion: every completion found a live reader or none")
