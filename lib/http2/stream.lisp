(elle/epoch 12)
## lib/http2/stream.lisp — HTTP/2 stream state machine + flow control
##
## Loaded via:
##   (def frame  ((import "std/http2/frame")))
##   (def stream ((import "std/http2/stream") :frame frame))
##
## No sync dependency — uses bare ev/futex-wait and ev/futex-wake.
##
## Futex keys come from `(gensym)`, a process-global primitive counter,
## NOT a module-local counter.  `(import ...)` returns a fresh module
## instance each call, so a module-local counter would restart at 0 in
## every importer and hand out colliding keys — and since the scheduler's
## park-queue is process-global (one key -> one wait list), a wake on one
## channel/flow-control futex would unpark a waiter on another instance's
## (same elle bug class as the lib/sync futex-key collision, #861).
##
## Exports: {:make-stream :make-channel :transition :make-flow-control :test}

(fn [&named frame]

  ## ── Channel: unbounded cooperative FIFO ──────────────────────────────
  ## put never blocks. take blocks only when empty. No lock — this is a
  ## single-threaded cooperative runtime.

  (defn make-channel []
    (let [key (gensym)
          bx (box 0)
          buf @[]
          @closed false
          @waiting false]
      {:put (fn [val]
              (push buf val)
              (when waiting
                (rebox bx (inc (unbox bx)))
                (ev/futex-wake key 1))
              nil)
       :take (fn []
               (while (and (not closed) (= (length buf) 0))
                 (assign waiting true)
                 (let [gen (unbox bx)]
                   (when (= (length buf) 0) (ev/futex-wait key bx gen)))
                 (assign waiting false))
               (when (> (length buf) 0)
                 (let [val (get buf 0)]
                   (remove buf 0)
                   val)))
       :close (fn []
                (assign closed true)
                (rebox bx (inc (unbox bx)))
                (ev/futex-wake key 999999999)
                nil)
       :closed? (fn [] closed)
       :size (fn [] (length buf))}))

  ## ── Stream constructor ─────────────────────────────────────────────────

  (defn make-stream [id initial-window]
    @{:id id
      :state :idle
      :flow (make-flow-control initial-window)
      :recv-window initial-window
      :data-queue (make-channel)
      :headers nil
      :pending-headers nil
      :error-code nil})

  ## ── State transitions ──────────────────────────────────────────────────

  (defn tx-key [state event]
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
    (let* [current stream:state
           key (tx-key current event)
           next-state (get transitions key)]
      (if (nil? next-state)
        (error {:error :h2-error
                :reason :stream-error
                :stream-id stream:id
                :code 0x1
                :message (concat "invalid transition: " (string current) " + "
                                 (string event))})
        (put stream :state next-state))
      next-state))

  ## ── Flow control ───────────────────────────────────────────────────────

  (defn make-flow-control [initial-window]
    (let [key (gensym)
          bx (box 0)]
      @{:send-window initial-window
        :recv-window initial-window
        :futex-key key
        :futex-box bx
        :waiting false}))

  (defn consume-send-window [fc amount]
    (while (<= fc:send-window 0)
      (put fc :waiting true)
      (let [gen (unbox fc:futex-box)]
        (when (<= fc:send-window 0)
          (ev/futex-wait fc:futex-key fc:futex-box gen)))
      (put fc :waiting false))
    (let [actual (min amount fc:send-window)]
      (put fc :send-window (- fc:send-window actual))
      actual))

  (defn apply-window-update [fc increment]
    (let [new-window (+ fc:send-window increment)]
      (when (> new-window 2147483647)
        (error {:error :h2-error
                :reason :flow-control-error
                :message "flow control window overflow"}))
      (put fc :send-window new-window))
    (when fc:waiting
      (rebox fc:futex-box (inc (unbox fc:futex-box)))
      (ev/futex-wake fc:futex-key 999999999)))

  (defn consume-recv-window [fc amount]
    (put fc :recv-window (- fc:recv-window amount)))

  (defn replenish-recv-window [fc amount]
    (put fc :recv-window (+ fc:recv-window amount)))

  ## ── Tests ──────────────────────────────────────────────────────────────

  (defn run-tests []
    (let [s (make-stream 1 65535)]
      (assert (= s:state :idle) "stream: initial state")
      (stream-transition s :send-headers)
      (assert (= s:state :open) "stream: idle->open")
      (stream-transition s :send-end-stream)
      (assert (= s:state :half-closed-local) "stream: open->half-closed-local")
      (stream-transition s :recv-end-stream)
      (assert (= s:state :closed) "stream: half-closed-local->closed"))

    (let [s (make-stream 1 65535)]
      (stream-transition s :recv-headers)
      (assert (= s:state :open) "stream server: idle->open")
      (stream-transition s :recv-end-stream)
      (assert (= s:state :half-closed-remote)
              "stream server: open->half-closed-remote")
      (stream-transition s :send-end-stream)
      (assert (= s:state :closed) "stream server: half-closed-remote->closed"))

    (let [s (make-stream 3 65535)]
      (stream-transition s :send-headers)
      (stream-transition s :recv-rst)
      (assert (= s:state :closed) "stream: RST closes"))

    (let [s (make-stream 5 65535)]
      (stream-transition s :send-headers)
      (stream-transition s :send-end-stream)
      (let [[ok? err] (protect (stream-transition s :send-end-stream))]
        (assert (not ok?) "stream: invalid transition errors")))

    (let [s (make-stream 1 65535)]
      (assert (= s:flow:send-window 65535) "stream: flow control initial window")
      (let [consumed (consume-send-window s:flow 1000)]
        (assert (= consumed 1000) "stream: flow consume")
        (assert (= s:flow:send-window 64535) "stream: flow after consume"))
      (apply-window-update s:flow 1000)
      (assert (= s:flow:send-window 65535) "stream: flow after update"))

    (let [s (make-stream 1 65535)]
      (assert (nil? s:pending-headers) "stream: pending-headers nil initially"))

    (let [fc (make-flow-control 100)]
      (assert (= fc:send-window 100) "fc: initial send window")
      (let [consumed (consume-send-window fc 50)]
        (assert (= consumed 50) "fc: consumed 50")
        (assert (= fc:send-window 50) "fc: window after consume"))
      (consume-send-window fc 50)
      (assert (= fc:send-window 0) "fc: window at 0")
      (apply-window-update fc 200)
      (assert (= fc:send-window 200) "fc: window after update"))

    (let [fc (make-flow-control 2147483647)]
      (let [[ok? err] (protect (apply-window-update fc 1))]
        (assert (not ok?) "fc: overflow detection")))

    (let [fc (make-flow-control 65535)]
      (consume-recv-window fc 1000)
      (assert (= fc:recv-window 64535) "fc: recv consumed")
      (replenish-recv-window fc 1000)
      (assert (= fc:recv-window 65535) "fc: recv replenished"))

    true)

  {:make-stream make-stream
   :make-channel make-channel
   :transition stream-transition
   :make-flow-control make-flow-control
   :consume-send-window consume-send-window
   :apply-window-update apply-window-update
   :consume-recv-window consume-recv-window
   :replenish-recv-window replenish-recv-window
   :test run-tests})
