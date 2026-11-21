(elle/epoch 18)
# audited: 2026-11-20
# A fiber joined to an orphan that ev/run reaps at its end, an importer among them, raises nothing out of ev/run.
# docs/scheduler.md
#
# ev-run-teardown.lisp holds the orphans themselves. Each orphan here waits on
# a timer or a futex far past the program's end, so teardown aborts it with
# {:error :shutdown}, and its joiner resumes with that error and raises it.

(defn reaped-cleanly [label joiner thunk]
  "Run `thunk` as a fiber the program never awaits, and check that ev/run
   returns the program's own value once teardown reaps it. `joiner` names
   the fiber whose :shutdown must stay out of the program."
  (let* [outcome (protect (ev/timeout 5.0
                                      (fn []
                                        (ev/run
                                          (fn []
                                            (ev/spawn thunk)
                                            (ev/sleep 0.02)
                                            :program-done)))))
         [ok? result] outcome]
    (assert ok?
            (concat label "a: " joiner " must not re-raise the injected "
                    ":shutdown; ev/run errored with " (string result)))
    (assert (= result :program-done)
            (concat label "b: ev/run must return its thunk's value; got "
                    (string result)))))

(defn sleeper []
  "Sleep far past the program's end."
  (ev/sleep 10.0)
  :late)

(defn parked []
  "Park on a futex nobody wakes."
  (ev/futex-wait (sys/unique) (box 0) 0)
  :late)

## ── 1. A fiber joined to a reaped orphan ──────────────────────────────────
##
## A fiber the program never awaits joins the orphan. The counter-factual
## records the orphan alone as scheduler-killed, so ev/run crashes on the
## joiner's error.

(reaped-cleanly "1" "a fiber joined to a reaped orphan"
                (fn [] (ev/join (ev/spawn sleeper))))

(println "  PASS: a joiner of a reaped orphan raises nothing")

## ── 2. An import whose loader is reaped ───────────────────────────────────
##
## An import joins its importer to a loader fiber, so a loader parked past
## the program's end is an orphan like the one in test 1, and its importer is
## that orphan's joiner.

(reaped-cleanly "2" "an importer whose loader teardown reaped"
                (fn []
                  (parameterize ((*loader* (fn [_] (sleeper))))
                    (import "x"))))

(println "  PASS: an importer of a reaped loader raises nothing")

## ── 3. A join that arrives after the orphan is reaped ─────────────────────
##
## The joiner waits in a select over a futex-parked orphan, so teardown
## reaps the orphan first and wakes the joiner, which then joins a fiber
## already ended. The counter-factual marks a joiner only when it is woken
## from the orphan's waiter list, so ev/run crashes on this late joiner.

(reaped-cleanly "3" "a late joiner of a reaped orphan"
                (fn []
                  (let [orphan (ev/spawn parked)]
                    (ev/select [orphan])
                    (ev/join orphan))))

(println "  PASS: a late joiner of a reaped orphan raises nothing")

## ── 4. An ev/timeout whose parked body is reaped ──────────────────────────
##
## ev/timeout is the shape of test 3: teardown reaps the parked body while
## the timer's sleep has yet to answer its cancel, and ev/timeout joins the
## body that woke its select.

(reaped-cleanly "4" "an ev/timeout whose parked body was reaped"
                (fn [] (ev/timeout 30 parked)))

(println "  PASS: an ev/timeout whose body was reaped raises nothing")

(println "ev-run-teardown-join: all tests passed")
