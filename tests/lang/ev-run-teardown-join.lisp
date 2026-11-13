(elle/epoch 16)
# audited: 2026-11-13
# A fiber joined to an orphan that ev/run reaps at its end, an importer among them, raises nothing out of ev/run.
# docs/scheduler.md
#
# ev-run-teardown.lisp holds the orphans themselves. Each orphan here parks on
# a timer far past the program's end, so teardown aborts it with
# {:error :shutdown}, and its joiner resumes with that error and raises it.

## ── 1. A fiber joined to a reaped orphan ──────────────────────────────────
##
## A second fiber the program never awaits joins the orphan. The
## counter-factual records the orphan alone as scheduler-killed, so ev/run
## crashes on the joiner's error.

(let* [outcome (protect (ev/timeout 5.0
                                    (fn []
                                      (ev/run
                                        (fn []
                                          (let [orphan (ev/spawn
                                                         (fn []
                                                           (ev/sleep 10.0)
                                                           :late))]
                                            (ev/spawn (fn [] (ev/join orphan))))
                                          (ev/sleep 0.02)
                                          :program-done)))))
       [ok? result] outcome]
  (assert ok?
          (concat "1a: a fiber joined to a reaped orphan must not re-raise "
                  "the orphan's injected :shutdown; ev/run errored with "
                  (string result)))
  (assert (= result :program-done)
          (concat "1b: ev/run must return its thunk's value; got "
                  (string result))))

(println "  PASS: a joiner of a reaped orphan raises nothing")

## ── 2. An import whose loader is reaped ───────────────────────────────────
##
## An import joins its importer to a loader fiber, so a loader parked past
## the program's end is an orphan like the one in test 1, and its importer is
## that orphan's joiner.

(let* [outcome (protect (ev/timeout 5.0
                                    (fn []
                                      (ev/run
                                        (fn []
                                          (parameterize ((*loader*
                                                           (fn [_]
                                                             (ev/sleep 10.0)
                                                             :late)))
                                            (ev/spawn (fn [] (import "x"))))
                                          (ev/sleep 0.02)
                                          :program-done)))))
       [ok? result] outcome]
  (assert ok?
          (concat "2a: an importer whose loader teardown reaped must not "
                  "re-raise the injected :shutdown; ev/run errored with "
                  (string result)))
  (assert (= result :program-done)
          (concat "2b: ev/run must return its thunk's value; got "
                  (string result))))

(println "  PASS: an importer of a reaped loader raises nothing")

(println "ev-run-teardown-join: all tests passed")
