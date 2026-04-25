#!/usr/bin/env elle
(elle/epoch 9)

# Tests for resuming errored fibers — the restarts system.
#
# Fibers in the :error state MUST be resumable. Only :dead fibers are
# terminal. This is the foundation of Elle's restarts: a parent catches
# an error from a child, inspects it, and resumes the child with a
# recovery value.

# ── Test 1: resume an errored fiber with a recovery value ─────────

(println "  1. resume errored fiber with recovery value...")
(let [f (fiber/new (fn []
           (+ 1 (emit :error {:reason :divide-by-zero})))
         |:error|)]
  # First resume starts the fiber; it hits the error and suspends
  (def result (fiber/resume f))
  (assert (= (fiber/status f) :paused)
    "fiber with caught error is :paused")

  # The error was caught by the mask — the fiber is paused, not errored.
  # Resume it with a recovery value.
  (def recovered (fiber/resume f 42))
  (assert (= recovered 43)
    "recovery value flows back into computation"))
(println "  1. ok")

# ── Test 2: uncaught error → :error status, then resume ──────────
#
# When the parent does NOT catch the error (mask doesn't include :error),
# the fiber goes to :error status. Resuming must still work.

(println "  2. resume fiber in :error state...")
(let [inner (fiber/new (fn []
               (+ 1 (emit :error {:reason :oops})))
             |:yield|)]  # mask catches :yield only, NOT :error
  # Wrap in an outer fiber that catches :error
  (let [outer (fiber/new (fn []
                 (fiber/resume inner))
               |:error|)]
    # Resume outer → it resumes inner → inner errors →
    # error propagates through inner (uncaught) to outer (caught)
    (def err-val (fiber/resume outer))
    (assert (= (fiber/status inner) :error)
      "inner fiber is in :error state")
    (assert (= err-val:reason :oops)
      "error value is accessible")

    # Now resume the errored inner fiber with a recovery value
    (def recovered (fiber/resume inner 42))
    (assert (= recovered 43)
      "errored fiber resumes and computes with recovery value")))
(println "  2. ok")

# ── Test 3: resume errored fiber multiple times ───────────────────
#
# A fiber that errors, gets resumed with recovery, errors again,
# and gets resumed again. Each cycle must work.

(println "  3. multiple error-resume cycles...")
(let [f (fiber/new (fn []
           (var count 0)
           (var total 0)
           (while true
             (assign count (+ count 1))
             (let [v (emit :error {:reason :need-input
                                   :attempt count})]
               (assign total (+ total v))
               (when (>= count 3)
                 (break total)))))
         |:error|)]
  # Each resume: fiber emits :error, we catch it, resume with a value
  (def v1 (fiber/resume f))
  (assert (= v1:reason :need-input) "first error caught")

  (def v2 (fiber/resume f 10))
  (assert (= v2:reason :need-input) "second error caught")

  (def v3 (fiber/resume f 20))
  (assert (= v3:reason :need-input) "third error caught")

  (def result (fiber/resume f 30))
  (assert (= result 60) "total is 10+20+30=60"))
(println "  3. ok")

# ── Test 4: fiber/status transitions ──────────────────────────────

(println "  4. status transitions through error-resume...")
(let [f (fiber/new (fn []
           (emit :error {:reason :check})
           :done)
         |:yield|)]  # does NOT catch :error
  (let [outer (fiber/new (fn []
                 (fiber/resume f))
               |:error|)]
    (assert (= (fiber/status f) :new) "starts :new")

    (fiber/resume outer)
    (assert (= (fiber/status f) :error) "after uncaught error: :error")

    # Resume the errored fiber — it should continue past the emit
    (def result (fiber/resume f :recovered))
    (assert (= result :done) "errored fiber resumes to completion")
    (assert (= (fiber/status f) :dead) "after completion: :dead")))
(println "  4. ok")

# ── Test 5: dead fibers remain non-resumable ──────────────────────

(println "  5. dead fibers cannot be resumed...")
(let [f (fiber/new (fn [] 42) |:error|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "fiber is dead")
  (let [[ok? err] (protect (fiber/resume f))]
    (assert (not ok?) "resuming dead fiber errors")
    (assert (= err:message "fiber/resume: cannot resume completed fiber")
      "correct error message")))
(println "  5. ok")

(println "  all fiber-error-resume tests passed")
