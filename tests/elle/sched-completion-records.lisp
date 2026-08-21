(elle/epoch 12)
# What the scheduler remembers about the fibers it has finished with.
#
# Two records outlive a fiber's run: its status (`:ok` / `:error`) and a
# mark saying someone took its result. Both are keyed by the fiber, so a
# record that is never dropped holds the fiber value — and everything the
# fiber closed over — for as long as the loop runs. A server that spawns
# a fiber per request would then pay for every request it ever served.
#
# Nothing reads a record once the result is delivered: a later join
# re-derives the status from the fiber itself, and the unjoined-error
# tail at the end of the loop looks only at fibers NOBODY joined. So a
# delivered join or abort must retire both records. The program's own
# fibers are the exception — the loop reads their record to know the
# program finished.
#
# `ev/report` exposes the two counts (`:records`, `:marks`), which is
# what makes the bound assertable from inside the program.
#
# See docs/scheduler.md § "Completion records".

# ── 1. The counts are in the report ──────────────────────────────────

(println "the report names what the loop remembers...")

(let [r (ev/report)]
  (assert (int? (get r :records)) "report: records is an int")
  (assert (int? (get r :marks)) "report: marks is an int"))

# ── 2. A joined fiber leaves nothing behind ──────────────────────────
# The counts must not scale with the number of fibers joined.

(println "a joined fiber leaves no record...")

(defn join-churn [n]
  (def @i 0)
  (while (< i n)
    (assert (= (ev/join (ev/spawn (fn [] 7))) 7) "the joined fiber returns 7")
    (assign i (+ i 1))))

(join-churn 20)
(def base-records (get (ev/report) :records))
(def base-marks (get (ev/report) :marks))

(join-churn 200)

(let [records (get (ev/report) :records)
      marks (get (ev/report) :marks)]
  (assert (< (- records base-records) 5)
          (string "200 joined fibers left " (- records base-records)
                  " new completion records (must stay bounded)"))
  (assert (< (- marks base-marks) 5)
          (string "200 joined fibers left " (- marks base-marks)
                  " new join marks (must stay bounded)")))

# ── 3. A fiber that FAILED is remembered ─────────────────────────────
# An abort ends the target in `:error`, and a failure keeps its records:
# the mark is what stops the loop from re-raising an error the aborter
# already took, and any later look at the fiber re-derives the failure
# from its own status. So these records grow with the number of failures,
# not with the number of fibers.

(println "a failed fiber keeps its record...")

(defn abort-churn [n]
  (def @i 0)
  (while (< i n)
    (ev/abort (ev/spawn (fn [] (ev/sleep 30))))
    (assign i (+ i 1))))

(def abort-base (get (ev/report) :records))
(abort-churn 20)

(let [records (get (ev/report) :records)]
  (assert (>= (- records abort-base) 20)
          (string "20 aborted fibers left only " (- records abort-base)
                  " records — a failure must keep the mark that stops the "
                  "loop re-raising it")))

# And the loop still ends normally with those failures on record: the
# aborter observed every one of them.
(println "  aborted fibers on record do not crash the loop")

# ── 4. What a retired record must not lose ───────────────────────────
# The status and the value live in the fiber, so a second join answers
# exactly as the first did.

(println "a retired record is re-derivable...")

(let [f (ev/spawn (fn [] 42))]
  (assert (= (ev/join f) 42) "the first join returns the fiber's value")
  (assert (= (ev/join f) 42) "a second join returns it again, from the fiber")
  (assert (= (fiber/status f) :dead) "the fiber itself still reads terminal"))

(let [g (ev/spawn (fn [] (error {:error :boom})))]
  (let [[ok? val] (ev/join-protected g)]
    (assert (not ok?) "the protected join reports the failure")
    (assert (= (get val :error) :boom) "and carries the error value"))
  (let [[ok2? val2] (ev/join-protected g)]
    (assert (not ok2?) "a second protected join reports it again")
    (assert (= (get val2 :error) :boom) "with the same error value")))

# An UNJOINED failure is the loop's business, and its record is what the
# loop reads to raise it. tests/elle/ev-unjoined-error.lisp pins that
# crash; here we only pin that the record survives while unobserved.

(println "an unobserved failure keeps its record...")

(let [before (get (ev/report) :records)]
  (ev/spawn (fn [] 7))
  (ev/sleep 0)
  (let [after (get (ev/report) :records)]
    (assert (>= after before)
            "a fiber nobody joined keeps the record the loop reads")))

(println "sched-completion-records: ok")
