(elle/epoch 9)
## lib/http2/session.lisp — shared HTTP/2 session management
##
## Loaded via:
##   (def session ((import "std/http2/session")
##                 :sync sync :frame frame :stream stream :hpack hpack))
##
## Exports: {:make-session :writer-loop :read-loop :send-frame :send-settings
##           :send-settings-ack :send-window-update :send-goaway
##           :send-rst-stream :apply-remote-settings :get-stream
##           :notify-all-streams :encode-and-send-headers
##           :send-data-with-flow-control :ack-settings-received
##           :default-settings :initial-window :max-frame :test}

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
      :local-settings @{:header-table-size 4096
                        :initial-window-size INITIAL-WINDOW
                        :max-frame-size MAX-FRAME
                        :max-concurrent-streams 100
                        :enable-push 0}
      :remote-settings @{:header-table-size 4096
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
     On write error: sets closed?, notifies all streams."
    (let [q session:write-queue
          t session:transport
          @shutting-down false]
      (let [[ok? _] (protect
        (forever
          (let [item (q:take)]
            (when (= item :shutdown) (assign shutting-down true))
            (unless shutting-down
              (let [[ftype flags sid payload] item]
                (frame:write-frame t ftype flags sid payload)))
            (while (> (q:size) 0)
              (let [next (q:take)]
                (when (= next :shutdown) (assign shutting-down true))
                (unless shutting-down
                  (let [[ftype flags sid payload] next]
                    (frame:write-frame t ftype flags sid payload)))))
            (unless shutting-down (t:flush))
            (when shutting-down (break nil)))))]
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
    (let [latch (sync:make-latch)]
      (put session :settings-ack-latch latch)
      (ev/spawn
        (fn []
          (let [result (ev/timeout 30 (fn [] (latch:wait) :acked))]
            (when (nil? result)
              (when (not session:closed?)
                (let [[ok? _] (protect
                  (send-goaway session 0 C:err-settings-timeout))]
                  (when ok?
                    (session:write-queue:put :shutdown))))))))))

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

  (defn encode-and-send-headers [session sid header-pairs end-stream?]
    "HPACK-encode header-pairs, then send as HEADERS + CONTINUATION frames
     if the encoded block exceeds remote max-frame-size. The encode + all
     frame enqueues are non-yielding (atomic) to preserve HPACK state."
    (let* [h-block (hpack:encode session:hpack-encoder (freeze header-pairs))
           max-frame (get session:remote-settings :max-frame-size)
           total (length h-block)]
      (if (<= total max-frame)
        (let [[ft fl si pl]
              (frame:make-headers-frame sid h-block end-stream? true)]
          (send-frame session ft fl si pl))
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
     Computes allowed = min(remaining, max-frame, conn-window, stream-window)
     atomically to avoid window leaks."
    (let* [max-frame (get session:remote-settings :max-frame-size)
           @offset 0
           total (length body-bytes)]
      (while (< offset total)
        (let* [remaining (- total offset)
               want (min remaining max-frame)
               conn-allowed (stream:consume-send-window session:conn-flow want)
               allowed (stream:consume-send-window s-flow conn-allowed)]
          (assert (> allowed 0) "flow control: allowed must be > 0")
          # If stream consumed less than connection, refund the difference
          (when (< allowed conn-allowed)
            (stream:apply-window-update session:conn-flow (- conn-allowed allowed)))
          (let* [chunk (slice body-bytes offset (+ offset allowed))
                 end? (= (+ offset allowed) total)
                 [ft fl si pl] (frame:make-data-frame sid chunk end?)]
            (send-frame session ft fl si pl)
            (assign offset (+ offset allowed)))))))

  ## ── Strip PADDED flag padding ──────────────────────────────────────────

  (defn strip-padding [payload flags]
    "Strip padding from DATA/HEADERS payload when FLAG_PADDED is set."
    (if (has-flag? flags C:flag-padded)
      (let [pad-len (get payload 0)]
        (when (>= pad-len (length payload))
          (error {:error :h2-error :reason :protocol-error
                  :message "padding length exceeds payload"}))
        (slice payload 1 (- (length payload) pad-len)))
      payload))

  ## ── Apply remote settings ──────────────────────────────────────────────

  (defn apply-remote-settings [session settings-payload]
    "Parse and apply peer's SETTINGS frame. Adjusts existing stream
     windows by the delta (RFC 9113 Section 6.9.2).
     Validates setting values per RFC 9113 Section 6.5.2."
    (let [entries (frame:parse-settings settings-payload)]
      (each entry in entries
        (cond
          (= entry:id C:settings-initial-window-size)
           (begin
             (when (> entry:value 2147483647)
               (error {:error :h2-error :reason :flow-control-error
                       :code C:err-flow-control-error
                       :message "INITIAL_WINDOW_SIZE exceeds 2^31-1"}))
             (let [old-val (get session:remote-settings :initial-window-size)
                   new-val entry:value
                   delta (- new-val old-val)]
               (put session:remote-settings :initial-window-size new-val)
               (when (not (= delta 0))
                 (each sid in (keys session:streams)
                   (when-let [s (get session:streams sid)]
                     (if (> delta 0)
                       (stream:apply-window-update s:flow delta)
                       (let [lock s:flow:lock]
                         (lock:acquire)
                         (put s:flow :send-window (+ s:flow:send-window delta))
                         (lock:release))))))))
          (= entry:id C:settings-max-frame-size)
           (begin
             (when (or (< entry:value 16384) (> entry:value 16777215))
               (error {:error :h2-error :reason :protocol-error
                       :code C:err-protocol-error
                       :message "MAX_FRAME_SIZE outside valid range 16384..16777215"}))
             (put session:remote-settings :max-frame-size entry:value))
          (= entry:id C:settings-header-table-size)
           (begin
             (put session:remote-settings :header-table-size entry:value)
             (hpack:set-encoder-table-size session:hpack-encoder entry:value))
          (= entry:id C:settings-max-concurrent-streams)
           (put session:remote-settings :max-concurrent-streams entry:value)
          (= entry:id C:settings-enable-push)
           (begin
             (when (and (not (= entry:value 0)) (not (= entry:value 1)))
               (error {:error :h2-error :reason :protocol-error
                       :code C:err-protocol-error
                       :message "ENABLE_PUSH must be 0 or 1"}))
             (put session:remote-settings :enable-push entry:value))))))

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

  ## ── Shared reader loop ─────────────────────────────────────────────────

  (defn read-loop [sess &named on-headers on-goaway]
    "Read frames from transport and dispatch. Takes callbacks:
     on-headers: (fn [sess s sid hdrs end?]) — client enqueues, server spawns handler
     on-goaway:  (fn [sess payload])         — client tracks state, server shuts down"
    (let [t sess:transport
          max-size (get sess:local-settings :max-frame-size)]
      (forever
        (let [[ok? f] (protect (frame:read-frame t max-size))]
          (when (not ok?)
            (put sess :closed? true)
            (sess:write-queue:put :shutdown)
            (notify-all-streams sess :transport-error)
            (break nil))
          (when (nil? f)
            (put sess :closed? true)
            (sess:write-queue:put :shutdown)
            (notify-all-streams sess :eof)
            (break nil))
          (let [ftype f:type
                flags f:flags
                sid   f:stream-id
                payload f:payload]
            (cond
              ## ── SETTINGS ──
              (= ftype C:type-settings)
               (if (has-flag? flags C:flag-ack)
                 (ack-settings-received sess)
                 (begin
                   (apply-remote-settings sess payload)
                   (send-settings-ack sess)))

              ## ── PING ──
              (= ftype C:type-ping)
               (unless (has-flag? flags C:flag-ack)
                 (let [[ftype flags sid payload]
                       (frame:make-ping-frame payload :ack? true)]
                   (send-frame sess ftype flags sid payload)))

              ## ── GOAWAY ──
              (= ftype C:type-goaway)
               (when (on-goaway sess payload)
                 (break nil))

              ## ── WINDOW_UPDATE ──
              (= ftype C:type-window-update)
               (let [increment (bit/and (frame:read-u32 payload 0) 0x7fffffff)]
                 (when (= increment 0)
                   (if (= sid 0)
                     (begin
                       (send-goaway sess 0 C:err-protocol-error)
                       (break nil))
                     (send-rst-stream sess sid C:err-protocol-error)))
                 (when (> increment 0)
                   (if (= sid 0)
                     (stream:apply-window-update sess:conn-flow increment)
                     (when-let [s (get sess:streams sid)]
                       (stream:apply-window-update s:flow increment)))))

              ## ── RST_STREAM ──
              (= ftype C:type-rst-stream)
               (when-let [s (get sess:streams sid)]
                 (stream:transition s :recv-rst)
                 (let [err-code (frame:read-u32 payload 0)]
                   (put s :error-code err-code)
                   (s:data-queue:put {:type :rst :code err-code}))
                 (del sess:streams sid))

              ## ── HEADERS ──
              (= ftype C:type-headers)
               (let [s (get-stream sess sid)
                     payload (strip-padding payload flags)]
                 (when (= s:state :idle)
                   (stream:transition s :recv-headers))
                 (if (has-flag? flags C:flag-end-headers)
                   (let [headers (hpack:decode sess:hpack-decoder payload)
                         end? (has-flag? flags C:flag-end-stream)]
                     (on-headers sess s sid headers end?))
                   (begin
                     (when (has-flag? flags C:flag-end-stream)
                       (stream:transition s :recv-end-stream))
                     (put s :pending-headers
                          @{:data payload
                            :end-stream (has-flag? flags C:flag-end-stream)}))))

              ## ── CONTINUATION ──
              (= ftype C:type-continuation)
               (when-let [s (get sess:streams sid)]
                 (when s:pending-headers
                   (put s:pending-headers :data
                        (concat s:pending-headers:data payload))
                   (when (has-flag? flags C:flag-end-headers)
                     (let [headers (hpack:decode sess:hpack-decoder s:pending-headers:data)
                           end? s:pending-headers:end-stream]
                       (put s :pending-headers nil)
                       (on-headers sess s sid headers end?)))))

              ## ── DATA ──
              (= ftype C:type-data)
               (let [payload (strip-padding payload flags)]
                 (let [len (length payload)]
                   (when (> len 0)
                     (send-window-update sess 0 len)))
                 (when-let [s (get sess:streams sid)]
                   (let [end? (has-flag? flags C:flag-end-stream)]
                     (s:data-queue:put {:type :data :data payload
                                        :end-stream end?})
                     (when end? (stream:transition s :recv-end-stream))
                     (let [len (length payload)]
                       (when (> len 0)
                         (send-window-update sess sid len)))
                     (when end? (del sess:streams sid)))))

              ## ── PUSH_PROMISE — reject ──
              (= ftype C:type-push-promise)
               (send-rst-stream sess sid C:err-refused-stream)

              ## ── Unknown — ignore ──
              true nil))))))

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

    # ── apply-remote-settings window adjustment ──
    (let [mock-transport {:read nil :write nil :flush nil :close nil}
          sess (make-session mock-transport "test" false)
          s1 (get-stream sess 1)]
      (assert (= s1:flow:send-window 65535) "settings: initial stream window")
      (let [payload (concat (frame:u16->bytes C:settings-initial-window-size)
                            (frame:u32->bytes 70000))]
        (apply-remote-settings sess payload))
      (assert (= s1:flow:send-window 70000)
              (concat "settings: adjusted window " (string s1:flow:send-window))))

    # ── SETTINGS validation: ENABLE_PUSH ──
    (let [mock-transport {:read nil :write nil :flush nil :close nil}
          sess (make-session mock-transport "test" false)
          payload (concat (frame:u16->bytes C:settings-enable-push)
                          (frame:u32->bytes 2))]
      (let [[ok? _] (protect (apply-remote-settings sess payload))]
        (assert (not ok?) "settings: ENABLE_PUSH=2 rejected")))

    # ── SETTINGS validation: MAX_FRAME_SIZE ──
    (let [mock-transport {:read nil :write nil :flush nil :close nil}
          sess (make-session mock-transport "test" false)
          payload (concat (frame:u16->bytes C:settings-max-frame-size)
                          (frame:u32->bytes 100))]
      (let [[ok? _] (protect (apply-remote-settings sess payload))]
        (assert (not ok?) "settings: MAX_FRAME_SIZE=100 rejected")))

    # ── SETTINGS validation: INITIAL_WINDOW_SIZE ──
    (let [mock-transport {:read nil :write nil :flush nil :close nil}
          sess (make-session mock-transport "test" false)
          payload (concat (frame:u16->bytes C:settings-initial-window-size)
                          (frame:u32->bytes 2147483648))]
      (let [[ok? _] (protect (apply-remote-settings sess payload))]
        (assert (not ok?) "settings: INITIAL_WINDOW_SIZE > 2^31-1 rejected")))

    # ── SETTINGS_HEADER_TABLE_SIZE updates encoder ──
    (let [mock-transport {:read nil :write nil :flush nil :close nil}
          sess (make-session mock-transport "test" false)
          payload (concat (frame:u16->bytes C:settings-header-table-size)
                          (frame:u32->bytes 2048))]
      (assert (= sess:hpack-encoder:table:max-size 4096)
              "settings: encoder table starts at 4096")
      (apply-remote-settings sess payload)
      (assert (= sess:hpack-encoder:table:max-size 2048)
              "settings: encoder table resized to 2048"))

    # ── strip-padding ──
    (let [padded (bytes 3 0x41 0x42 0x43 0x00 0x00 0x00)
          stripped (strip-padding padded C:flag-padded)]
      (assert (= stripped (bytes 0x41 0x42 0x43)) "strip-padding: basic"))

    (let [unpadded (bytes 0x41 0x42 0x43)
          stripped (strip-padding unpadded 0)]
      (assert (= stripped unpadded) "strip-padding: no flag"))

    true)

  ## ── Exports ────────────────────────────────────────────────────────────

  {:make-session      make-session
   :writer-loop       writer-loop
   :read-loop         read-loop
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
   :encode-and-send-headers encode-and-send-headers
   :send-data-with-flow-control send-data-with-flow-control
   :default-settings  default-settings
   :initial-window    INITIAL-WINDOW
   :max-frame         MAX-FRAME
   :test              run-tests})
