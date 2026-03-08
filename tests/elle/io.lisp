# I/O — stream primitives, sync scheduler, ev/spawn, async backend

(import-file "tests/elle/assert.lisp")

# === Type predicates ===

(assert-false (io-request? 42) "io-request? on int")
(assert-false (io-request? "hello") "io-request? on string")
(assert-true (io-backend? (io/backend :sync)) "io-backend? on sync backend")
(assert-false (io-backend? 42) "io-backend? on int")

# === Scheduler parameter ===

(assert-true (parameter? *scheduler*) "*scheduler* is a parameter")
(assert-eq (*scheduler*) sync-scheduler "*scheduler* default is sync-scheduler")

# === sync-scheduler with pure fiber ===

(assert-eq
  (sync-scheduler (fiber/new (fn [] (+ 1 2)) (bit/or 1 512)))
  3
  "sync-scheduler runs pure fiber")

# === sync-scheduler with I/O ===

(spit "/tmp/elle-test-io-lisp" "hello from io test")
(assert-eq
  (sync-scheduler
    (fiber/new
      (fn [] (stream/read-all (port/open "/tmp/elle-test-io-lisp" :read)))
      (bit/or 1 512)))
  "hello from io test"
  "sync-scheduler dispatches stream/read-all")

# === ev/spawn pure ===

(assert-eq (ev/spawn (fn [] 42)) 42 "ev/spawn pure closure")

# === ev/spawn with I/O ===

(spit "/tmp/elle-test-ev-spawn-lisp" "spawn content")
(assert-eq
  (ev/spawn (fn []
    (stream/read-all (port/open "/tmp/elle-test-ev-spawn-lisp" :read))))
  "spawn content"
  "ev/spawn with stream/read-all")

# === Error propagation ===

(assert-err
  (fn () (sync-scheduler (fiber/new (fn [] (error :boom)) (bit/or 1 512))))
  "sync-scheduler propagates errors")

(assert-err
  (fn () (ev/spawn (fn [] (error :kaboom))))
  "ev/spawn propagates errors")

# === stream/read-line ===

(spit "/tmp/elle-test-readline-lisp" "line1\nline2\nline3")
(assert-eq
  (ev/spawn (fn []
    (let ((p (port/open "/tmp/elle-test-readline-lisp" :read)))
      (stream/read-line p))))
  "line1"
  "stream/read-line reads first line")

# === io/backend errors ===

(assert-err
  (fn () (io/backend :invalid))
  "io/backend :invalid errors")

# === io/execute roundtrip ===

(spit "/tmp/elle-test-io-exec-lisp" "hello from elle")
(let* ((backend (io/backend :sync))
       (port (port/open "/tmp/elle-test-io-exec-lisp" :read))
       (f (fiber/new (fn [] (stream/read-all port)) 512)))
  (fiber/resume f)
  (assert-eq
    (io/execute backend (fiber/value f))
    "hello from elle"
    "io/execute roundtrip reads file"))

# === sync-scheduler I/O dispatch ===

(spit "/tmp/elle-test-sched-io-lisp" "scheduler test")
(assert-eq
  (sync-scheduler
    (fiber/new
      (fn [] (stream/read-all (port/open "/tmp/elle-test-sched-io-lisp" :read)))
      (bit/or 1 512)))
  "scheduler test"
  "sync-scheduler dispatches I/O")

# === Pure code unchanged with scheduler ===

(assert-eq (+ 1 2 3) 6 "pure code works with scheduler")

# === stream I/O via ev/spawn ===

(spit "/tmp/elle-test-toplevel-io-lisp" "top level")
(assert-eq
  (ev/spawn (fn []
    (stream/read-all (port/open "/tmp/elle-test-toplevel-io-lisp" :read))))
  "top level"
  "stream I/O via ev/spawn")

# === stdlib functions work with scheduler ===

(assert-eq
  (map (fn [x] (* x x)) (list 1 2 3))
  (list 1 4 9)
  "stdlib map works with scheduler")

# === Async backend ===

(assert-true (io-backend? (io/backend :async)) "io-backend? on async backend")

# === io/submit returns int ===

(spit "/tmp/elle-test-submit-lisp" "test")
(let* ((backend (io/backend :async))
       (port (port/open "/tmp/elle-test-submit-lisp" :read))
       (f (fiber/new (fn [] (stream/read-all port)) 512)))
  (fiber/resume f)
  (assert-true
    (int? (io/submit backend (fiber/value f)))
    "io/submit returns int"))

# === io/reap returns tuple ===

(assert-true
  (tuple? (io/reap (io/backend :async)))
  "io/reap returns tuple")

# === io/wait returns tuple ===

(assert-true
  (tuple? (io/wait (io/backend :async) 0))
  "io/wait returns tuple")

# === io/submit on sync backend errors ===

(assert-err
  (fn ()
    (spit "/tmp/elle-test-submit-sync-lisp" "test")
    (let* ((backend (io/backend :sync))
           (port (port/open "/tmp/elle-test-submit-sync-lisp" :read))
           (f (fiber/new (fn [] (stream/read-all port)) 512)))
      (fiber/resume f)
      (io/submit backend (fiber/value f))))
  "io/submit on sync backend errors")

# === io/submit + io/wait roundtrip ===

(spit "/tmp/elle-test-submit-wait-lisp" "roundtrip")
(let* ((backend (io/backend :async))
       (port (port/open "/tmp/elle-test-submit-wait-lisp" :read))
       (f (fiber/new (fn [] (stream/read-all port)) 512)))
  (fiber/resume f)
  (let ((id (io/submit backend (fiber/value f))))
    (let ((completions (io/wait backend -1)))
      (assert-eq (length completions) 1 "io/wait returns 1 completion"))))

# === Completion struct has :id ===

(spit "/tmp/elle-test-comp-id-lisp" "test")
(let* ((backend (io/backend :async))
       (port (port/open "/tmp/elle-test-comp-id-lisp" :read))
       (f (fiber/new (fn [] (stream/read-all port)) 512)))
  (fiber/resume f)
  (let ((id (io/submit backend (fiber/value f))))
    (let ((completions (io/wait backend -1)))
      (assert-eq id (get (get completions 0) :id) "completion :id matches submission id"))))

# === Completion struct has :error nil ===

(spit "/tmp/elle-test-comp-val-lisp" "hello async")
(let* ((backend (io/backend :async))
       (port (port/open "/tmp/elle-test-comp-val-lisp" :read))
       (f (fiber/new (fn [] (stream/read-all port)) 512)))
  (fiber/resume f)
  (let ((id (io/submit backend (fiber/value f))))
    (let ((completions (io/wait backend -1)))
      (assert-true (nil? (get (get completions 0) :error)) "completion :error is nil on success"))))

# === make-async-scheduler ===

(assert-true (pair? (make-async-scheduler)) "make-async-scheduler returns pair")

# === ev/run pure thunk ===

(assert-true (nil? (ev/run (fn [] 42))) "ev/run pure thunk returns nil")

# === ev/run I/O thunk ===

(spit "/tmp/elle-test-ev-run-io-lisp" "async scheduler")
(let ((result @[]))
  (ev/run
    (fn []
      (push result (stream/read-all (port/open "/tmp/elle-test-ev-run-io-lisp" :read)))))
  (assert-eq (get result 0) "async scheduler" "ev/run I/O thunk reads file"))

# === ev/run multiple thunks ===

(spit "/tmp/elle-test-ev-multi-1-lisp" "first")
(spit "/tmp/elle-test-ev-multi-2-lisp" "second")
(let ((results @[]))
  (ev/run
    (fn []
      (push results (stream/read-all (port/open "/tmp/elle-test-ev-multi-1-lisp" :read))))
    (fn []
      (push results (stream/read-all (port/open "/tmp/elle-test-ev-multi-2-lisp" :read)))))
  (assert-eq (length results) 2 "ev/run runs multiple thunks"))

# === ev/run error propagation ===

(assert-err
  (fn () (ev/run (fn [] (error :async-boom))))
  "ev/run propagates errors")

# === ev/run write thunk ===

(ev/run
  (fn []
    (let ((p (port/open "/tmp/elle-test-ev-write-lisp" :write)))
      (stream/write p "async write test")
      (stream/flush p))))
(assert-eq (slurp "/tmp/elle-test-ev-write-lisp") "async write test" "ev/run write thunk")

# ============================================================================
# Error tests (from integration/io.rs)
# ============================================================================

# stream_write_outside_scheduler_errors — SKIPPED
# SIG_IO propagates as an uncatchable signal outside a scheduler.
# This is testable from Rust (eval_source catches all signals) but not from Elle.

# stream_write_non_port_errors — SKIPPED
# Same issue: stream/write yields SIG_IO before type checking the port argument.
