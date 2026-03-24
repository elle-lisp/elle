# I/O — stream primitives, sync scheduler, ev/spawn, async backend


# === Type predicates ===

(assert (not (io-request? 42)) "io-request? on int")
(assert (not (io-request? "hello")) "io-request? on string")
(assert (io-backend? (io/backend :sync)) "io-backend? on sync backend")
(assert (not (io-backend? 42)) "io-backend? on int")

# === Scheduler parameter ===

(assert (parameter? *spawn*) "*spawn* is a parameter")
# In the async-first model, user code runs under the async scheduler.
# The sync-scheduler is still available but not the default for user code.
(assert (fn? (*spawn*)) "*spawn* is bound to a function")

# === sync-scheduler with pure fiber ===

(assert (= (sync-scheduler (fiber/new (fn [] (+ 1 2)) (bit/or 1 512))) 3) "sync-scheduler runs pure fiber")

# === sync-scheduler with I/O ===

(spit "/tmp/elle-test-io-lisp" "hello from io test")
(assert (= (sync-scheduler
    (fiber/new
      (fn [] (port/read-all (port/open "/tmp/elle-test-io-lisp" :read)))
      (bit/or 1 512))) "hello from io test") "sync-scheduler dispatches port/read-all")

# === ev/spawn returns fiber ===

(assert (fiber? (ev/spawn (fn [] 42))) "ev/spawn returns a fiber")

# === ev/spawn with I/O (result collected via mutable) ===

(spit "/tmp/elle-test-ev-spawn-lisp" "spawn content")
(let ((result @[]))
  (ev/spawn (fn []
    (push result (port/read-all (port/open "/tmp/elle-test-ev-spawn-lisp" :read)))))
  # Pump happens naturally since we're inside ev/run; spawned fiber runs before user code returns.
  )

# === Error propagation ===

(let (([ok? _] (protect ((fn () (sync-scheduler (fiber/new (fn [] (error :boom)) (bit/or 1 512)))))))) (assert (not ok?) "sync-scheduler propagates errors"))

# ev/spawn errors propagate when the scheduler pump drains the fiber
(let (([ok? _] (protect ((fn () (ev/run (fn [] (ev/spawn (fn [] (error :kaboom))))))))))
  (assert (not ok?) "ev/spawn propagates errors via ev/run"))

# === port/read-line ===

(spit "/tmp/elle-test-readline-lisp" "line1\nline2\nline3")
(let ((result @[]))
  (ev/run (fn []
    (push result (let ((p (port/open "/tmp/elle-test-readline-lisp" :read)))
      (port/read-line p)))))
  (assert (= (get result 0) "line1") "port/read-line reads first line"))

# === io/backend errors ===

(let (([ok? _] (protect ((fn () (io/backend :invalid)))))) (assert (not ok?) "io/backend :invalid errors"))

# === io/execute roundtrip ===

(spit "/tmp/elle-test-io-exec-lisp" "hello from elle")
(let* ((backend (io/backend :sync))
       (port (port/open "/tmp/elle-test-io-exec-lisp" :read))
       (f (fiber/new (fn [] (port/read-all port)) 512)))
  (fiber/resume f)
  (assert (= (io/execute backend (fiber/value f)) "hello from elle") "io/execute roundtrip reads file"))

# === sync-scheduler I/O dispatch ===

(spit "/tmp/elle-test-sched-io-lisp" "scheduler test")
(assert (= (sync-scheduler
    (fiber/new
      (fn [] (port/read-all (port/open "/tmp/elle-test-sched-io-lisp" :read)))
      (bit/or 1 512))) "scheduler test") "sync-scheduler dispatches I/O")

# === Pure code unchanged with scheduler ===

(assert (= (+ 1 2 3) 6) "pure code works with scheduler")

# === stream I/O via ev/run ===

(spit "/tmp/elle-test-toplevel-io-lisp" "top level")
(let ((result @[]))
  (ev/run (fn []
    (push result (port/read-all (port/open "/tmp/elle-test-toplevel-io-lisp" :read)))))
  (assert (= (get result 0) "top level") "stream I/O via ev/run"))

# === stdlib functions work with scheduler ===

(assert (= (map (fn [x] (* x x)) (list 1 2 3)) (list 1 4 9)) "stdlib map works with scheduler")

# === Async backend ===

(assert (io-backend? (io/backend :async)) "io-backend? on async backend")

# === io/submit returns int ===

(spit "/tmp/elle-test-submit-lisp" "test")
(let* ((backend (io/backend :async))
       (port (port/open "/tmp/elle-test-submit-lisp" :read))
       (f (fiber/new (fn [] (port/read-all port)) 512)))
  (fiber/resume f)
  (assert (int? (io/submit backend (fiber/value f))) "io/submit returns int"))

# === io/reap returns tuple ===

(assert (array? (io/reap (io/backend :async))) "io/reap returns tuple")

# === io/wait returns tuple ===

(assert (array? (io/wait (io/backend :async) 0)) "io/wait returns tuple")

# === io/submit on sync backend errors ===

# port/open must be opened BEFORE the assert-err lambda so it doesn't yield
# inside protect's fiber (protect uses mask=1 which doesn't handle SIG_IO).
(spit "/tmp/elle-test-submit-sync-lisp" "test")
(let ((submit-sync-port (port/open "/tmp/elle-test-submit-sync-lisp" :read)))
  (let (([ok? _] (protect ((fn ()
      (let* ((backend (io/backend :sync))
             (f (fiber/new (fn [] (port/read-all submit-sync-port)) 512)))
        (fiber/resume f)
        (io/submit backend (fiber/value f)))))))) (assert (not ok?) "io/submit on sync backend errors")))

# === io/submit + io/wait roundtrip ===

(spit "/tmp/elle-test-submit-wait-lisp" "roundtrip")
(let* ((backend (io/backend :async))
       (port (port/open "/tmp/elle-test-submit-wait-lisp" :read))
       (f (fiber/new (fn [] (port/read-all port)) 512)))
  (fiber/resume f)
  (let ((id (io/submit backend (fiber/value f))))
    (let ((completions (io/wait backend -1)))
      (assert (= (length completions) 1) "io/wait returns 1 completion"))))

# === Completion struct has :id ===

(spit "/tmp/elle-test-comp-id-lisp" "test")
(let* ((backend (io/backend :async))
       (port (port/open "/tmp/elle-test-comp-id-lisp" :read))
       (f (fiber/new (fn [] (port/read-all port)) 512)))
  (fiber/resume f)
  (let ((id (io/submit backend (fiber/value f))))
    (let ((completions (io/wait backend -1)))
      (assert (= id (get (get completions 0) :id)) "completion :id matches submission id"))))

# === Completion struct has :error nil ===

(spit "/tmp/elle-test-comp-val-lisp" "hello async")
(let* ((backend (io/backend :async))
       (port (port/open "/tmp/elle-test-comp-val-lisp" :read))
       (f (fiber/new (fn [] (port/read-all port)) 512)))
  (fiber/resume f)
  (let ((id (io/submit backend (fiber/value f))))
    (let ((completions (io/wait backend -1)))
      (assert (nil? (get (get completions 0) :error)) "completion :error is nil on success"))))

# === make-async-scheduler ===

(assert (struct? (make-async-scheduler)) "make-async-scheduler returns struct")

# === ev/run pure thunk ===

(assert (nil? (ev/run (fn [] 42))) "ev/run pure thunk returns nil")

# === ev/run I/O thunk ===

(spit "/tmp/elle-test-ev-run-io-lisp" "async scheduler")
(let ((result @[]))
  (ev/run
    (fn []
      (push result (port/read-all (port/open "/tmp/elle-test-ev-run-io-lisp" :read)))))
  (assert (= (get result 0) "async scheduler") "ev/run I/O thunk reads file"))

# === ev/run multiple thunks ===

(spit "/tmp/elle-test-ev-multi-1-lisp" "first")
(spit "/tmp/elle-test-ev-multi-2-lisp" "second")
(let ((results @[]))
  (ev/run
    (fn []
      (push results (port/read-all (port/open "/tmp/elle-test-ev-multi-1-lisp" :read))))
    (fn []
      (push results (port/read-all (port/open "/tmp/elle-test-ev-multi-2-lisp" :read)))))
  (assert (= (length results) 2) "ev/run runs multiple thunks"))

# === ev/run error propagation ===

(let (([ok? _] (protect ((fn () (ev/run (fn [] (error :async-boom)))))))) (assert (not ok?) "ev/run propagates errors"))

# === ev/run write thunk ===

(ev/run
  (fn []
    (let ((p (port/open "/tmp/elle-test-ev-write-lisp" :write)))
      (port/write p "async write test")
      (port/flush p))))
(assert (= (slurp "/tmp/elle-test-ev-write-lisp") "async write test") "ev/run write thunk")

# ============================================================================
# ev/sleep tests
# ============================================================================

# === ev/sleep basic — returns nil ===

(let ((result @[]))
  (ev/run (fn []
    (push result (ev/sleep 0))
    (push result :done)))
  (assert (= (get result 0) nil) "ev/sleep returns nil")
  (assert (= (get result 1) :done) "code after ev/sleep runs"))

# === ev/sleep with nonzero duration ===

(let ((result @[]))
  (ev/run (fn []
    (ev/sleep 0.05)
    (push result :woke)))
  (assert (= (get result 0) :woke) "ev/sleep 50ms completes"))

# === concurrent sleeps run in parallel ===

(let ((t0 (clock/monotonic)))
  (ev/run
    (fn [] (ev/sleep 0.1))
    (fn [] (ev/sleep 0.1))
    (fn [] (ev/sleep 0.1)))
  (let ((elapsed (- (clock/monotonic) t0)))
    (assert (< elapsed 0.5) "3 concurrent 100ms sleeps complete in <500ms (parallel)")))

# === ev/sleep interleaved with I/O ===

(spit "/tmp/elle-test-sleep-io-lisp" "sleep-and-io")
(let ((result @[]))
  (ev/run
    (fn []
      (ev/sleep 0.01)
      (push result :slept))
    (fn []
      (push result (port/read-all (port/open "/tmp/elle-test-sleep-io-lisp" :read)))))
  (assert (= (length result) 2) "ev/sleep + I/O: both fibers complete")
  (assert (any? (fn [x] (= x :slept)) result) "ev/sleep fiber completed")
  (assert (any? (fn [x] (= x "sleep-and-io")) result) "I/O fiber completed"))

# === ev/sleep ordering — shorter sleep finishes first ===

(let ((result @[]))
  (ev/run
    (fn []
      (ev/sleep 0.1)
      (push result :slow))
    (fn []
      (ev/sleep 0.01)
      (push result :fast)))
  (assert (= (get result 0) :fast) "shorter sleep finishes first")
  (assert (= (get result 1) :slow) "longer sleep finishes second"))

# === ev/sleep error: negative duration ===
# No need for nested ev/run — user code already runs in async scheduler.

(let (([ok? _] (protect (ev/sleep -1)))) (assert (not ok?) "ev/sleep rejects negative int"))

(let (([ok? _] (protect (ev/sleep -0.5)))) (assert (not ok?) "ev/sleep rejects negative float"))

# === ev/sleep error: non-numeric ===

(let (([ok? _] (protect (ev/sleep "hello")))) (assert (not ok?) "ev/sleep rejects non-numeric"))

# === ev/sleep error: wrong arity ===

(let (([ok? _] (protect ((fn () (eval '(ev/sleep))))))) (assert (not ok?) "ev/sleep rejects zero args"))

(let (([ok? _] (protect ((fn () (eval '(ev/sleep 1 2))))))) (assert (not ok?) "ev/sleep rejects two args"))

# ============================================================================
# Error tests (from integration/io.rs)
# ============================================================================

# stream_write_outside_scheduler_errors — SKIPPED
# SIG_IO propagates as an uncatchable signal outside a scheduler.
# This is testable from Rust (eval_source catches all signals) but not from Elle.

# stream_write_non_port_errors — SKIPPED
# Same issue: port/write yields SIG_IO before type checking the port argument.
