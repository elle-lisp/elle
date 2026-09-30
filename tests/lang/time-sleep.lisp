(elle/epoch 14)
# audited: 2026-09-30
# time/sleep blocks the calling thread for a number of seconds, and refuses an argument that is not one.
# docs/analysis/debugging.md
#
# ev/sleep is the sleep that yields to the scheduler instead; io.lisp tests it.

(defn error-kind [thunk]
  "The :error kind `thunk` signals, or nil when it returns."
  (let [[ok? err] (protect (thunk))]
    (when (not ok?) (get err :error))))

# ── A sleep answers nil ─────────────────────────────────────────────

(assert (nil? (time/sleep 0)) "time/sleep of an int answers nil")
(assert (nil? (time/sleep 0.0)) "time/sleep of a float answers nil")

# ── It takes exactly one argument ───────────────────────────────────

(let [[ok? _] (protect ((fn () (eval '(time/sleep)))))]
  (assert (not ok?) "time/sleep with no arguments fails"))
(let [[ok? _] (protect ((fn () (eval '(time/sleep 1 2)))))]
  (assert (not ok?) "time/sleep with two arguments fails"))

# ── It refuses what is not a duration ───────────────────────────────

(assert (= (error-kind (fn () (time/sleep "not a number"))) :type-error)
        "time/sleep refuses a string")
(assert (= (error-kind (fn () (time/sleep nil))) :type-error)
        "time/sleep refuses nil")
(assert (= (error-kind (fn () (time/sleep -1))) :argument-error)
        "time/sleep refuses a negative int")
(assert (= (error-kind (fn () (time/sleep -0.5))) :argument-error)
        "time/sleep refuses a negative float")
(assert (= (error-kind (fn () (time/sleep (/ 1.0 0.0)))) :argument-error)
        "time/sleep refuses an infinite duration")

# ── It refuses a duration longer than the clock can count ───────────
# The trap: 1e300 seconds does not fit a Rust Duration, and the conversion
# that assumed it would panicked, so the process died where no protect could
# catch it. The int path took any non-negative int and slept that long.

(assert (= (error-kind (fn () (time/sleep 1e300))) :argument-error)
        "time/sleep refuses a float the clock cannot count")
(assert (= (error-kind (fn () (time/sleep 9223372036854775807))) :argument-error)
        "time/sleep refuses an int the clock cannot count")
