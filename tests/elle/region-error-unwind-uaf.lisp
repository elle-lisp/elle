(elle/epoch 12)
# Soundness complement of region-error-unwind.lisp
# (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
# still owes"). Run under `--trace=guardfree` by the subprocess pin
# `region_error_unwind_uaf` in tests/integration/elle_scripts.rs.
#
# An error abandons the frame, so the exit runs the releases the frame still
# owed — off the slots the emitter recorded for its value routes. Each is a
# release the frame genuinely had, run earlier than it would have been, so what
# has to survive is everything that outlives the frame.
#
# Three faces.
#
# 1. The SIGNAL PAYLOAD. A raising native may hand back a value it read out of
#    an argument the abandoned frame is holding, and `protect` delivers that
#    payload to the catcher as data. The walk skips a slot whose value lives in
#    the payload's region, so the catcher's read must find it whole.
# 2. A value the frame STORED somewhere that outlives it. The store funnel took
#    a counted reference, so the walk's release drops the frame's own and no
#    more — the container's read after the catch must still work.
# 3. The RESTARTS system. A fiber body parks its own frame on an error exit, so
#    a restart replays the very instructions the walk would have run. The parked
#    frame is not walked at all; resuming it and reading its values proves it.
#
# Every read below happens AFTER the unwind ran, so an over-release faults at
# the deref (guardfree) or trips the generation check.

# ── 1. the payload a raising native builds while the frame holds its argument ─
# The frame materializes a fresh string for the call, so its release sits past
# the raise and the walk runs it. The payload the catcher receives is built by
# the raising native and must be whole after that.

(defn payload-from-arg [tag]
  (let [[ok e] (protect (get (string "s-" tag) :not-a-key))]
    [ok (type-of e) (get e :error)]))

(var i 0)
(while (< i 40)
  (let [r (payload-from-arg i)]
    (assert (= (get r 0) false) "the bad-key read must raise")
    (assert (= (get r 1) :struct) "the error payload must survive the unwind")
    (assert (= (type-of (get r 2)) :keyword)
            "the payload's own fields must survive the unwind"))
  (assign i (+ i 1)))

# ── 2. a value the abandoned frame stored into a longer-lived container ───────
# `sink` outlives every call. The frame stores a fresh string into it, then
# raises with that string's own binding still live — so the walk releases the
# frame's reference and the sink's counted one must keep the value alive.

(def sink @[])

(defn store-then-raise [tag]
  (let [s (string "kept-" tag)]
    (push sink s)
    # `s` is still live here: its release sits past the raising call.
    (get s :not-a-key)))

(assign i 0)
(while (< i 40)
  (protect (store-then-raise i))
  (assign i (+ i 1)))

(assert (= (length sink) 40) "every stored value must have reached the sink")
(var n 0)
(while (< n (length sink))
  (assert (= (type-of (get sink n)) :string)
          "a value the abandoned frame stored must outlive the unwind")
  (assign n (+ n 1)))

# ── 3. the restarts system: a parked `:error` frame is not walked ─────────────
# A fiber body's first run parks its own frame on an error exit, so the frame
# keeps every release it owed. Resuming the fiber replays those instructions;
# reading the fiber's value across the restart proves the walk left the parked
# frame alone.

(defn park-then-restart [tag]
  (let [f (fiber/new (fn []
                       (let [held (string "held-" tag)]
                         (get held :not-a-key))) |:error|)]
    (fiber/resume f)
    (let [v (fiber/value f)]
      (try
        (fiber/resume f)
        (catch e nil))
      [(type-of v) (fiber/status f)])))

(assign i 0)
(while (< i 40)
  (let [r (park-then-restart i)]
    (assert (= (get r 0) :struct)
            "a parked error frame's payload must survive the restart"))
  (assign i (+ i 1)))

# ── 4. the caught error's handler reads an outer frame's value ────────────────
# The enclosing frame is abandoned too, so its own pending releases run. What
# the handler names must be the catcher's, never the abandoned frame's.

(defn outer-holds [tag]
  (let [o (string "outer-" tag)]
    (let [[ok e] (protect (get o :not-a-key))]
      # `o` belongs to THIS frame, which caught rather than raised.
      [(length o) ok (type-of e)])))

(assign i 0)
(while (< i 40)
  (let [r (outer-holds i)]
    (assert (< 0 (get r 0)) "the catching frame's own value must be whole"))
  (assign i (+ i 1)))

(println "region-error-unwind-uaf: ok")
