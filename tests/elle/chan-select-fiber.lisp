(elle/epoch 10)
# Channel/select × fiber scheduler interaction tests.
#
# Regression cover for: chan/select must not park the OS thread, because the
# fiber scheduler runs on that same thread.  An ev/spawn'd producer fiber
# cannot fire its chan/send while the parent is parked inside a synchronous
# crossbeam Select::select_timeout — the bug originally reproduced in
# ~/git/grace/infra/repro-spawn-chan-select.lisp.

# ============================================================================
# Test A: ev/spawn'd producer + chan/select with timeout.
#
# A trivial fiber sends a value on a channel.  The parent chan/selects with a
# 1s timeout.  If chan/select is scheduler-aware, the parent yields, the fiber
# runs, sends, and the parent resumes with the value well under the timeout.
# If chan/select parks the OS thread, the fiber never runs and the parent
# hits the timeout.
# ============================================================================

(let [[tx rx] (chan)
      _ (ev/spawn (fn [] (chan/send tx :hello-from-fiber)))
      t0 (clock/monotonic)
      sel (chan/select @[rx] 1000)
      elapsed (- (clock/monotonic) t0)]
  (assert (array? sel) "chan/select should return an array")
  (assert (= (length sel) 2)
          "select should resolve to [index msg], not [:timeout]")
  (assert (= (get sel 0) 0) "select index should be 0 (the only receiver)")
  (assert (= (get sel 1) :hello-from-fiber)
          "select should observe the fiber's send")
  (assert (< elapsed 0.5)
          "select must resume promptly, not wait out the 1s timeout"))

# ============================================================================
# Test B: chan/select with no producer must still hit the timeout.
#
# Counter-factual to test A — confirms the timeout path still fires when
# there is genuinely nothing to receive.  Without this we couldn't tell a
# broken timeout (returns early as :woken/spurious) from a working one.
# ============================================================================

(let [[tx rx] (chan)
      t0 (clock/monotonic)
      sel (chan/select @[rx] 50)
      elapsed (- (clock/monotonic) t0)]
  (assert (array? sel) "chan/select should return an array")
  (assert (= (length sel) 1)
          "timeout result should be a single-element [:timeout] tuple")
  (assert (= (get sel 0) :timeout)
          "select with no producer must return [:timeout]")
  (assert (>= elapsed 0.04)
          "select must actually wait the timeout, not return immediately")
  (assert (< elapsed 0.5) "select must not over-wait the timeout"))

# ============================================================================
# Test C: ev/spawn'd producer that yields before sending.
#
# The producer does an ev/sleep before sending, forcing the parent's
# chan/select to genuinely park (not just race-win the try-fast-path).
# Confirms the wake-after-park path actually wakes the parent.
# ============================================================================

(let [[tx rx] (chan)
      _ (ev/spawn (fn []
                    (ev/sleep 0.02)
                    (chan/send tx :after-sleep)))
      t0 (clock/monotonic)
      sel (chan/select @[rx] 1000)
      elapsed (- (clock/monotonic) t0)]
  (assert (= (length sel) 2) "parked select must wake when the producer fires")
  (assert (= (get sel 1) :after-sleep)
          "parked select must observe the producer's value")
  (assert (>= elapsed 0.02)
          "parked select must wait at least the producer's sleep")
  (assert (< elapsed 0.5)
          "parked select must resume on the producer's send, not the timeout"))

# ============================================================================
# Test D: sys/spawn'd OS-thread producer + chan/select.
#
# A producer thread sends on the channel from outside the scheduler.  The
# parent fiber chan/selects.  Confirms the cross-thread wake path works —
# chan/send from a foreign thread must wake a parked selector on the
# scheduler thread.
# ============================================================================

(let [[tx rx] (chan)
      _ (sys/spawn (fn []  # Small synchronous sleep so the parent definitely
                     # parks first.  ev/sleep requires a scheduler — we
                     # don't have one on the spawned OS thread.
                     (time/sleep 0.02)
                     (chan/send tx :hello-from-os-thread)))
      t0 (clock/monotonic)
      sel (chan/select @[rx] 1000)
      elapsed (- (clock/monotonic) t0)]
  (assert (= (length sel) 2)
          "cross-thread select must wake when producer thread fires")
  (assert (= (get sel 1) :hello-from-os-thread)
          "cross-thread select must observe the thread's value")
  (assert (< elapsed 0.5) "cross-thread select must not wait out the timeout"))

# ============================================================================
# Test E: cross-thread race stress.
#
# Many producer threads, each firing immediately with no sleep, so the parent's
# initial chan/try-select frequently sees :empty just before the send arrives —
# exactly the race that the post-register re-check in chan/wait-ready closes.
# If the race is open, at least one of the chan/selects will park on an
# eventfd no one will signal, hit the 500ms timeout, and the test will fail.
# ============================================================================

(let [iterations 200]
  (each i in (range 0 iterations)
    (let [[tx rx] (chan)
          producer (sys/spawn (fn [] (chan/send tx i)))
          t0 (clock/monotonic)
          sel (chan/select @[rx] 500)
          elapsed (- (clock/monotonic) t0)]
      (sys/join producer)
      (assert (= (length sel) 2)
              (string "iteration " i ": cross-thread select hit timeout"))
      (assert (= (get sel 1) i) (string "iteration " i ": value mismatch"))  # Per-iteration cap — a lost-wake race would push elapsed up to
      # ~500ms (the timeout) while still returning a value via the
      # wrapper's post-park chan/try-select.  Anything above 50ms is
      # extremely suspect on a quiet machine.
      (assert (< elapsed 0.05)
              (string "iteration " i ": cross-thread select waited too long: "
                      elapsed " s — race likely lost the wake")))))

# ============================================================================
# Test F: multi-receiver select picks the right index.
#
# Two channels, only one producer.  chan/select over both must report the
# correct receiver index — not 0 by default.  Index identifies which channel
# fired, which is the entire point of select.
# ============================================================================

(let [[tx0 rx0] (chan)
      [tx1 rx1] (chan)
      _ (ev/spawn (fn [] (chan/send tx1 :from-one)))
      sel (chan/select @[rx0 rx1] 1000)]
  (assert (= (length sel) 2)
          "multi-receiver select must resolve to [i v], not :timeout")
  (assert (= (get sel 0) 1)
          "multi-receiver select must report the firing receiver's index")
  (assert (= (get sel 1) :from-one)
          "multi-receiver select must carry the firing receiver's value"))

# Two producers on different channels: confirm we still get one result
# (whichever fires first) and that the other value stays in its channel
# for a subsequent recv.

(let [[tx0 rx0] (chan)
      [tx1 rx1] (chan)
      _ (ev/spawn (fn [] (chan/send tx0 :zero)))
      sel (chan/select @[rx0 rx1] 1000)]
  (assert (= (get sel 0) 0) "select picks the receiver that has a value")
  (assert (= (get sel 1) :zero) "select returns the right value")  # The unused channel must still be empty.
  (let [r1 (chan/recv rx1)]
    (assert (= (get r1 0) :empty)
            "untouched receiver in a multi-receiver select stays empty")))

# ============================================================================
# Test G: receiver closes while parked.
#
# Parent parks on a receiver; a fiber closes the sender (last sender → channel
# becomes disconnected from the receiver's perspective).  The wake from
# chan/close should unblock the parked select; chan/try-select inside the
# wrapper then reports the closure via a runtime error (matching the same
# semantics as a closed receiver passed in upfront).
# ============================================================================

(let [[tx rx] (chan)
      _ (ev/spawn (fn []
                    (ev/sleep 0.02)  # Close every sender so the receiver observes disconnect.
                    (chan/close tx)))
      t0 (clock/monotonic)
      [ok? result] (protect (chan/select @[rx] 1000))
      elapsed (- (clock/monotonic) t0)]
  (assert ok? "select must return cleanly when the sender closes mid-park")
  (assert (= (get result 0) :disconnected)
          "select must observe :disconnected after the sender closes")
  (assert (< elapsed 0.5) "select must wake on close, not wait the full timeout"))

# ============================================================================
# Test H: ev/abort cancels a parked select cleanly.
#
# Spawn a fiber that parks forever in chan/select on a never-firing channel,
# then abort it.  The abort path drops the PendingOp, which drops the
# ChanSelectGuard, which closes the wake fd and deregisters from the
# WakeList.  We don't have direct visibility into fd leaks from Lisp, but we
# can confirm the abort returns promptly and subsequent selects on the same
# channel still work — a leaked WakeList fd would cause stale wakes.
# ============================================================================

(let [[tx rx] (chan)
      victim (ev/spawn (fn [] (chan/select @[rx] 60000)))]
  (ev/sleep 0.02)
  (ev/abort victim)  # The aborted fiber should be done now.  A fresh chan/select on the
  # same channel must work — confirms the WakeList wasn't left with a
  # dangling fd that would mis-fire or cause errors.
  (let [_ (ev/spawn (fn [] (chan/send tx :after-abort)))
        sel (chan/select @[rx] 1000)]
    (assert (= (length sel) 2)
            "post-abort select on the same channel must still work")
    (assert (= (get sel 1) :after-abort)
            "post-abort select must observe a fresh send")))
