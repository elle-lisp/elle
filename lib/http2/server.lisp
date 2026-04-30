(elle/epoch 9)
## lib/http2/server.lisp — HTTP/2 server connection handler
##
## Loaded via:
##   (def server ((import "std/http2/server")
##                :sync sync :hpack hpack :frame frame :stream stream
##                :session session :tls tls :transport transport))
##
## Exports: {:serve :test}

(fn [&named sync hpack frame stream session tls transport]
  (def C frame:constants)

  ## ── Server request handler ─────────────────────────────────────────────

  (defn handle-server-request [sess s sid hdrs end? handler]
    "Handle one HTTP/2 server request on stream s."
    (def @body-parts @[])
    (unless end?
      (forever
        (let [msg (s:data-queue:take)]
          (cond
            (= msg:type :data)
              (begin
                (push body-parts msg:data)
                (when msg:end-stream (break nil)))
            (= msg:type :error) (error msg:error)
            true (break nil)))))
    (let* [method-pair (first (filter (fn [h] (= (get h 0) ":method")) hdrs))
           path-pair (first (filter (fn [h] (= (get h 0) ":path")) hdrs))
           req-headers @{}
           _ (each h in hdrs
               (let [name (get h 0)]
                 (unless (string/starts-with? name ":")
                   (put req-headers (keyword name) (get h 1)))))
           body-val (if (empty? body-parts)
                      nil
                      (apply concat (freeze body-parts)))
           request {:method (if method-pair (get method-pair 1) "GET")
                    :path (if path-pair (get path-pair 1) "/")
                    :headers (freeze req-headers)
                    :body body-val}
           [ok? response] (protect (handler request))]
      (if ok?
        (let* [status (string (or response:status 200))
               resp-headers (or response:headers {})
               trailers response:trailers
               resp-body (if (nil? response:body)
                           (bytes)
                           (if (string? response:body)
                             (bytes response:body)
                             response:body))
               @h-pairs @[[":status" status]]
               _ (each k in (keys resp-headers)
                   (push h-pairs [(string k) (string (get resp-headers k))]))
               has-body (> (length resp-body) 0)
               has-trailers (and trailers (not (empty? trailers)))
               end-on-headers (and (not has-body) (not has-trailers))]
          (session:encode-and-send-headers sess sid (freeze h-pairs)
          end-on-headers)
          (when has-body
            (session:send-data-with-flow-control sess sid s:flow resp-body
            :end-stream (not has-trailers)))
          (when has-trailers
            (session:encode-and-send-headers sess sid trailers true))
          (stream:transition s :send-end-stream))
        (begin
          (let [h-pairs [[":status" "500"]]
                err response]
            (session:encode-and-send-headers sess sid h-pairs true)
            (stream:transition s :send-end-stream)
            (error err))))))

  ## ── Server on-headers callback ─────────────────────────────────────────

  (defn make-on-headers [handler on-error]
    "Create the on-headers callback for server reader loop."
    (fn [sess s sid hdrs end?]
      (put s :headers hdrs)
      (when end? (stream:transition s :recv-end-stream))  # Check max-concurrent-streams
      (let [max-streams (get sess:local-settings :max-concurrent-streams)
            active (length (keys sess:streams))]
        (if (> active max-streams)
          (begin
            (del sess:streams sid)
            (session:send-rst-stream sess sid C:err-refused-stream))
          (ev/spawn (fn []
                      (defer
                        (del sess:streams sid)
                        (let [[ok? err] (protect (handle-server-request sess s
                              sid hdrs end? handler))]
                          (unless ok?
                            (protect (session:send-rst-stream sess sid
                                     C:err-internal-error))
                            (when on-error (on-error err)))))))))))

  ## ── Server connection ──────────────────────────────────────────────────

  (defn
    server-connection
    [transport handler sess &named on-error make-on-headers-fn]
    "Handle one HTTP/2 server connection."
    (let [mk-on-headers (or make-on-headers-fn make-on-headers)]
      (let [preface (frame:read-exact transport 24)]
        (when (or (nil? preface) (not (= preface C:client-preface)))
          (error {:error :h2-error
                  :reason :protocol-error
                  :message "invalid client connection preface"})))  # Read client SETTINGS
      (let [f (frame:read-frame transport
                                (get sess:local-settings :max-frame-size))]
        (when (or (nil? f) (not (= f:type C:type-settings)))
          (error {:error :h2-error
                  :reason :protocol-error
                  :message "expected SETTINGS as first client frame"}))
        (session:apply-remote-settings sess f:payload))  # Send our SETTINGS + ACK + connection WINDOW_UPDATE
      (let [[ftype flags sid payload] (frame:make-settings-frame session:default-settings)]
        (frame:write-frame transport ftype flags sid payload))
      (let [[ftype flags sid payload] (frame:make-settings-ack)]
        (frame:write-frame transport ftype flags sid payload))
      (let [delta (- session:initial-window 65535)]
        (when (> delta 0)
          (let [[ftype flags sid payload] (frame:make-window-update-frame 0
                delta)]
            (frame:write-frame transport ftype flags sid payload))))
      (transport:flush)  # Start writer fiber
      (put sess :writer-fiber (ev/spawn (fn [] (session:writer-loop sess))))  # Shared reader loop with server callbacks
      (session:read-loop sess :on-headers (mk-on-headers handler on-error)
                         :on-goaway (fn [sess payload]
                                      (sess:write-queue:put :shutdown)
                                      true))  # Wait for writer to drain queued frames before returning
      (when sess:writer-fiber (ev/join-protected sess:writer-fiber))))

  ## ── h2-serve ───────────────────────────────────────────────────────────

  (defn h2-serve [listener handler &named tls-config on-error]
    "Serve HTTP/2 connections. Runs forever."
    (forever
      (let* [tcp-port (tcp/accept listener)
             t (if tls-config
                 (begin
                   (when (nil? tls)
                     (error {:error :h2-error
                             :reason :tls-not-configured
                             :message "TLS serving requires :tls plugin"}))
                   (transport:tls (tls:accept listener tls-config)))
                 (transport:tcp tcp-port))
             sess (session:make-session t "" true)]
        (ev/spawn (fn []
                    (let [[ok? err] (protect (server-connection t handler sess
                          :on-error on-error))]
                      (unless ok? (when on-error (on-error err)))
                      (protect (t:close))))))))

  ## ── Streaming server ───────────────────────────────────────────────────
  ##
  ## The streaming handler receives (req ctrl) where:
  ##   req  = {:method :path :headers}  — no :body (use ctrl:recv)
  ##   ctrl = {:recv          (fn [] -> bytes|nil)
  ##           :send-headers  (fn [status headers-map] -> nil)
  ##           :send-data     (fn [data] -> nil)
  ##           :end-stream    (fn [] -> nil)
  ##           :send-trailers (fn [trailer-pairs] -> nil)}

  (defn make-stream-ctrl [sess s sid]
    "Create a stream controller for the streaming handler."
    (let [@headers-sent false
          @ended false
          @recv-done false]
      {:recv (fn []
               "Read next DATA bytes from client, or nil on end-stream."
               (when recv-done nil)
               (unless recv-done
                 (let [msg (s:data-queue:take)]
                   (cond
                     (nil? msg) (begin
                                  (assign recv-done true)
                                  nil)
                     (= msg:type :data)
                       (if msg:end-stream
                         (begin
                           (assign recv-done true)  # Return the final chunk if non-empty, else nil
                           (if (> (length msg:data) 0) msg:data nil))
                         msg:data)
                     (= msg:type :end) (begin
                                         (assign recv-done true)
                                         nil)
                     (= msg:type :error) (begin
                       (assign recv-done true)
                       (error msg:error))
                     true (begin
                            (assign recv-done true)
                            nil)))))

       :send-headers (fn [status &named headers]
                       "Send response HEADERS. Must be called before send-data."
                       (when headers-sent
                         (error {:error :h2-error
                                 :reason :protocol-error
                                 :message "response headers already sent"}))
                       (let [@h-pairs @[[":status" (string status)]]]
                         (when headers
                           (each k in (keys headers)
                             (push h-pairs [(string k) (string (get headers k))])))
                         (session:encode-and-send-headers sess sid
                         (freeze h-pairs) false)
                         (assign headers-sent true)))

       :send-data (fn [data]
                    "Send one DATA frame with flow control, no END_STREAM."
                    (unless headers-sent
                      (error {:error :h2-error
                              :reason :protocol-error
                              :message "must send headers before data"}))
                    (when ended
                      (error {:error :h2-error
                              :reason :protocol-error
                              :message "stream already ended"}))
                    (let [body (if (string? data) (bytes data) data)]
                      (session:send-data-with-flow-control sess sid s:flow body
                      :end-stream false)))

       :end-stream (fn []
                     "Send empty DATA with END_STREAM (plain close)."
                     (unless headers-sent
                       (error {:error :h2-error
                               :reason :protocol-error
                               :message "must send headers before end-stream"}))
                     (when ended
                       (error {:error :h2-error
                               :reason :protocol-error
                               :message "stream already ended"}))
                     (let [[ft fl si pl] (frame:make-data-frame sid (bytes) true)]
                       (session:send-frame sess ft fl si pl))
                     (stream:transition s :send-end-stream)
                     (assign ended true))

       :send-trailers (fn [trailer-pairs]
                        "Send trailing HEADERS with END_STREAM."
                        (unless headers-sent
                          (error {:error :h2-error
                                  :reason :protocol-error
                                  :message "must send headers before trailers"}))
                        (when ended
                          (error {:error :h2-error
                                  :reason :protocol-error
                                  :message "stream already ended"}))
                        (session:encode-and-send-headers sess sid trailer-pairs
                        true)
                        (stream:transition s :send-end-stream)
                        (assign ended true))}))

  (defn handle-streaming-request [sess s sid hdrs end? handler]
    "Handle one streaming server request."  # If client sent HEADERS+END_STREAM (no body), enqueue a sentinel
    # so ctrl:recv returns nil immediately instead of blocking
    (when end? (s:data-queue:put {:type :end :end-stream true}))
    (let* [method-pair (first (filter (fn [h] (= (get h 0) ":method")) hdrs))
           path-pair (first (filter (fn [h] (= (get h 0) ":path")) hdrs))
           req-headers @{}
           _ (each h in hdrs
               (let [name (get h 0)]
                 (unless (string/starts-with? name ":")
                   (put req-headers (keyword name) (get h 1)))))
           request {:method (if method-pair (get method-pair 1) "GET")
                    :path (if path-pair (get path-pair 1) "/")
                    :headers (freeze req-headers)}
           ctrl (make-stream-ctrl sess s sid)
           [ok? err] (protect (handler request ctrl))]
      (unless ok?
        (protect (session:send-rst-stream sess sid C:err-internal-error))
        (error err))))

  (defn make-streaming-on-headers [handler on-error]
    (fn [sess s sid hdrs end?]
      (put s :headers hdrs)
      (when end? (stream:transition s :recv-end-stream))
      (let [max-streams (get sess:local-settings :max-concurrent-streams)
            active (length (keys sess:streams))]
        (if (> active max-streams)
          (begin
            (del sess:streams sid)
            (session:send-rst-stream sess sid C:err-refused-stream))
          (ev/spawn (fn []
                      (defer
                        (del sess:streams sid)
                        (let [[ok? err] (protect (handle-streaming-request sess
                              s sid hdrs end? handler))]
                          (unless ok?
                            (protect (session:send-rst-stream sess sid
                                     C:err-internal-error))
                            (when on-error (on-error err)))))))))))

  (defn h2-serve-streaming [listener handler &named tls-config on-error]
    "Serve HTTP/2 connections with streaming handler. Runs forever."
    (forever
      (let* [tcp-port (tcp/accept listener)
             t (if tls-config
                 (begin
                   (when (nil? tls)
                     (error {:error :h2-error
                             :reason :tls-not-configured
                             :message "TLS serving requires :tls plugin"}))
                   (transport:tls (tls:accept listener tls-config)))
                 (transport:tcp tcp-port))
             sess (session:make-session t "" true)]
        (ev/spawn (fn []
                    (let [[ok? err] (protect (server-connection t handler sess
                          :on-error on-error
                          :make-on-headers-fn make-streaming-on-headers))]
                      (unless ok? (when on-error (on-error err)))
                      (protect (t:close))))))))

  ## ── Tests ──────────────────────────────────────────────────────────────

  (defn run-tests []
    true)

  ## ── Exports ────────────────────────────────────────────────────────────

  {:serve h2-serve :serve-streaming h2-serve-streaming :test run-tests})
