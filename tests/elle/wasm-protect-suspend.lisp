(elle/epoch 12)
# Counterfactual: `protect`/`defer`/`with` around a body that SUSPENDS on a
# scheduler wait must drive that body to completion and observe its real result.
#
# THE GAP: these macros wrap their body in a NESTED fiber `f` with a narrow mask
# (SIG_ERROR only) and resume it once with a single `(fiber/resume f)`. When the
# body suspends on a scheduler `:wait` (an inner `ev/join`), `f` emits SIG_WAIT,
# which `f`'s mask does not cover. On the VM the uncaught wait PROPAGATES up the
# fiber stack: the resumer re-emits it, the scheduler drives `f` transparently,
# and the single `(fiber/resume f)` only returns once `f` is done. Under
# `--wasm=full` the host resume path (src/wasm/resume.rs `route_emit`) used to
# PARK the child on any non-error suspend and return signal 0, so
# `(fiber/resume f)` returned immediately with `f` still `:paused` holding the
# raw wait-request struct — `protect` then read `[false {:op :join …}]` instead
# of the body's value, and `defer`/`with` propagated the request as a bogus
# error. The fix propagates an uncaught wait/io through the resumer (so the
# scheduler catches it) and, on the resumer's resume, RE-DRIVES `f` with the
# scheduler's value.
#
# RED only under `--wasm=full` before the fix (the VM/JIT save the whole stack
# and always propagate); GREEN on every tier after. Surfaces the corpus files
# structured-concurrency.lisp and sync.lisp.

# protect around a suspending join: success returns [true value].
(assert (= [true 30] (protect (ev/join (ev/spawn (fn [] (+ 10 20))))))
        "protect around a suspending join returns [true value]")

# protect around a suspending join whose child errors: [false error].
(assert (= [false {:e 1}] (protect (ev/join (ev/spawn (fn [] (error {:e 1}))))))
        "protect around a suspending join captures the child's error")

# A direct (non-suspending) error is caught the same way — regression guard for
# the error path the suspend fix must not disturb.
(assert (= [false {:e 2}] (protect (error {:e 2})))
        "protect around a direct error still returns [false error]")

# defer around a suspending body: cleanup runs unconditionally, the body's value
# is returned on success.
(let [log @[]]
  (let [v (defer
            (push log :cleaned)
            (ev/join (ev/spawn (fn [] 42))))]
    (assert (= v 42) "defer returns a suspending body's value")
    (assert (= log @[:cleaned]) "defer cleanup runs after a suspending body")))

# defer around a suspending body that errors: cleanup runs, error propagates.
(let [log @[]]
  (let [[ok? val] (protect (defer
                             (push log :cleaned)
                             (ev/join (ev/spawn (fn [] (error {:e 3}))))))]
    (assert (= ok? false) "defer propagates a suspending body's error")
    (assert (= val {:e 3}) "defer surfaces the real error value")
    (assert (= log @[:cleaned]) "defer cleanup runs even when the body errors")))

# Two sequential joins inside one protect: the body re-suspends once per join,
# so the resumer must re-drive the protect fiber across BOTH scheduler
# round-trips before its result is read.
(assert (= [true 30]
           (protect (+ (ev/join (ev/spawn (fn [] 10)))
                       (ev/join (ev/spawn (fn [] 20))))))
        "protect drives a body across two sequential waits")

# Nested protect around a suspending body: the inner protect propagates through
# the outer one, which itself propagates to the scheduler.
(assert (= [true [true 3]]
           (protect (protect (ev/join (ev/spawn (fn [] (+ 1 2)))))))
        "nested protect around a suspending body composes")

# with (resource acquire/release) around a suspending body: destructor runs.
(let [log @[]]
  (let [v (with r
                (do
                  (push log :acquired)
                  :resource) (fn [_] (push log :released))
                (ev/join (ev/spawn (fn [] 99))))]
    (assert (= v 99) "with returns a suspending body's value")
    (assert (= log @[:acquired :released])
            "with releases after a suspending body")))

(println "wasm-protect-suspend: ok")
