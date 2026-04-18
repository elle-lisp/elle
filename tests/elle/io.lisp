(elle/epoch 8)
# I/O — stream primitives, ev/spawn, async backend


# === Type predicates ===

(assert (not (io-request? 42)) "io-request? on int")
(assert (not (io-request? "hello")) "io-request? on string")
(assert (io-backend? (io/backend :async)) "io-backend? on async backend")
(assert (not (io-backend? 42)) "io-backend? on int")

# === Scheduler parameter ===

(assert (parameter? *spawn*) "*spawn* is a parameter")
(assert (fn? (*spawn*)) "*spawn* is bound to a function")

# === ev/spawn returns fiber ===

(assert (fiber? (ev/spawn (fn [] 42))) "ev/spawn returns a fiber")

# === ev/spawn with I/O (result collected via mutable) ===

(spit "/tmp/elle-test-ev-spawn-lisp" "spawn content")
(let [result @[]]
  (ev/spawn (fn []
    (push result (port/read-all (port/open "/tmp/elle-test-ev-spawn-lisp" :read)))))
  # Pump happens naturally; spawned fiber runs before user code returns.
  )

# === Error propagation ===

# ev/spawn errors propagate via ev/join
(let [[ok? _] (protect (ev/join (ev/spawn (fn [] (error :kaboom)))))]
  (assert (not ok?) "ev/spawn propagates errors via ev/join"))

# === port/read-line ===

(spit "/tmp/elle-test-readline-lisp" "line1\nline2\nline3")
(let [line (let [p (port/open "/tmp/elle-test-readline-lisp" :read)]
              (port/read-line p))]
  (assert (= line "line1") "port/read-line reads first line"))

# === io/backend errors ===

(let [[ok? _] (protect ((fn () (io/backend :invalid))))] (assert (not ok?) "io/backend :invalid errors"))

# === stream I/O ===

(spit "/tmp/elle-test-toplevel-io-lisp" "top level")
(assert (= (string (port/read-all (port/open "/tmp/elle-test-toplevel-io-lisp" :read)))
           "top level") "stream I/O works")

# === stdlib functions work with scheduler ===

(assert (= (map (fn [x] (* x x)) (list 1 2 3)) (list 1 4 9)) "stdlib map works with scheduler")

# === Async backend ===

(assert (io-backend? (io/backend :async)) "io-backend? on async backend")

# === io/submit returns int ===

(spit "/tmp/elle-test-submit-lisp" "test")
(let* [backend (io/backend :async)
       port (port/open "/tmp/elle-test-submit-lisp" :read)
       f (fiber/new (fn [] (port/read-all port)) 512)]
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
(let [submit-sync-port (port/open "/tmp/elle-test-submit-sync-lisp" :read)]
  (let [[ok? _] (protect ((fn ()
      (let* [backend (io/backend :sync)
             f (fiber/new (fn [] (port/read-all submit-sync-port)) 512)]
        (fiber/resume f)
        (io/submit backend (fiber/value f))))))] (assert (not ok?) "io/submit on sync backend errors")))

# === io/submit + io/wait roundtrip ===

(spit "/tmp/elle-test-submit-wait-lisp" "roundtrip")
(let* [backend (io/backend :async)
       port (port/open "/tmp/elle-test-submit-wait-lisp" :read)
       f (fiber/new (fn [] (port/read-all port)) 512)]
  (fiber/resume f)
  (let [id (io/submit backend (fiber/value f))]
    (let [completions (io/wait backend -1)]
      (assert (= (length completions) 1) "io/wait returns 1 completion"))))

# === Completion struct has :id ===

(spit "/tmp/elle-test-comp-id-lisp" "test")
(let* [backend (io/backend :async)
       port (port/open "/tmp/elle-test-comp-id-lisp" :read)
       f (fiber/new (fn [] (port/read-all port)) 512)]
  (fiber/resume f)
  (let [id (io/submit backend (fiber/value f))]
    (let [completions (io/wait backend -1)]
      (assert (= id (get (get completions 0) :id)) "completion :id matches submission id"))))

# === Completion struct has :error nil ===

(spit "/tmp/elle-test-comp-val-lisp" "hello async")
(let* [backend (io/backend :async)
       port (port/open "/tmp/elle-test-comp-val-lisp" :read)
       f (fiber/new (fn [] (port/read-all port)) 512)]
  (fiber/resume f)
  (let [id (io/submit backend (fiber/value f))]
    (let [completions (io/wait backend -1)]
      (assert (nil? (get (get completions 0) :error)) "completion :error is nil on success"))))

# === make-async-scheduler ===

(assert (struct? (make-async-scheduler)) "make-async-scheduler returns struct")

# === basic expression evaluation ===

(assert (= 42 42) "pure expression")

# === I/O thunk (direct) ===

(spit "/tmp/elle-test-ev-run-io-lisp" "async scheduler")
(assert (= (string (port/read-all (port/open "/tmp/elle-test-ev-run-io-lisp" :read)))
           "async scheduler") "I/O thunk reads file")

# === multiple concurrent fibers ===

(spit "/tmp/elle-test-ev-multi-1-lisp" "first")
(spit "/tmp/elle-test-ev-multi-2-lisp" "second")
(let [results @[]]
  (let [f1 (ev/spawn (fn []
              (push results (port/read-all (port/open "/tmp/elle-test-ev-multi-1-lisp" :read)))))
        f2 (ev/spawn (fn []
              (push results (port/read-all (port/open "/tmp/elle-test-ev-multi-2-lisp" :read)))))]
    (ev/join f1)
    (ev/join f2))
  (assert (= (length results) 2) "concurrent fibers both complete"))

# === error propagation ===

(let [[ok? _] (protect ((fn () (error :async-boom))))] (assert (not ok?) "protect captures errors"))

# === async write ===

(let [p (port/open "/tmp/elle-test-ev-write-lisp" :write)]
  (port/write p "async write test")
  (port/flush p))
(assert (= (slurp "/tmp/elle-test-ev-write-lisp") "async write test") "async write thunk")

# ============================================================================
# ev/sleep tests
# ============================================================================

# === ev/sleep basic — returns nil ===

(assert (nil? (ev/sleep 0)) "ev/sleep returns nil")

# === ev/sleep with nonzero duration ===

(ev/sleep 0.05)
(assert true "ev/sleep 50ms completes")

# === concurrent sleeps run in parallel ===

(let [t0 (clock/monotonic)]
  (let [f1 (ev/spawn (fn [] (ev/sleep 0.1)))
        f2 (ev/spawn (fn [] (ev/sleep 0.1)))
        f3 (ev/spawn (fn [] (ev/sleep 0.1)))]
    (ev/join f1)
    (ev/join f2)
    (ev/join f3))
  (let [elapsed (- (clock/monotonic) t0)]
    (assert (< elapsed 0.5) "3 concurrent 100ms sleeps complete in <500ms (parallel)")))

# === ev/sleep interleaved with I/O ===

(spit "/tmp/elle-test-sleep-io-lisp" "sleep-and-io")
(let [result @[]]
  (let [f1 (ev/spawn (fn []
              (ev/sleep 0.01)
              (push result :slept)))
        f2 (ev/spawn (fn []
              (push result (string (port/read-all (port/open "/tmp/elle-test-sleep-io-lisp" :read))))))]
    (ev/join f1)
    (ev/join f2))
  (assert (= (length result) 2) "ev/sleep + I/O: both fibers complete")
  (assert (any? (fn [x] (= x :slept)) result) "ev/sleep fiber completed")
  (assert (any? (fn [x] (= x "sleep-and-io")) result) "I/O fiber completed"))

# === ev/sleep ordering — shorter sleep finishes first ===

(let [result @[]]
  (let [f1 (ev/spawn (fn []
              (ev/sleep 0.1)
              (push result :slow)))
        f2 (ev/spawn (fn []
              (ev/sleep 0.01)
              (push result :fast)))]
    (ev/join f1)
    (ev/join f2))
  (assert (= (get result 0) :fast) "shorter sleep finishes first")
  (assert (= (get result 1) :slow) "longer sleep finishes second"))

# === ev/sleep error: negative duration ===
# User code already runs in the async scheduler.

(let [[ok? _] (protect (ev/sleep -1))] (assert (not ok?) "ev/sleep rejects negative int"))

(let [[ok? _] (protect (ev/sleep -0.5))] (assert (not ok?) "ev/sleep rejects negative float"))

# === ev/sleep error: non-numeric ===

(let [[ok? _] (protect (ev/sleep "hello"))] (assert (not ok?) "ev/sleep rejects non-numeric"))

# === ev/sleep error: wrong arity ===

(let [[ok? _] (protect ((fn () (eval '(ev/sleep)))))] (assert (not ok?) "ev/sleep rejects zero args"))

(let [[ok? _] (protect ((fn () (eval '(ev/sleep 1 2)))))] (assert (not ok?) "ev/sleep rejects two args"))

# ============================================================================
# Error tests (from integration/io.rs)
# ============================================================================

# stream_write_outside_scheduler_errors — SKIPPED
# SIG_IO propagates as an uncatchable signal outside a scheduler.
# This is testable from Rust (eval_source catches all signals) but not from Elle.

# stream_write_non_port_errors — SKIPPED
# Same issue: port/write yields SIG_IO before type checking the port argument.
