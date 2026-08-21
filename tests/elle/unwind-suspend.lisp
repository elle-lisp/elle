(elle/epoch 12)
## tests/elle/unwind-suspend.lisp — what runs, and in what order, when a
## body catches an error around async I/O.
##
## `protect`, `try` and `defer` each run their body in a fiber and drive it
## with one `fiber/resume`. A body that waits on the scheduler sends its
## I/O request out through that fiber, so the completion comes back to a
## fiber suspended one or more levels down. The body owns the continuation
## either way: the forms after the capture run before any enclosing
## cleanup, and the enclosing form continues after that.
##
## Every case needs both halves — a body that suspends AND an error —
## because the completion is what re-enters the chain. A body that errors
## without suspending never leaves this thread, and one that suspends and
## succeeds never takes the error path. The last section pins those two on
## their own, so a fix for the pair cannot cost either of them.
##
## See docs/errors.md § "Cleanup around async I/O" and
## docs/signals/primitives.md § "Unwinding that suspends".

(println "tests/elle/unwind-suspend.lisp:")

(def listener (tcp/listen "127.0.0.1" 0))

(defn timed-out []
  "An async call that must fail: nothing ever connects to this listener."
  (tcp/accept listener :timeout 20))

(defn suspend []
  "Give the scheduler a turn, the way any I/O in a body or a cleanup does."
  (ev/sleep 0))

## ── 1. protect inside a defer body ──────────────────────────────────

(let [order @[]]
  (let [result (defer
                 (push order :cleanup)
                 (let [[ok? err] (protect (timed-out))]
                   (assert (not ok?) "1: protect captured the failure")
                   (push order (get err :error)))
                 (suspend)
                 (push order :body-end)
                 :body-value)]
    (assert (= result :body-value) "1: defer returned the body's value")
    (assert (= (freeze order) [:timeout :body-end :cleanup])
            (string "1: body then cleanup, got " (string (freeze order))))))
(println "  protect in a defer body: ok")

## ── 2. try/catch inside a defer body ────────────────────────────────

(let [order @[]]
  (let [result (defer
                 (push order :cleanup)
                 (try
                   (timed-out)
                   (catch e (push order (get e :error))))
                 (suspend)
                 (push order :body-end)
                 :body-value)]
    (assert (= result :body-value) "2: defer returned the body's value")
    (assert (= (freeze order) [:timeout :body-end :cleanup])
            (string "2: handler, body, then cleanup, got "
                    (string (freeze order))))))
(println "  try/catch in a defer body: ok")

## ── 3. one defer inside another ─────────────────────────────────────
##
## The inner form's cleanup belongs to the outer form's body, so it runs
## before the outer cleanup — and the outer body continues after it.

(let [order @[]]
  (let [result (defer
                 (push order :outer-cleanup)
                 (defer
                   (push order :inner-cleanup)
                   (protect (timed-out))
                   (suspend)
                   (push order :inner-body-end))
                 (push order :outer-body-end)
                 :body-value)]
    (assert (= result :body-value)
            "3: the outer defer returned its body's value")
    (assert (= (freeze order)
               [:inner-body-end :inner-cleanup :outer-body-end :outer-cleanup])
            (string "3: inside out, got " (string (freeze order))))))
(println "  a defer inside a defer: ok")

## ── 4. a cleanup that suspends on its own ───────────────────────────
##
## Cleanup is ordinary code: it can wait on the scheduler too. It still
## runs after the whole body, and the form still returns the body's value.

(let [order @[]]
  (let [result (defer
                 (begin
                   (suspend)
                   (push order :cleanup))
                 (protect (timed-out))
                 (suspend)
                 (push order :body-end)
                 :body-value)]
    (assert (= result :body-value) "4: defer returned the body's value")
    (assert (= (freeze order) [:body-end :cleanup])
            (string "4: body then cleanup, got " (string (freeze order))))))
(println "  a cleanup that suspends: ok")

## ── 5. with-temp-dir holds the directory for the whole body ─────────
##
## `with` is `defer`, so a body that captures an async error must still
## find its resource when it continues.

(def @kept nil)
(with-temp-dir dir (assign kept dir) (protect (timed-out)) (suspend)
               (file/write (path/join dir "after") "data")
               (assert (= (file/read (path/join dir "after")) "data")
                       "5: the directory is still there after the body's capture"))
(assert (string? kept) "5: the body ran")
(assert (not (path/exists? kept)) "5: the directory is gone once the body ends")
(println "  with-temp-dir across a capture: ok")

## ── 6. the two halves on their own ──────────────────────────────────
##
## Neither half alone takes the path above, and both keep their own
## behavior.

## An error with no suspension: cleanup still runs after the body.
(let [order @[]]
  (let [result (defer
                 (push order :cleanup)
                 (protect (error {:error :boom :message "boom"}))
                 (push order :body-end)
                 :body-value)]
    (assert (= result :body-value) "6a: defer returned the body's value")
    (assert (= (freeze order) [:body-end :cleanup])
            (string "6a: body then cleanup, got " (string (freeze order))))))

## A suspension with no error: the body's value comes back.
(let [order @[]]
  (let [result (defer
                 (push order :cleanup)
                 (let [[ok? _] (protect (ev/sleep 0.001))]
                   (assert ok? "6b: a sleep that completes is a success"))
                 (push order :body-end)
                 :body-value)]
    (assert (= result :body-value) "6b: defer returned the body's value")
    (assert (= (freeze order) [:body-end :cleanup])
            (string "6b: body then cleanup, got " (string (freeze order))))))

## An uncaptured async error: defer runs its cleanup, then propagates.
(let [order @[]]
  (let [[ok? err] (protect ((fn []
                              (defer
                                (push order :cleanup)
                                (push order :body-start)
                                (timed-out)))))]
    (assert (not ok?) "6c: the body's error propagates out of defer")
    (assert (= (get err :error) :timeout)
            (string "6c: expected a :timeout error, got " (string err)))
    (assert (= (freeze order) [:body-start :cleanup])
            (string "6c: the cleanup ran, got " (string (freeze order))))))
(println "  error alone, suspension alone, and propagation: ok")

## ── 7. ev/abort delivers its error the same way ─────────────────────
##
## A completion's error is not the only error that reaches a parked
## chain: `ev/abort` — and every `ev/timeout` that runs out — injects one
## by the same route. The body owns its continuation there too.

(let [order @[]]
  (let [f (ev/spawn (fn []
                      (defer
                        (push order :cleanup)
                        (protect (ev/sleep 30))
                        (suspend)
                        (push order :body-end))))]
    (ev/sleep 0.001)
    (ev/abort f)
    (ev/sleep 0.01)
    (assert (= (freeze order) [:body-end :cleanup])
            (string "7: body then cleanup, got " (string (freeze order))))))
(println "  ev/abort into a captured wait: ok")

(port/close listener)
(println "tests/elle/unwind-suspend.lisp: all tests passed")
