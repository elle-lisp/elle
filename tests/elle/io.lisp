# I/O — stream primitives, sync scheduler, ev/spawn

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
