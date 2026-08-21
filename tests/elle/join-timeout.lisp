(elle/epoch 12)
# os/join with a deadline (docs/threads.md § join with a deadline).
#
# sys/join (aliases os/join / join) waits for an OS thread to finish WITHOUT
# polling and WITHOUT parking the OS thread: it cooperates with the scheduler
# via the same cross-thread wake path chan/select uses. A spawned worker, on
# completion, signals a private channel; the joiner parks on it (yielding to
# the scheduler) until the worker is done or the deadline elapses.
#
# These assertions encode the contract independently of the implementation:
#   - no-timeout join returns the worker's value (eventually);
#   - a timeout join on a slow worker raises a typed {:error :timeout} promptly,
#     not after the worker finishes;
#   - join is idempotent once the thread has completed;
#   - a worker that errors surfaces as {:error :thread-error}.
#
# Slow workers ship `ev/run` into the spawned thread (the closure references it,
# so the serializer drags it into the bundle) so they can ev/sleep — a worker
# has no scheduler of its own.

# ── no-timeout join returns the result ────────────────────────────────
(assert (= 42 (sys/join (sys/spawn-vm (fn [] (+ 40 2)))))
        "join with no timeout returns the worker's value")

# ── a generous timeout still returns the result ───────────────────────
(assert (= 42 (sys/join (sys/spawn-vm (fn [] (+ 40 2))) 5000))
        "join within a generous deadline returns the worker's value")

# ── no-timeout join WAITS for a slow worker, then returns its value ────
# The worker sleeps ~0.3s under a shipped scheduler; the join must block until
# it finishes (no premature nil/timeout) and yield the real result.
(assert (= :slow (sys/join (sys/spawn-vm (fn []
                             (ev/run (fn []
                                       (ev/sleep 0.3)
                                       :slow))))))
        "no-timeout join waits for a slow worker and returns its value")

# ── a short timeout on a slow worker raises a typed :timeout, PROMPTLY ─
# Worker would take ~2s; we wait 100ms. The join must return a timeout error in
# well under the worker's runtime (proving it did not wait out the worker).
(let [start (clock/monotonic)
      h (sys/spawn-vm (fn []
                        (ev/run (fn []
                                  (ev/sleep 2)
                                  :too-slow))))
      [ok? payload] (protect (sys/join h 100))
      elapsed (- (clock/monotonic) start)]
  (assert (not ok?) "slow worker + short deadline does not succeed")
  (assert (= (get payload :error) :timeout)
          "timeout surfaces as a typed {:error :timeout}")
  (assert (< elapsed 1.0)
          "join returns at the deadline, not after the worker finishes"))

# ── join is idempotent: a completed thread can be joined repeatedly ────
(let [h (sys/spawn-vm (fn [] (* 6 7)))
      first (sys/join h)
      second (sys/join h)]
  (assert (= 42 first) "first join returns the result")
  (assert (= 42 second) "second join returns the same result"))

# A completed thread joined with a timeout still returns its value (the result
# is already present, so no waiting and certainly no timeout).
(let [h (sys/spawn-vm (fn [] 7))
      _ (sys/join h)]
  (assert (= 7 (sys/join h 1))
          "joining a finished thread with a tiny timeout still returns the value"))

# ── a worker that errors surfaces as a thread-error, not a hang ────────
(let [h (sys/spawn-vm (fn [] (error "boom")))
      [ok? payload] (protect (sys/join h 5000))]
  (assert (not ok?) "a worker that errors does not succeed")
  (assert (= (get payload :error) :thread-error)
          "a worker error surfaces as {:error :thread-error}"))

# ── the join yields: a concurrent fiber makes progress while we wait ───
# While blocked joining a slow OS thread, the scheduler must keep running other
# fibers. A fiber that sends on a channel mid-join must be observed.
(let [[tx rx] (chan)
      _ (ev/spawn (fn [] (chan/send tx :fiber-ran)))
      h (sys/spawn-vm (fn []
                        (ev/run (fn []
                                  (ev/sleep 0.3)
                                  :done))))
      _ (sys/join h)
      got (chan/recv rx)]
  (assert (= (get got 0) :ok) "a concurrent fiber ran while join was waiting")
  (assert (= (get got 1) :fiber-ran) "the concurrent fiber's message arrived"))
