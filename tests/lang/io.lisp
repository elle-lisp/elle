(elle/epoch 12)
# I/O — stream primitives, ev/spawn, async backend
#
# Scratch files live under the platform temp root: each temp-using section wraps
# its file lifecycle in (with-temp-dir dir …), which binds a unique dir from
# file/mktempdir (honors TMPDIR) and deletes the tree after — even on failure. No
# hardcoded paths, no litter. Create+use+delete stay inside one thunk so the file
# is self-contained per tier under `elle test`.


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

(with-temp-dir dir
               (let [fpath (path/join dir "ev-spawn")]
                 (spit fpath "spawn content")
                 (let [result @[]]
                   # ev/join so the read completes inside the with-temp-dir body — the defer
                   # cleanup would otherwise race the spawned fiber and delete the file first.
                   (ev/join (ev/spawn (fn []
                                        (push result
                                        (port/read-all (port/open fpath :read)))))))))

# === Error propagation ===

# ev/spawn errors propagate via ev/join
(let [[ok? _] (protect (ev/join (ev/spawn (fn [] (error :kaboom)))))]
  (assert (not ok?) "ev/spawn propagates errors via ev/join"))

# === port/read-line ===

(with-temp-dir dir
               (let [fpath (path/join dir "readline")]
                 (spit fpath "line1\nline2\nline3")
                 (let [line (let [p (port/open fpath :read)]
                              (port/read-line p))]
                   (assert (= line "line1") "port/read-line reads first line"))))

# === a recycled descriptor number carries no remainder ===
# A read that overshoots leaves the rest of the block buffered for the next
# read on the same descriptor: `port/read-line` stops at the newline and holds
# what it read past it. The descriptor number goes back to the OS as soon as
# the port's descriptor closes — which a port dropped rather than `port/close`d
# still does — so the next `port/open` can be handed that very number, and it
# must read its own file and nothing else.

(with-temp-dir dir
               (let [over (path/join dir "overshoot")
                     fresh (path/join dir "fresh")]
                 (spit over "line1\nline2\nline3")
                 (spit fresh "fresh contents")
                 (assert (= (port/read-line (port/open over :read)) "line1")
                         "port/read-line stops at the first newline")
                 (assert (= (string (port/read-all (port/open fresh :read)))
                            "fresh contents")
                         "a port on a recycled descriptor number reads only its own file")))

# === io/backend errors ===

(let [[ok? _] (protect ((fn () (io/backend :invalid))))]
  (assert (not ok?) "io/backend :invalid errors"))

# === worker keepalive ===
#
# The second argument is how long an idle I/O worker waits for another
# operation before it retires, in seconds; nil takes the runtime's default and
# 0 turns reuse off. Nothing a program can observe distinguishes the crews —
# `io/workers` counts operations, not threads — so what is asserted here is
# that each form is accepted, refused, and still does the I/O it was given.

(assert (parameter? *io-keepalive*) "*io-keepalive* is a parameter")
(assert (nil? (*io-keepalive*)) "*io-keepalive* defaults to the runtime's own")

(assert (io-backend? (io/backend :async nil)) "nil keepalive builds a backend")
(assert (io-backend? (io/backend :async 0)) "a zero keepalive builds a backend")
(assert (io-backend? (io/backend :async 0.25))
        "a fractional keepalive builds a backend")

(let [[ok? _] (protect ((fn () (io/backend :async "soon"))))]
  (assert (not ok?) "a keepalive that is not a number errors"))
(let [[ok? _] (protect ((fn () (io/backend :async -1))))]
  (assert (not ok?) "a negative keepalive errors"))

# A scheduler reads the parameter when it makes its backend, so this whole
# program's I/O runs on a crew that retires every worker at once.
(assert (= "reuse off"
           (parameterize ((*io-keepalive* 0))
             (ev/run (fn []
                       (with-temp-dir dir
                                      (let [fpath (path/join dir "keepalive")]
                                        (spit fpath "reuse off")
                                        (string (port/read-all (port/open fpath
                                        :read)))))))))
        "I/O works with worker reuse turned off")

# === stream I/O ===

(with-temp-dir dir
               (let [fpath (path/join dir "toplevel-io")]
                 (spit fpath "top level")
                 (assert (= (string (port/read-all (port/open fpath :read)))
                            "top level") "stream I/O works")))

# === stdlib functions work with scheduler ===

(assert (= (map (fn [x] (* x x)) (list 1 2 3)) (list 1 4 9))
        "stdlib map works with scheduler")

# === Async backend ===

(assert (io-backend? (io/backend :async)) "io-backend? on async backend")

# === io/submit returns int ===

(with-temp-dir dir
               (let [fpath (path/join dir "submit")]
                 (spit fpath "test")
                 (let* [backend (io/backend :async)
                        port (port/open fpath :read)
                        f (fiber/new (fn [] (port/read-all port)) 512)]
                   (fiber/resume f)
                   (assert (int? (io/submit backend (fiber/value f)))
                           "io/submit returns int"))))

# === io/reap returns tuple ===

(assert (array? (io/reap (io/backend :async))) "io/reap returns tuple")

# === io/wait returns tuple ===

(assert (array? (io/wait (io/backend :async) 0)) "io/wait returns tuple")

# === io/submit on sync backend errors ===

# port/open must be opened BEFORE the assert-err lambda so it doesn't yield
# inside protect's fiber (protect uses mask=1 which doesn't handle SIG_IO).
(with-temp-dir dir
               (let [fpath (path/join dir "submit-sync")]
                 (spit fpath "test")
                 (let [submit-sync-port (port/open fpath :read)]
                   (let [[ok? _] (protect ((fn ()
                           (let* [backend (io/backend :sync)
                                  f (fiber/new (fn []
                                    (port/read-all submit-sync-port)) 512)]
                             (fiber/resume f)
                             (io/submit backend (fiber/value f))))))]
                     (assert (not ok?) "io/submit on sync backend errors")))))

# === io/submit + io/wait roundtrip ===

(with-temp-dir dir
               (let [fpath (path/join dir "submit-wait")]
                 (spit fpath "roundtrip")
                 (let* [backend (io/backend :async)
                        port (port/open fpath :read)
                        f (fiber/new (fn [] (port/read-all port)) 512)]
                   (fiber/resume f)
                   (let [id (io/submit backend (fiber/value f))]
                     (let [completions (io/wait backend -1)]
                       (assert (= (length completions) 1)
                               "io/wait returns 1 completion"))))))

# === Completion struct has :id ===

(with-temp-dir dir
               (let [fpath (path/join dir "comp-id")]
                 (spit fpath "test")
                 (let* [backend (io/backend :async)
                        port (port/open fpath :read)
                        f (fiber/new (fn [] (port/read-all port)) 512)]
                   (fiber/resume f)
                   (let [id (io/submit backend (fiber/value f))]
                     (let [completions (io/wait backend -1)]
                       (assert (= id (get (get completions 0) :id))
                               "completion :id matches submission id"))))))

# === Completion struct has :error nil ===

(with-temp-dir dir
               (let [fpath (path/join dir "comp-val")]
                 (spit fpath "hello async")
                 (let* [backend (io/backend :async)
                        port (port/open fpath :read)
                        f (fiber/new (fn [] (port/read-all port)) 512)]
                   (fiber/resume f)
                   (let [id (io/submit backend (fiber/value f))]
                     (let [completions (io/wait backend -1)]
                       (assert (nil? (get (get completions 0) :error))
                               "completion :error is nil on success"))))))

# === make-async-scheduler ===

(assert (struct? (make-async-scheduler)) "make-async-scheduler returns struct")

# === basic expression evaluation ===

(assert (= 42 42) "pure expression")

# === I/O thunk (direct) ===

(with-temp-dir dir
               (let [fpath (path/join dir "ev-run-io")]
                 (spit fpath "async scheduler")
                 (assert (= (string (port/read-all (port/open fpath :read)))
                            "async scheduler") "I/O thunk reads file")))

# === multiple concurrent fibers ===

(with-temp-dir dir
               (let [f1path (path/join dir "ev-multi-1")
                     f2path (path/join dir "ev-multi-2")]
                 (spit f1path "first")
                 (spit f2path "second")
                 (let [results @[]]
                   (let [f1 (ev/spawn (fn []
                                        (push results
                                        (port/read-all (port/open f1path :read)))))
                         f2 (ev/spawn (fn []
                                        (push results
                                        (port/read-all (port/open f2path :read)))))]
                     (ev/join f1)
                     (ev/join f2))
                   (assert (= (length results) 2)
                           "concurrent fibers both complete"))))

# === error propagation ===

(let [[ok? _] (protect ((fn () (error :async-boom))))]
  (assert (not ok?) "protect captures errors"))

# === async write ===

(with-temp-dir dir
               (let [fpath (path/join dir "ev-write")]
                 (let [p (port/open fpath :write)]
                   (port/write p "async write test")
                   (port/flush p))
                 (assert (= (slurp fpath) "async write test")
                         "async write thunk")))

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
    (assert (< elapsed 0.5)
            "3 concurrent 100ms sleeps complete in <500ms (parallel)")))

# === ev/sleep interleaved with I/O ===

(with-temp-dir dir
               (let [fpath (path/join dir "sleep-io")]
                 (spit fpath "sleep-and-io")
                 (let [result @[]]
                   (let [f1 (ev/spawn (fn []
                                        (ev/sleep 0.01)
                                        (push result :slept)))
                         f2 (ev/spawn (fn []
                                        (push result
                                        (string (port/read-all (port/open fpath
                                        :read))))))]
                     (ev/join f1)
                     (ev/join f2))
                   (assert (= (length result) 2)
                           "ev/sleep + I/O: both fibers complete")
                   (assert (any? (fn [x] (= x :slept)) result)
                           "ev/sleep fiber completed")
                   (assert (any? (fn [x] (= x "sleep-and-io")) result)
                           "I/O fiber completed"))))

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

(let [[ok? _] (protect (ev/sleep -1))]
  (assert (not ok?) "ev/sleep rejects negative int"))

(let [[ok? _] (protect (ev/sleep -0.5))]
  (assert (not ok?) "ev/sleep rejects negative float"))

# === ev/sleep error: non-numeric ===

(let [[ok? _] (protect (ev/sleep "hello"))]
  (assert (not ok?) "ev/sleep rejects non-numeric"))

# === ev/sleep error: wrong arity ===

(let [[ok? _] (protect ((fn () (eval '(ev/sleep)))))]
  (assert (not ok?) "ev/sleep rejects zero args"))

(let [[ok? _] (protect ((fn () (eval '(ev/sleep 1 2)))))]
  (assert (not ok?) "ev/sleep rejects two args"))
# ============================================================================
# Error tests (from integration/io.rs)
# ============================================================================
# stream_write_outside_scheduler_errors — SKIPPED
# SIG_IO propagates as an uncatchable signal outside a scheduler.
# This is testable from Rust (eval_source catches all signals) but not from Elle.
# stream_write_non_port_errors — SKIPPED
# Same issue: port/write yields SIG_IO before type checking the port argument.
