(elle/epoch 12)
# Soundness complement of region-tail-signal-exit.lisp
# (docs/impl/region/mechanism.md § "What the fall-through owes, a signal exit
# owes too"). Run under `--trace=guardfree` by the subprocess pin
# `region_tail_signal_exit_uaf` in tests/integration/elle_scripts.rs.
#
# A native tail call that leaves by a SIGNAL abandons the post-`TailCall` block,
# so the signal exit runs that block's releases itself. Each such release is a
# new release on a path that ran none before, and it owes exactly what any new
# release owes: the reference it drops must be the frame's own.
#
# Three faces have to survive it.
#
# 1. The value the SIGNAL PAYLOAD carries. A fiber carrier hands the payload the
#    fiber ARGUMENT itself, and a suspending native hands the scheduler a
#    request embedding its arguments — each a holder of its own, counted where
#    it is taken, so the frame's release must not be the value's last.
# 2. The REPLAY. A suspending signal parks the continuation at the
#    post-`TailCall` ip and an `:error` fiber is resumable, so the same release
#    is reached a second time. The exit consumes the route's name — a value or
#    cell route stamps its local `nil` — so the replay finds nothing and the
#    release counts once. Reading the value after the replay proves the first
#    run did not free it.
# 3. An OUTER holder. A caught error hands control back to a frame that still
#    names the value, whose own reference the exit's release must leave standing.
#
# Every read below happens AFTER the signal exit ran, so an over-release faults
# at the deref (guardfree) or trips the generation check.

# ── 1. the fiber carrier, and the outer holder ────────────────────────────────
# `(fiber/abort f 7)` is a native tail call on a CAPTURED `f`, so the frame
# mints a reference for the move and the post-`TailCall` block is its only
# consumer. The abort leaves by SIG_ABORT; `f` must survive for the reads that
# follow, which reach it through the binding the thunk captured it from.

(defn abort-then-read [tag]
  (let [f (fiber/new (fn []
                       (yield tag)
                       9) |:yield|)]
    (fiber/resume f)
    (try
      ((fn [] (fiber/abort f 7)))
      (catch e nil))
    # The frame's retain is gone; the binding's reference is not.
    [(fiber/status f) (type-of f)]))

(var i 0)
(while (< i 40)
  (let [r (abort-then-read (string "tag-" i))]
    (assert (= (get r 1) :fiber) "the aborted fiber value must still be live"))
  (assign i (+ i 1)))

# ── 2. the replay: a restarted `:error` fiber re-enters the same block ────────
# The fiber's mask catches :error, so the abort lands it `:error` — resumable.
# Resuming replays the parked frame from the post-`TailCall` ip, where the
# release the exit already ran sits. The nil stamp is what makes that second
# arrival a no-op; without it the replay releases a reference nobody holds.

(defn abort-then-restart [tag]
  (let [payload (string "payload-" tag)
        f (fiber/new (fn []
                       (yield payload)
                       9) |:yield :error|)]
    (fiber/resume f)
    (try
      ((fn [] (fiber/abort f payload)))
      (catch e nil))
    (try
      (fiber/resume f)
      (catch e nil))
    # Both the payload and the fiber outlive every replayed release.
    [(length payload) (type-of f)]))

(assign i 0)
(while (< i 40)
  (let [r (abort-then-restart i)]
    (assert (< 0 (get r 0)) "the abort payload must survive the replay")
    (assert (= (get r 1) :fiber) "the restarted fiber must survive the replay"))
  (assign i (+ i 1)))

# ── 3. the suspending exit: park, resume, then read the captured argument ─────
# `(fiber/resume inner)` in tail position is the SIG_SWITCH handoff: the frame
# parks its continuation at the post-`TailCall` ip and the resume replays it.
# `inner` is captured, so the block's release is the borrowed-arg retain — run
# once at the exit, no-oped at the replay, and `inner` still read afterwards.

(defn switch-then-read [tag]
  (let [inner (fiber/new (fn [] (string "inner-" tag)) 1)
        outer (fiber/new (fn [] ((fn [] (fiber/resume inner)))) 1)]
    (let [v (fiber/resume outer)]
      [(type-of inner) (type-of outer) v])))

(assign i 0)
(while (< i 40)
  (let [r (switch-then-read i)]
    (assert (= (get r 0) :fiber) "the resumed inner fiber must survive the park")
    (assert (= (get r 1) :fiber) "the parking outer fiber must survive its exit"))
  (assign i (+ i 1)))

# ── 4. the capability denial: the denied call's arguments stay readable ───────
# The denial abandons the block before the native runs at all. The denied
# fiber's error struct — built from the very arguments whose releases the exit
# runs — is read afterwards through `fiber/value`.

(defn denied-then-read [tag]
  (let [msg (string "blocked-" tag)
        f (fiber/new (fn [] (println msg)) |:error :io| :deny |:io|)]
    (fiber/resume f)
    [(get (fiber/value f) :error) (length msg)]))

(assign i 0)
(while (< i 40)
  (let [r (denied-then-read i)]
    (assert (< 0 (get r 1)) "the denied call's argument must survive the exit"))
  (assign i (+ i 1)))

(println "region-tail-signal-exit-uaf: ok")
