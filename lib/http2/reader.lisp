(elle/epoch 12)
# audited: 2026-09-14
## lib/http2/reader.lisp — the frame reader both an h2 client and an h2 server run
##
## Loaded via:
##   (def reader ((import "std/http2/reader")
##                :frame frame :stream stream :hpack hpack :session session))
##
## Exports: {:read-loop :test}
##
## Every frame type is handled the same way for either role, so the loop
## lives here once and takes the two callbacks that differ. It reads the
## session and calls the send side through `session`, which is why the
## dependency runs this way and not the other: `session` knows nothing
## about the reader.
##
## lib/http2/overview.md

(fn [&named frame stream hpack session]
  (def C frame:constants)
  (def has-flag? frame:has-flag?)

  ## ── Strip PADDED flag padding ──────────────────────────────────────────

  (defn strip-padding [payload flags]
    "Strip padding from DATA/HEADERS payload when FLAG_PADDED is set."
    (if (has-flag? flags C:flag-padded)
      (let [pad-len (get payload 0)]
        (when (>= pad-len (length payload))
          (error {:error :h2-error
                  :reason :protocol-error
                  :message "padding length exceeds payload"}))
        (slice payload 1 (- (length payload) pad-len)))
      payload))

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
            (session:notify-all-streams sess :transport-error)
            (break nil))
          (when (nil? f)
            (put sess :closed? true)
            (sess:write-queue:put :shutdown)
            (session:notify-all-streams sess :eof)
            (break nil))
          (let [ftype f:type
                flags f:flags
                sid f:stream-id
                payload f:payload]
            (when sess:expecting-continuation-sid
              (unless (and (= ftype C:type-continuation)
                           (= sid sess:expecting-continuation-sid))
                (session:send-goaway sess 0 C:err-protocol-error)
                (break nil)))

            (cond  ## ── SETTINGS ──
              (= ftype C:type-settings)
                (begin  # §6.5: SETTINGS on non-zero stream → PROTOCOL_ERROR
                  (when (not (= sid 0))
                    (session:send-goaway sess 0 C:err-protocol-error)
                    (break nil))
                  (if (has-flag? flags C:flag-ack)
                    (session:ack-settings-received sess)
                    (begin
                      (session:apply-remote-settings sess payload)
                      (session:send-settings-ack sess))))

              ## ── PING ──
              (= ftype C:type-ping)
                (begin  # §6.7: PING on non-zero stream → PROTOCOL_ERROR
                  (when (not (= sid 0))
                    (session:send-goaway sess 0 C:err-protocol-error)
                    (break nil))  # §6.7: PING payload MUST be 8 octets
                  (when (not (= (length payload) 8))
                    (session:send-goaway sess 0 C:err-frame-size-error)
                    (break nil))
                  (unless (has-flag? flags C:flag-ack)
                    (let [[ftype flags sid payload] (frame:make-ping-frame payload
                          :ack? true)]
                      (session:send-frame sess ftype flags sid payload))))

              ## ── GOAWAY ──
              (= ftype C:type-goaway)
                (begin  # §6.8: GOAWAY on non-zero stream → PROTOCOL_ERROR
                  (when (not (= sid 0))
                    (session:send-goaway sess 0 C:err-protocol-error)
                    (break nil))
                  (when (on-goaway sess payload) (break nil)))

              ## ── WINDOW_UPDATE ──
              (= ftype C:type-window-update)
                (let [increment (bit/and (frame:read-u32 payload 0) 0x7fffffff)]
                  (when (= increment 0)
                    (if (= sid 0)
                      (begin
                        (session:send-goaway sess 0 C:err-protocol-error)
                        (break nil))
                      (session:send-rst-stream sess sid C:err-protocol-error)))
                  (when (> increment 0)
                    (if (= sid 0)
                      (stream:apply-window-update sess:conn-flow increment)
                      (when-let [s (get sess:streams sid)]
                                (stream:apply-window-update s:flow increment)))))

              ## ── RST_STREAM ──
              (= ftype C:type-rst-stream)
                (begin  # §6.4: RST_STREAM on stream 0 → PROTOCOL_ERROR
                  (when (= sid 0)
                    (session:send-goaway sess 0 C:err-protocol-error)
                    (break nil))
                  (when-let [s (get sess:streams sid)]
                            (stream:transition s :recv-rst)
                            (let [err-code (frame:read-u32 payload 0)]
                              (put s :error-code err-code)
                              (s:data-queue:put {:type :rst :code err-code}))  # Flush pending stream WU before removing
                            (when (> (or s:pending-stream-wu 0) 0)
                              (session:send-window-update sess sid
                              s:pending-stream-wu)) (del sess:streams sid)))

              ## ── HEADERS ──
              (= ftype C:type-headers)
                (begin  # §6.2: HEADERS on stream 0 → PROTOCOL_ERROR
                  (when (= sid 0)
                    (session:send-goaway sess 0 C:err-protocol-error)
                    (break nil))
                  (let [s (session:get-stream sess sid)
                        payload (strip-padding payload flags)]
                    (when (= s:state :idle) (stream:transition s :recv-headers))
                    (if (has-flag? flags C:flag-end-headers)
                      (let [headers (hpack:decode sess:hpack-decoder payload)
                            end? (has-flag? flags C:flag-end-stream)]
                        (on-headers sess s sid headers end?))
                      (begin  # §6.2: track continuation expectation
                        (put sess :expecting-continuation-sid sid)
                        (when (has-flag? flags C:flag-end-stream)
                          (stream:transition s :recv-end-stream))
                        (put s
                             :pending-headers @{:data payload
                             :end-stream (has-flag? flags C:flag-end-stream)})))))

              ## ── CONTINUATION ──
              (= ftype C:type-continuation)
                (begin  # §6.10: CONTINUATION on stream 0 → PROTOCOL_ERROR
                  (when (= sid 0)
                    (session:send-goaway sess 0 C:err-protocol-error)
                    (break nil))  # §6.10: CONTINUATION without pending HEADERS → PROTOCOL_ERROR
                  (unless sess:expecting-continuation-sid
                    (session:send-goaway sess 0 C:err-protocol-error)
                    (break nil))
                  (when-let [s (get sess:streams sid)]
                            (when s:pending-headers
                              (put s:pending-headers
                                   :data (concat s:pending-headers:data payload))
                              (when (has-flag? flags C:flag-end-headers)
                                (put sess :expecting-continuation-sid nil)
                                (let [headers (hpack:decode sess:hpack-decoder
                                      s:pending-headers:data)
                                      end? s:pending-headers:end-stream]
                                  (put s :pending-headers nil)
                                  (on-headers sess s sid headers end?))))))

              ## ── DATA ──
              (= ftype C:type-data)
                (begin  # §6.1: DATA on stream 0 → PROTOCOL_ERROR
                  (when (= sid 0)
                    (session:send-goaway sess 0 C:err-protocol-error)
                    (break nil))
                  (let [payload (strip-padding payload flags)]
                    (let [len (length payload)]
                      (when (> len 0)
                        (put sess :pending-conn-wu (+ sess:pending-conn-wu len))))

                    # §5.1: DATA on half-closed(remote) or closed → stream
                    # error
                    (when-let [s (get sess:streams sid)]
                              (when (or (= s:state :half-closed-remote)
                                        (= s:state :closed))
                                (session:send-rst-stream sess sid
                                C:err-stream-closed))
                              (let [end? (has-flag? flags C:flag-end-stream)
                                    len (length payload)]
                                (when (> len 0)
                                  (put s
                                       :pending-stream-wu (+ (or s:pending-stream-wu
                                       0) len)))
                                (when (>= sess:pending-conn-wu sess:wu-threshold)
                                  (session:send-window-update sess 0
                                  sess:pending-conn-wu)
                                  (put sess :pending-conn-wu 0))
                                (when (>= (or s:pending-stream-wu 0)
                                  sess:wu-threshold)
                                  (session:send-window-update sess sid
                                  s:pending-stream-wu)
                                  (put s :pending-stream-wu 0))
                                (when end?  # Flush remaining WU before closing stream
                                  (when (> sess:pending-conn-wu 0)
                                    (session:send-window-update sess 0
                                    sess:pending-conn-wu)
                                    (put sess :pending-conn-wu 0))
                                  (when (> (or s:pending-stream-wu 0) 0)
                                    (session:send-window-update sess sid
                                    s:pending-stream-wu)
                                    (put s :pending-stream-wu 0)))

                                # Deliver data (may block on full queue —
                                # but WU is already enqueued above)
                                (s:data-queue:put {:type :data
                                :data payload
                                :end-stream end?})
                                (when sess:closed? (break nil))
                                (when end?
                                  (stream:transition s :recv-end-stream)  # Only remove stream when fully closed
                                  # (both sides done). Half-closed-remote
                                  # means our side still needs to send +
                                  # receive WU for flow control.
                                  (when (= s:state :closed)
                                    (del sess:streams sid)))))))

              ## ── PUSH_PROMISE — reject ──
              (= ftype C:type-push-promise) (session:send-rst-stream sess sid
              C:err-refused-stream)

              ## ── Unknown — ignore ──
              true nil))))  # Ensure writer always gets shutdown signal after loop exit
      (sess:write-queue:put :shutdown)))


  ## ── Tests ──────────────────────────────────────────────────────────────

  (defn run-tests []
    (let [padded (bytes 3 0x41 0x42 0x43 0x00 0x00 0x00)
          stripped (strip-padding padded C:flag-padded)]
      (assert (= stripped (bytes 0x41 0x42 0x43)) "strip-padding: basic"))

    (let [unpadded (bytes 0x41 0x42 0x43)
          stripped (strip-padding unpadded 0)]
      (assert (= stripped unpadded) "strip-padding: no flag"))

    true)

  ## ── Exports ────────────────────────────────────────────────────────────

  {:read-loop read-loop :test run-tests})
