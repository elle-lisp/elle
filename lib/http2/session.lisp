(elle/epoch 9)
## lib/http2/session.lisp — shared HTTP/2 session management
##
## Loaded via:
##   (def session ((import "std/http2/session")
##                 :sync sync :frame frame :stream stream :hpack hpack))
##
## Exports: {:make-session :writer-loop :send-frame :send-settings
##           :send-settings-ack :send-window-update :send-goaway
##           :send-rst-stream :apply-remote-settings :get-stream
##           :notify-all-streams :send-headers-with-continuation
##           :send-data-with-flow-control :default-settings
##           :initial-window :max-frame :test}

(fn [&named sync frame stream hpack]

  (def C frame:constants)
  (def has-flag? frame:has-flag?)

  ## ── Default settings ───────────────────────────────────────────────────

  (def INITIAL-WINDOW (* 1024 1024))
  (def MAX-FRAME (* 256 1024))

  (def default-settings
    [[C:settings-initial-window-size INITIAL-WINDOW]
     [C:settings-max-frame-size      MAX-FRAME]
     [C:settings-enable-push         0]])

  ## ── Session constructor ────────────────────────────────────────────────

  (defn make-session [transport host is-server? &named scheme]
    @{:transport      transport
      :is-server?     is-server?
      :host           host
      :scheme         (or scheme "http")
      :streams        @{}
      :next-stream-id (if is-server? 2 1)
      :hpack-encoder  (hpack:make-encoder)
      :hpack-decoder  (hpack:make-decoder)
      :local-settings {:header-table-size 4096
                       :initial-window-size INITIAL-WINDOW
                       :max-frame-size MAX-FRAME
                       :max-concurrent-streams 100
                       :enable-push 0}
      :remote-settings {:header-table-size 4096
                        :initial-window-size 65535
                        :max-frame-size 16384
                        :max-concurrent-streams 100}
      :conn-flow      (stream:make-flow-control 65535)
      :write-queue    (sync:make-queue 256)
      :reader-fiber   nil
      :writer-fiber   nil
      :closed?        false
      :goaway-recvd?  false
      :last-stream-id 0
      :settings-ack-latch nil})

  ## ── Writer fiber ───────────────────────────────────────────────────────

  (defn writer-loop [session]
    "Drain write-queue and batch-write frames to transport.
     On write error: sets closed?, notifies all streams (defect 9)."
    (let [q session:write-queue
          t session:transport]
      (let [[ok? _] (protect
        (forever
          (let [item (q:take)]
            (when (= item :shutdown) (break nil))
            (let [[ftype flags sid payload] item]
              (frame:write-frame t ftype flags sid payload))
            (while (> (q:size) 0)
              (let [next (q:take)]
                (when (= next :shutdown) (break nil))
                (let [[ftype flags sid payload] next]
                  (frame:write-frame t ftype flags sid payload))))
            (t:flush))))]
        (unless ok?
          (put session :closed? true)
          (notify-all-streams session :writer-error)))))

  ## ── Send helpers ───────────────────────────────────────────────────────

  (defn send-frame [session ftype flags sid payload]
    "Enqueue a frame for the writer fiber."
    (session:write-queue:put [ftype flags sid payload]))

  (defn send-settings [session settings]
    "Send SETTINGS and start a 30s timeout for the ACK."
    (let [[ftype flags sid payload] (frame:make-settings-frame settings)]
      (send-frame session ftype flags sid payload))
    # Start SETTINGS timeout: if no ACK within 30s, send GOAWAY
    (let [latch (sync:make-latch)]
      (put session :settings-ack-latch latch)
      (ev/spawn
        (fn []
          (let [result (ev/timeout 30 (fn [] (latch:wait) :acked))]
            (when (nil? result)
              # Timeout: no ACK received
              (when (not session:closed?)
                (send-goaway session 0 C:err-settings-timeout)
                (session:write-queue:put :shutdown))))))))

  (defn send-settings-ack [session]
    (let [[ftype flags sid payload] (frame:make-settings-ack)]
      (send-frame session ftype flags sid payload)))

  (defn ack-settings-received [session]
    "Called when SETTINGS ACK is received. Opens the latch if pending."
    (when session:settings-ack-latch
      (session:settings-ack-latch:open)
      (put session :settings-ack-latch nil)))

  (defn send-window-update [session stream-id increment]
    (let [[ftype flags sid payload] (frame:make-window-update-frame stream-id increment)]
      (send-frame session ftype flags sid payload)))

  (defn send-goaway [session last-stream-id error-code &named debug-data]
    (let [[ftype flags sid payload]
          (frame:make-goaway-frame last-stream-id error-code :debug-data debug-data)]
      (send-frame session ftype flags sid payload)))

  (defn send-rst-stream [session stream-id error-code]
    (let [[ftype flags sid payload] (frame:make-rst-stream-frame stream-id error-code)]
      (send-frame session ftype flags sid payload)))

  ## ── HPACK encode + send with CONTINUATION ──────────────────────────────

  (defn send-headers-with-continuation [session sid header-pairs end-stream?]
    "HPACK-encode header-pairs, then send as HEADERS + CONTINUATION frames
     if the encoded block exceeds remote max-frame-size. The encode + all
     frame enqueues are non-yielding (atomic) to preserve HPACK state."
    (let* [h-block (hpack:encode session:hpack-encoder header-pairs)
           max-frame (get session:remote-settings :max-frame-size)
           total (length h-block)]
      (if (<= total max-frame)
        # Fits in one frame
        (let [[ft fl si pl]
              (frame:make-headers-frame sid h-block end-stream? true)]
          (send-frame session ft fl si pl))
        # Split across HEADERS + CONTINUATION(s)
        (begin
          (let* [first-chunk (slice h-block 0 max-frame)
                 [ft fl si pl]
                 (frame:make-headers-frame sid first-chunk end-stream? false)]
            (send-frame session ft fl si pl))
          (let [@offset max-frame]
            (while (< offset total)
              (let* [remaining (- total offset)
                     chunk-size (min remaining max-frame)
                     chunk (slice h-block offset (+ offset chunk-size))
                     end-headers? (= (+ offset chunk-size) total)
                     [ft fl si pl]
                     (frame:make-continuation-frame sid chunk end-headers?)]
                (send-frame session ft fl si pl)
                (assign offset (+ offset chunk-size)))))))))

  ## ── DATA with connection + stream flow control ─────────────────────────

  (defn send-data-with-flow-control [session sid s-flow body-bytes]
    "Send DATA frames respecting both connection and per-stream windows.
     body-bytes: the full body to send. Automatically chunks and sets
     END_STREAM on the last frame."
    (let* [max-frame (get session:remote-settings :max-frame-size)
           @offset 0
           total (length body-bytes)]
      (while (< offset total)
        (let* [remaining (- total offset)
               # Respect connection send window
               conn-allowed (stream:consume-send-window session:conn-flow
                              (min remaining max-frame))
               # Respect per-stream send window
               stream-allowed (stream:consume-send-window s-flow
                                conn-allowed)
               chunk (slice body-bytes offset (+ offset stream-allowed))
               end? (= (+ offset stream-allowed) total)
               [ft fl si pl] (frame:make-data-frame sid chunk end?)]
          (send-frame session ft fl si pl)
          (assign offset (+ offset stream-allowed))))))

  ## ── Apply remote settings ──────────────────────────────────────────────

  (defn apply-remote-settings [session settings-payload]
    "Parse and apply peer's SETTINGS frame. Adjusts existing stream
     windows by the delta (defect 13, RFC 9113 Section 6.9.2)."
    (let [entries (frame:parse-settings settings-payload)]
      (each entry in entries
        (cond
          (= entry:id C:settings-initial-window-size)
           (let [old-val (get session:remote-settings :initial-window-size)
                 new-val entry:value
                 delta (- new-val old-val)]
             (put session:remote-settings :initial-window-size new-val)
             # Adjust existing stream windows by the delta
             (when (not (= delta 0))
               (each sid in (keys session:streams)
                 (when-let [s (get session:streams sid)]
                   (if (> delta 0)
                     (stream:apply-window-update s:flow delta)
                     (let [lock s:flow:lock]
                       (lock:acquire)
                       (put s:flow :send-window (+ s:flow:send-window delta))
                       (lock:release)))))))
          (= entry:id C:settings-max-frame-size)
           (put session:remote-settings :max-frame-size entry:value)
          (= entry:id C:settings-header-table-size)
           (put session:remote-settings :header-table-size entry:value)
          (= entry:id C:settings-max-concurrent-streams)
           (put session:remote-settings :max-concurrent-streams entry:value)
          (= entry:id C:settings-enable-push)
           (put session:remote-settings :enable-push entry:value)))))

  ## ── Get or create stream ───────────────────────────────────────────────

  (defn get-stream [session stream-id]
    "Get stream by ID, creating if idle."
    (let [existing (get session:streams stream-id)]
      (if (nil? existing)
        (let [s (stream:make-stream stream-id
                  (get session:remote-settings :initial-window-size))]
          (put session:streams stream-id s)
          s)
        existing)))

  ## ── Notify all streams ─────────────────────────────────────────────────

  (defn notify-all-streams [session reason]
    "Push an error into every live stream's data-queue so blocked
     consumers wake up instead of hanging forever."
    (each sid in (keys session:streams)
      (when-let [s (get session:streams sid)]
        (protect
          (s:data-queue:put {:type :error
                             :error {:error :h2-error
                                     :reason reason
                                     :message "session closed"}}))))
    (put session :streams @{}))

  ## ── Tests ──────────────────────────────────────────────────────────────

  (defn run-tests []
    # ── Session constructor ──
    (let [mock-transport {:read nil :write nil :flush nil :close nil}
          s (make-session mock-transport "example.com" false)]
      (assert (= s:host "example.com") "session: host")
      (assert (= s:is-server? false) "session: client")
      (assert (= s:next-stream-id 1) "session: client starts at 1")
      (assert (= s:closed? false) "session: not closed"))

    (let [mock-transport {:read nil :write nil :flush nil :close nil}
          s (make-session mock-transport "example.com" true)]
      (assert (= s:next-stream-id 2) "session: server starts at 2"))

    # ── send-headers-with-continuation splits ──
    # Test that small headers produce one frame
    # (Can't fully test without a real session, but we test the encoder path)

    # ── apply-remote-settings window adjustment ──
    (let [mock-transport {:read nil :write nil :flush nil :close nil}
          sess (make-session mock-transport "test" false)
          s1 (get-stream sess 1)]
      # Initial remote window is 65535
      (assert (= s1:flow:send-window 65535) "settings: initial stream window")
      # Simulate SETTINGS with new initial-window-size of 70000
      (let [payload (concat (frame:u16->bytes C:settings-initial-window-size)
                            (frame:u32->bytes 70000))]
        (apply-remote-settings sess payload))
      # Stream window should have increased by delta (70000 - 65535 = 4465)
      (assert (= s1:flow:send-window 70000)
              (concat "settings: adjusted window " (string s1:flow:send-window))))

    true)

  ## ── Exports ────────────────────────────────────────────────────────────

  {:make-session      make-session
   :writer-loop       writer-loop
   :send-frame        send-frame
   :send-settings     send-settings
   :send-settings-ack send-settings-ack
   :ack-settings-received ack-settings-received
   :send-window-update send-window-update
   :send-goaway       send-goaway
   :send-rst-stream   send-rst-stream
   :apply-remote-settings apply-remote-settings
   :get-stream        get-stream
   :notify-all-streams notify-all-streams
   :send-headers-with-continuation send-headers-with-continuation
   :send-data-with-flow-control send-data-with-flow-control
   :default-settings  default-settings
   :initial-window    INITIAL-WINDOW
   :max-frame         MAX-FRAME
   :test              run-tests})
