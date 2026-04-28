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
             (begin (push body-parts msg:data)
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
           request {:method  (if method-pair (get method-pair 1) "GET")
                    :path    (if path-pair (get path-pair 1) "/")
                    :headers (freeze req-headers)
                    :body    body-val}
           [ok? response] (protect (handler request))]
      (if ok?
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
          (session:encode-and-send-headers sess sid (freeze h-pairs) (not has-body))
          (when has-body
            (session:send-data-with-flow-control sess sid s:flow resp-body))
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
      (when end? (stream:transition s :recv-end-stream))
      # Check max-concurrent-streams
      (let [max-streams (get sess:local-settings :max-concurrent-streams)
            active (length (keys sess:streams))]
        (if (> active max-streams)
          (begin
            (del sess:streams sid)
            (session:send-rst-stream sess sid C:err-refused-stream))
          (ev/spawn
            (fn []
              (defer (del sess:streams sid)
                (let [[ok? err] (protect
                  (handle-server-request sess s sid hdrs end? handler))]
                  (unless ok?
                    (protect
                      (session:send-rst-stream sess sid C:err-internal-error))
                    (when on-error (on-error err)))))))))))

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
    # Send our SETTINGS + ACK + connection WINDOW_UPDATE
    (let [[ftype flags sid payload] (frame:make-settings-frame session:default-settings)]
      (frame:write-frame transport ftype flags sid payload))
    (let [[ftype flags sid payload] (frame:make-settings-ack)]
      (frame:write-frame transport ftype flags sid payload))
    (let [delta (- session:initial-window 65535)]
      (when (> delta 0)
        (let [[ftype flags sid payload] (frame:make-window-update-frame 0 delta)]
          (frame:write-frame transport ftype flags sid payload))))
    (transport:flush)
    # Start writer fiber
    (put sess :writer-fiber (ev/spawn (fn [] (session:writer-loop sess))))
    # Shared reader loop with server callbacks
    (session:read-loop sess
      :on-headers (make-on-headers handler on-error)
      :on-goaway (fn [sess payload]
                   (sess:write-queue:put :shutdown)
                   true)))

  ## ── h2-serve ───────────────────────────────────────────────────────────

  (defn h2-serve [listener handler &named tls-config on-error]
    "Serve HTTP/2 connections. Runs forever."
    (forever
      (let* [tcp-port (tcp/accept listener)
             t (if tls-config
                 (begin
                   (when (nil? tls)
                     (error {:error :h2-error :reason :tls-not-configured
                             :message "TLS serving requires :tls plugin"}))
                   (transport:tls (tls:accept listener tls-config)))
                 (transport:tcp tcp-port))
             sess (session:make-session t "" true)]
        (ev/spawn
          (fn []
            (let [[ok? err] (protect
              (server-connection t handler sess :on-error on-error))]
              (unless ok?
                (when on-error (on-error err)))
              (protect (t:close))))))))

  ## ── Tests ──────────────────────────────────────────────────────────────

  (defn run-tests []
    true)

  ## ── Exports ────────────────────────────────────────────────────────────

  {:serve h2-serve
   :test  run-tests})
