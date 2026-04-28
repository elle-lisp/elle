(elle/epoch 9)
## lib/http2/server.lisp — HTTP/2 server connection handler
##
## Loaded via:
##   (def server ((import "std/http2/server")
##                :sync sync :hpack hpack :frame frame :stream stream
##                :session session :tls tls))
##
## Exports: {:serve :test}

(fn [&named sync hpack frame stream session tls]

  (def C frame:constants)
  (def has-flag? frame:has-flag?)

  ## ── Server request handler ─────────────────────────────────────────────

  (defn handle-server-request [sess s sid hdrs end? handler]
    "Handle one HTTP/2 server request on stream s.
     Wrapped in protect+defer by caller for error handling and cleanup."
    # Collect body if not end-stream
    (def @body-parts @[])
    (unless end?
      (forever
        (let [msg (s:data-queue:take)]
          (cond
            (= msg:type :data)
             (begin (push body-parts msg:data)
                    (when msg:end-stream (break nil)))
            (= msg:type :error) (error msg:error)
            true (break nil)))))
    # Build request struct
    (let* [method-pair (first (filter (fn [h] (= (get h 0) ":method")) hdrs))
           path-pair (first (filter (fn [h] (= (get h 0) ":path")) hdrs))
           req-headers @{}
           _ (each h in hdrs
               (let [name (get h 0)]
                 (unless (string/starts-with? name ":")
                   (put req-headers (keyword (slice name 0)) (get h 1)))))
           body-val (if (empty? body-parts)
                      nil
                      (apply concat (freeze body-parts)))
           request {:method  (if method-pair (get method-pair 1) "GET")
                    :path    (if path-pair (get path-pair 1) "/")
                    :headers (freeze req-headers)
                    :body    body-val}
           [ok? response] (protect (handler request))]
      (if ok?
        # Send response — stream is in :open or :half-closed-remote
        # from the reader's recv-headers/recv-end-stream; no :send-headers needed
        (let* [status (string (or response:status 200))
               resp-headers (or response:headers {})
               resp-body (if (nil? response:body)
                           (bytes)
                           (if (string? response:body)
                             (bytes response:body)
                             response:body))
               @h-pairs @[[":status" status]]
               _ (each k in (keys resp-headers)
                   (push h-pairs [(string k) (string (get resp-headers k))]))
               has-body (> (length resp-body) 0)]
          # Send HEADERS (with CONTINUATION if needed)
          (session:send-headers-with-continuation sess sid h-pairs (not has-body))
          # Send DATA with flow control
          (when has-body
            (session:send-data-with-flow-control sess sid s:flow resp-body))
          (stream:transition s :send-end-stream))
        # Handler error: send 500 response, re-raise for logging
        (begin
          (let [h-pairs [[":status" "500"]]
                err response]
            (session:send-headers-with-continuation sess sid h-pairs true)
            (stream:transition s :send-end-stream)
            (error err))))))

  ## ── Server connection ──────────────────────────────────────────────────

  (defn server-connection [transport handler sess &named on-error]
    "Handle one HTTP/2 server connection."
    # Read client preface
    (let [preface (frame:read-exact transport 24)]
      (when (or (nil? preface) (not (= preface C:client-preface)))
        (error {:error :h2-error :reason :protocol-error
                :message "invalid client connection preface"})))
    # Read client SETTINGS
    (let [f (frame:read-frame transport (get sess:local-settings :max-frame-size))]
      (when (or (nil? f) (not (= f:type C:type-settings)))
        (error {:error :h2-error :reason :protocol-error
                :message "expected SETTINGS as first client frame"}))
      (session:apply-remote-settings sess f:payload))
    # Send our SETTINGS + ACK — write directly before writer starts
    (let [[ftype flags sid payload] (frame:make-settings-frame session:default-settings)]
      (frame:write-frame transport ftype flags sid payload))
    (let [[ftype flags sid payload] (frame:make-settings-ack)]
      (frame:write-frame transport ftype flags sid payload))
    (transport:flush)
    # Start writer fiber
    (put sess :writer-fiber (ev/spawn (fn [] (session:writer-loop sess))))
    # Reader loop — handles requests by spawning fibers
    (let [max-size (get sess:local-settings :max-frame-size)]
      (forever
        (let [[ok? f] (protect (frame:read-frame transport max-size))]
          (when (or (not ok?) (nil? f))
            (sess:write-queue:put :shutdown)
            (break nil))
          (let [ftype f:type
                flags f:flags
                sid   f:stream-id
                payload f:payload]
            (cond
              (= ftype C:type-settings)
               (if (has-flag? flags C:flag-ack)
                 (session:ack-settings-received sess)
                 (begin
                   (session:apply-remote-settings sess payload)
                   (session:send-settings-ack sess)))

              (= ftype C:type-ping)
               (unless (has-flag? flags C:flag-ack)
                 (let [[ft fl si pl] (frame:make-ping-frame payload :ack? true)]
                   (session:send-frame sess ft fl si pl)))

              (= ftype C:type-window-update)
               (let [increment (bit/and (frame:read-u32 payload 0) 0x7fffffff)]
                 (if (= sid 0)
                   (stream:apply-window-update sess:conn-flow increment)
                   (when-let [s (get sess:streams sid)]
                     (stream:apply-window-update s:flow increment))))

              (= ftype C:type-goaway)
               (begin (sess:write-queue:put :shutdown) (break nil))

              (= ftype C:type-headers)
               (let [max-streams (get sess:local-settings :max-concurrent-streams)
                     active (length (keys sess:streams))
                     existing (get sess:streams sid)]
                 (if (and (nil? existing) (>= active max-streams))
                   # Refuse stream: max-concurrent-streams exceeded
                   (session:send-rst-stream sess sid C:err-refused-stream)
                   # Accept and process the HEADERS
                   (let [s (session:get-stream sess sid)]
                     (when (= s:state :idle)
                       (stream:transition s :recv-headers))
                     (if (has-flag? flags C:flag-end-headers)
                       (let [hdrs (hpack:decode sess:hpack-decoder payload)
                             end? (has-flag? flags C:flag-end-stream)]
                         (put s :headers hdrs)
                         (when end? (stream:transition s :recv-end-stream))
                         (ev/spawn
                           (fn []
                             (defer (del sess:streams sid)
                               (let [[ok? err] (protect
                                 (handle-server-request sess s sid hdrs end? handler))]
                                 (unless ok?
                                   (protect
                                     (session:send-rst-stream sess sid C:err-internal-error))
                                   (when on-error (on-error err))))))))
                       # No END_HEADERS — buffer for CONTINUATION
                       (begin
                         (when (has-flag? flags C:flag-end-stream)
                           (stream:transition s :recv-end-stream))
                         (put s :pending-headers
                              @{:data payload
                                :end-stream (has-flag? flags C:flag-end-stream)}))))))

              (= ftype C:type-continuation)
               (when-let [s (get sess:streams sid)]
                 (when s:pending-headers
                   (put s:pending-headers :data
                        (concat s:pending-headers:data payload))
                   (when (has-flag? flags C:flag-end-headers)
                     (let [hdrs (hpack:decode sess:hpack-decoder s:pending-headers:data)
                           end? s:pending-headers:end-stream]
                       (put s :pending-headers nil)
                       (put s :headers hdrs)
                       (ev/spawn
                         (fn []
                           (defer (del sess:streams sid)
                             (let [[ok? err] (protect
                               (handle-server-request sess s sid hdrs end? handler))]
                               (unless ok?
                                 (protect
                                   (session:send-rst-stream sess sid C:err-internal-error))
                                 (when on-error (on-error err)))))))))))

              (= ftype C:type-data)
               (begin
                 (let [len (length payload)]
                   (when (> len 0)
                     (session:send-window-update sess 0 len)))
                 (when-let [s (get sess:streams sid)]
                   (let [end? (has-flag? flags C:flag-end-stream)]
                     (s:data-queue:put {:type :data :data payload
                                        :end-stream end?})
                     (when end? (stream:transition s :recv-end-stream))
                     (let [len (length payload)]
                       (when (> len 0)
                         (session:send-window-update sess sid len))))))

              (= ftype C:type-rst-stream)
               (when-let [s (get sess:streams sid)]
                 (stream:transition s :recv-rst)
                 (s:data-queue:put {:type :rst :code (frame:read-u32 payload 0)})
                 (del sess:streams sid))

              true nil))))))

  ## ── h2-serve ───────────────────────────────────────────────────────────

  (defn tcp-transport [port]
    "Wrap a TCP port as a transport with buffered binary writes."
    (def @wbuf-parts @[])
    {:read  (fn [n] (port/read port n))
     :write (fn [data]
              (let [d (if (bytes? data) data (bytes data))]
                (push wbuf-parts d)))
     :flush (fn []
              (when (> (length wbuf-parts) 0)
                (let [combined (apply concat (freeze wbuf-parts))]
                  (port/write port combined)
                  (assign wbuf-parts @[]))))
     :close (fn [] (port/close port))})

  (defn tls-transport [conn]
    "Wrap a TLS connection as a transport."
    {:read  (fn [n] (tls:read conn n))
     :write (fn [data] (tls:write conn data))
     :flush (fn [] nil)
     :close (fn [] (tls:close conn))})

  (defn h2-serve [listener handler &named tls-config on-error]
    "Serve HTTP/2 connections. Runs forever.
     listener: from (tcp/listen host port).
     handler:  (fn [request] response).
     Optional :tls-config for h2 over TLS, :on-error for error callback."
    (forever
      (let* [tcp-port (tcp/accept listener)
             transport (if tls-config
                         (begin
                           (when (nil? tls)
                             (error {:error :h2-error :reason :tls-not-configured
                                     :message "TLS serving requires :tls plugin"}))
                           (tls-transport (tls:accept listener tls-config)))
                         (tcp-transport tcp-port))
             sess (session:make-session transport "" true)]
        (ev/spawn
          (fn []
            (let [[ok? err] (protect
              (server-connection transport handler sess :on-error on-error))]
              (unless ok?
                (when on-error (on-error err)))
              (protect (transport:close))))))))

  ## ── Tests ──────────────────────────────────────────────────────────────

  (defn run-tests []
    # Basic smoke test: these are exercised more thoroughly by tests/h2-server.lisp
    true)

  ## ── Exports ────────────────────────────────────────────────────────────

  {:serve h2-serve
   :test  run-tests})
