(elle/epoch 9)
## lib/http2/stream.lisp — HTTP/2 stream state machine + flow control
##
## Loaded via:
##   (def sync   ((import "std/sync")))
##   (def frame  ((import "std/http2/frame")))
##   (def stream ((import "std/http2/stream") :sync sync :frame frame))
##
## Exports: {:make-stream :transition :make-flow-control :test}

(fn [&named sync frame]

  ## ── Stream constructor ─────────────────────────────────────────────────

  (defn make-stream [id initial-window]
    "Create a new stream with the given ID and initial flow-control window."
    @{:id id
      :state :idle
      :flow (make-flow-control initial-window)
      :recv-window initial-window
      :data-queue (sync:make-queue 64)
      :headers nil
      :pending-headers nil
      :error-code nil})

  ## ── State transitions ──────────────────────────────────────────────────
  ## Events: :send-headers :recv-headers :send-end-stream :recv-end-stream
  ##         :send-rst :recv-rst :send-push-promise :recv-push-promise

  (defn tx-key [state event]
    "Build a transition lookup key from two keywords using their hashes."
    (bit/xor (hash state) (bit/shl (hash event) 1)))
  (def transitions
    {(tx-key :idle :send-headers) :open
     (tx-key :idle :recv-headers) :open
     (tx-key :idle :send-push-promise) :reserved-local
     (tx-key :idle :recv-push-promise) :reserved-remote
     (tx-key :open :send-end-stream) :half-closed-local
     (tx-key :open :recv-end-stream) :half-closed-remote
     (tx-key :open :send-rst) :closed
     (tx-key :open :recv-rst) :closed
     (tx-key :half-closed-local :recv-end-stream) :closed
     (tx-key :half-closed-local :recv-rst) :closed
     (tx-key :half-closed-local :send-rst) :closed
     (tx-key :half-closed-remote :send-end-stream) :closed
     (tx-key :half-closed-remote :send-rst) :closed
     (tx-key :half-closed-remote :recv-rst) :closed
     (tx-key :reserved-local :send-headers) :half-closed-remote
     (tx-key :reserved-local :send-rst) :closed
     (tx-key :reserved-remote :recv-headers) :half-closed-local
     (tx-key :reserved-remote :send-rst) :closed})
  (defn stream-transition [stream event]
    "Apply a state transition to a stream. Signals :h2-error on invalid transitions."
    (let* [current stream:state
           key (tx-key current event)
           next-state (get transitions key)]
      (if (nil? next-state)
        (error {:error :h2-error
                :reason :stream-error
                :stream-id stream:id
                :code 0x1
                :message (concat "invalid transition: "
                                 (string current)
                                 " + "
                                 (string event))})
        (put stream :state next-state))
      next-state))

  ## ── Flow control ───────────────────────────────────────────────────────

  (defn make-flow-control [initial-window]
    "Create a flow control tracker. Returns mutable struct with
     send-window, recv-window, lock, and condvar for blocking."
    (let [lock (sync:make-lock)
          cv (sync:make-condvar)]
      @{:send-window initial-window
        :recv-window initial-window
        :lock lock
        :cv cv}))
  (defn consume-send-window [fc amount]
    "Block until enough send window is available, then consume it.
     Returns the actual amount consumed (may be less than requested
     if max-frame-size limits apply, but never 0)."
    (let [lock fc:lock
          cv fc:cv]
      (lock:acquire)
      (while (<= fc:send-window 0) (cv:wait lock))
      (let [actual (min amount fc:send-window)]
        (put fc :send-window (- fc:send-window actual))
        (lock:release)
        actual)))
  (defn apply-window-update [fc increment]
    "Apply a WINDOW_UPDATE increment to the send window. Wakes blocked senders."
    (let [lock fc:lock
          cv fc:cv]
      (lock:acquire)
      (let [new-window (+ fc:send-window increment)]
        (when (> new-window 2147483647)
          (lock:release)
          (error {:error :h2-error
                  :reason :flow-control-error
                  :message "flow control window overflow"}))
        (put fc :send-window new-window))
      (cv:broadcast)
      (lock:release)))
  (defn consume-recv-window [fc amount]
    "Consume recv window (for tracking). Does not block."
    (put fc :recv-window (- fc:recv-window amount)))
  (defn replenish-recv-window [fc amount]
    "Replenish recv window after consuming data."
    (put fc :recv-window (+ fc:recv-window amount)))

  ## ── Tests ──────────────────────────────────────────────────────────────

  (defn run-tests []  # ── State transitions ──
    (let [s (make-stream 1 65535)]
      (assert (= s:state :idle) "stream: initial state")
      (stream-transition s :send-headers)
      (assert (= s:state :open) "stream: idle->open")
      (stream-transition s :send-end-stream)
      (assert (= s:state :half-closed-local) "stream: open->half-closed-local")
      (stream-transition s :recv-end-stream)
      (assert (= s:state :closed) "stream: half-closed-local->closed"))

    # ── Server-side transitions ──
    (let [s (make-stream 1 65535)]
      (stream-transition s :recv-headers)
      (assert (= s:state :open) "stream server: idle->open")
      (stream-transition s :recv-end-stream)
      (assert (= s:state :half-closed-remote)
              "stream server: open->half-closed-remote")
      (stream-transition s :send-end-stream)
      (assert (= s:state :closed) "stream server: half-closed-remote->closed"))

    # ── RST_STREAM ──
    (let [s (make-stream 3 65535)]
      (stream-transition s :send-headers)
      (stream-transition s :recv-rst)
      (assert (= s:state :closed) "stream: RST closes"))

    # ── Invalid transition ──
    (let [s (make-stream 5 65535)]
      (stream-transition s :send-headers)
      (stream-transition s :send-end-stream)  # half-closed-local: cannot send end-stream again
      (let [[ok? err] (protect (stream-transition s :send-end-stream))]
        (assert (not ok?) "stream: invalid transition errors")))

    # ── Per-stream flow control struct ──
    (let [s (make-stream 1 65535)]
      (assert (= s:flow:send-window 65535) "stream: flow control initial window")
      (let [consumed (consume-send-window s:flow 1000)]
        (assert (= consumed 1000) "stream: flow consume")
        (assert (= s:flow:send-window 64535) "stream: flow after consume"))
      (apply-window-update s:flow 1000)
      (assert (= s:flow:send-window 65535) "stream: flow after update"))

    # ── Pending headers field ──
    (let [s (make-stream 1 65535)]
      (assert (nil? s:pending-headers) "stream: pending-headers nil initially"))

    # ── Connection-level flow control ──
    (let [fc (make-flow-control 100)]
      (assert (= fc:send-window 100) "fc: initial send window")
      (let [consumed (consume-send-window fc 50)]
        (assert (= consumed 50) "fc: consumed 50")
        (assert (= fc:send-window 50) "fc: window after consume"))  # Consume rest
      (consume-send-window fc 50)
      (assert (= fc:send-window 0) "fc: window at 0")  # Window update
      (apply-window-update fc 200)
      (assert (= fc:send-window 200) "fc: window after update"))

    # ── Flow control overflow ──
    (let [fc (make-flow-control 2147483647)]
      (let [[ok? err] (protect (apply-window-update fc 1))]
        (assert (not ok?) "fc: overflow detection")))

    # ── Recv window tracking ──
    (let [fc (make-flow-control 65535)]
      (consume-recv-window fc 1000)
      (assert (= fc:recv-window 64535) "fc: recv consumed")
      (replenish-recv-window fc 1000)
      (assert (= fc:recv-window 65535) "fc: recv replenished"))
    true)

  ## ── Exports ────────────────────────────────────────────────────────────

  {:make-stream make-stream
   :transition stream-transition
   :make-flow-control make-flow-control
   :consume-send-window consume-send-window
   :apply-window-update apply-window-update
   :consume-recv-window consume-recv-window
   :replenish-recv-window replenish-recv-window
   :test run-tests})
