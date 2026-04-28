(elle/epoch 9)
## lib/http2.lisp — HTTP/2 client and server for Elle
##
## Plain h2c (cleartext):
##   (def http2 ((import "std/http2")))
##
## h2 over TLS (requires TLS plugin):
##   (def tls-plug (import "plugin/tls"))
##   (def tls ((import "std/tls") tls-plug))
##   (def http2 ((import "std/http2") :tls tls))
##
## Usage:
##   (http2:get "https://example.com/")
##   (def sess (http2:connect "https://example.com"))
##   (http2:send sess "GET" "/" :headers [])
##   (http2:close sess)

(fn [&named tls]

  ## ── Import submodules ──────────────────────────────────────────────────

  (def sync    ((import "std/sync")))
  (def b64     ((import "std/base64")))
  (def huffman ((import "std/http2/huffman")))
  (def hpack   ((import "std/http2/hpack") :huffman huffman))
  (def frame   ((import "std/http2/frame")))
  (def stream  ((import "std/http2/stream") :sync sync :frame frame))
  (def session ((import "std/http2/session") :sync sync :frame frame
                                             :stream stream :hpack hpack))
  (def server  ((import "std/http2/server") :sync sync :hpack hpack
                                            :frame frame :stream stream
                                            :session session :tls tls))

  ## ── Convenience aliases ────────────────────────────────────────────────

  (def C frame:constants)
  (def has-flag? frame:has-flag?)

  ## ── Transport abstraction ──────────────────────────────────────────────

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

  ## ── URL parsing ────────────────────────────────────────────────────────

  (defn parse-url [url]
    "Parse an HTTP/2 URL. Supports http:// and https://."
    (let* [is-https (string/starts-with? url "https://")
           is-http  (string/starts-with? url "http://")
           _ (when (not (or is-https is-http))
               (error {:error :h2-error :reason :unsupported-scheme :url url
                       :message "URL must start with http:// or https://"}))
           prefix-len (if is-https 8 7)
           scheme (if is-https "https" "http")
           default-port (if is-https 443 80)
           tail (slice url prefix-len)
           slash (string/find tail "/")
           auth (if (nil? slash) tail (slice tail 0 slash))
           path+query (if (nil? slash) "/" (slice tail slash))
           colon (string/find auth ":")
           host (if (nil? colon) auth (slice auth 0 colon))
           port (if (nil? colon) default-port (parse-int (slice auth (inc colon))))]
      (when (empty? host)
        (error {:error :h2-error :reason :empty-host :url url :message "empty host"}))
      (let* [q-pos (string/find path+query "?")
             path (if (nil? q-pos) path+query (slice path+query 0 q-pos))
             query (if (nil? q-pos) nil (slice path+query (inc q-pos)))]
        {:scheme scheme :host host :port port :path path :query query})))

  ## ── Hostname resolution ────────────────────────────────────────────────

  (defn resolve-host [host]
    (first (sys/resolve host)))

  ## ── Connection open ────────────────────────────────────────────────────

  (defn open-transport [url-parsed]
    (let [ip (resolve-host url-parsed:host)]
      (if (= url-parsed:scheme "https")
        (begin
          (when (nil? tls)
            (error {:error :h2-error :reason :tls-not-configured
                    :message "https requires :tls plugin passed to (import \"std/http2\")"}))
          (let [conn (tls:connect url-parsed:host url-parsed:port
                                  {:alpn ["h2" "http/1.1"]})]
            {:transport (tls-transport conn) :tls-conn conn}))
        {:transport (tcp-transport (tcp/connect ip url-parsed:port)) :tls-conn nil})))

  ## ── Client reader loop ─────────────────────────────────────────────────

  (defn reader-loop [sess]
    "Read frames from transport and dispatch to stream queues."
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
                sid   f:stream-id
                payload f:payload]
            (cond
              ## ── SETTINGS ──
              (= ftype C:type-settings)
               (if (has-flag? flags C:flag-ack)
                 nil
                 (begin
                   (session:apply-remote-settings sess payload)
                   (session:send-settings-ack sess)))

              ## ── PING ──
              (= ftype C:type-ping)
               (unless (has-flag? flags C:flag-ack)
                 (let [[ftype flags sid payload]
                       (frame:make-ping-frame payload :ack? true)]
                   (session:send-frame sess ftype flags sid payload)))

              ## ── GOAWAY ──
              (= ftype C:type-goaway)
               (begin
                 (put sess :goaway-recvd? true)
                 (put sess :last-stream-id
                      (bit/and (frame:read-u32 payload 0) 0x7fffffff)))

              ## ── WINDOW_UPDATE ──
              (= ftype C:type-window-update)
               (let [increment (bit/and (frame:read-u32 payload 0) 0x7fffffff)]
                 (if (= sid 0)
                   (stream:apply-window-update sess:conn-flow increment)
                   (when-let [s (get sess:streams sid)]
                     (stream:apply-window-update s:flow increment))))

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
               (let [s (session:get-stream sess sid)]
                 (when (= s:state :idle)
                   (stream:transition s :recv-headers))
                 (if (has-flag? flags C:flag-end-headers)
                   (let [headers (hpack:decode sess:hpack-decoder payload)
                         end? (has-flag? flags C:flag-end-stream)]
                     (put s :headers headers)
                     (when end? (stream:transition s :recv-end-stream))
                     (s:data-queue:put {:type :headers
                                        :headers headers
                                        :end-stream end?})
                     (when end? (del sess:streams sid)))
                   # No END_HEADERS — buffer payload + remember END_STREAM
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
                       (put s :headers headers)
                       (s:data-queue:put {:type :headers
                                          :headers headers
                                          :end-stream end?})
                       (when end? (del sess:streams sid))))))

              ## ── DATA ──
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
                         (session:send-window-update sess sid len)))
                     (when end? (del sess:streams sid)))))

              ## ── PUSH_PROMISE — reject ──
              (= ftype C:type-push-promise)
               (session:send-rst-stream sess sid C:err-refused-stream)

              ## ── Unknown — ignore ──
              true nil))))))

  ## ── Client: handshake ──────────────────────────────────────────────────

  (defn client-handshake [sess]
    "Send client connection preface and initial SETTINGS."
    (sess:transport:write C:client-preface)
    (let [[ftype flags sid payload] (frame:make-settings-frame session:default-settings)]
      (frame:write-frame sess:transport ftype flags sid payload))
    (let [delta (- session:initial-window 65535)]
      (when (> delta 0)
        (let [[ftype flags sid payload] (frame:make-window-update-frame 0 delta)]
          (frame:write-frame sess:transport ftype flags sid payload))))
    (sess:transport:flush)
    (let [f (frame:read-frame sess:transport
              (get sess:local-settings :max-frame-size))]
      (when (or (nil? f) (not (= f:type C:type-settings)))
        (error {:error :h2-error :reason :protocol-error
                :message "expected SETTINGS as first server frame"}))
      (session:apply-remote-settings sess f:payload)
      (let [[ftype flags sid payload] (frame:make-settings-ack)]
        (frame:write-frame sess:transport ftype flags sid payload))
      (sess:transport:flush)))

  ## ── Client: connect ────────────────────────────────────────────────────

  (defn h2-connect [url &named transport host]
    "Open an HTTP/2 session."
    (let [sess
          (if transport
            (session:make-session transport (or host "localhost") false)
            (let* [parsed (parse-url url)
                   {:transport t :tls-conn tc} (open-transport parsed)]
              (session:make-session t parsed:host false :scheme parsed:scheme)))]
      (client-handshake sess)
      (put sess :writer-fiber
           (ev/spawn (fn [] (session:writer-loop sess))))
      (put sess :reader-fiber
           (ev/spawn (fn [] (reader-loop sess))))
      sess))

  ## ── Client: send request ───────────────────────────────────────────────

  (defn send-request-frames [sess method path &named body headers]
    "Send HEADERS + DATA frames for a request. Returns [stream-id stream].
     Shared by h2-send and h2-send-raw (defect 10)."
    (when sess:closed?
      (error {:error :h2-error :reason :connection-closed
              :message "session is closed"}))
    (let* [sid sess:next-stream-id
           _ (put sess :next-stream-id (+ sid 2))
           s (session:get-stream sess sid)
           authority sess:host
           pseudo [[":method" method]
                   [":path" path]
                   [":scheme" (or sess:scheme "http")]
                   [":authority" authority]]
           all-headers (if (nil? headers) pseudo (concat pseudo headers))
           has-body (and body (> (length body) 0))]
      (stream:transition s :send-headers)
      (session:send-headers-with-continuation sess sid all-headers (not has-body))
      (when has-body
        (let [body-bytes (if (string? body) (bytes body) body)]
          (session:send-data-with-flow-control sess sid s:flow body-bytes))
        (stream:transition s :send-end-stream))
      [sid s]))

  (defn h2-send [sess method path &named body headers]
    "Send an HTTP/2 request. Returns response struct."
    (let* [[sid s] (send-request-frames sess method path :body body :headers headers)
           @resp-headers nil
           @resp-body @[]
           @done false]
      (while (not done)
        (let [msg (s:data-queue:take)]
          (match msg:type
            :headers (begin (assign resp-headers msg:headers)
                            (when msg:end-stream (assign done true)))
            :data    (begin (push resp-body msg:data)
                            (when msg:end-stream (assign done true)))
            :rst     (error {:error :h2-error :reason :stream-error
                             :stream-id sid :code msg:code
                             :message (concat "stream reset: " (string msg:code))})
            :error   (error msg:error)
            _        (assign done true))))
      (let* [status-pair (first (filter (fn [h] (= (get h 0) ":status")) resp-headers))
             status (if status-pair (parse-int (get status-pair 1)) 0)
             hdrs @{}]
        (each h in resp-headers
          (let [name (get h 0)
                value (get h 1)]
            (unless (string/starts-with? name ":")
              (put hdrs (keyword (slice name 0)) value))))
        {:status  status
         :headers (freeze hdrs)
         :body    (if (empty? resp-body)
                    (bytes)
                    (apply concat (freeze resp-body)))})))

  (defn h2-send-raw [sess method path &named body headers]
    "Send request, return stream for caller to collect response."
    (let [[sid s] (send-request-frames sess method path :body body :headers headers)]
      s))

  ## ── Client: one-shot API ───────────────────────────────────────────────

  (defn h2-request [method url &named body headers]
    "Send a one-shot HTTP/2 request. Opens and closes a session."
    (let [sess (h2-connect url)]
      (defer (h2-close sess)
        (let* [parsed (parse-url url)
               path (if (nil? parsed:query)
                      parsed:path
                      (concat parsed:path "?" parsed:query))]
          (h2-send sess method path :body body :headers headers)))))

  (defn h2-get [url &named headers]
    (h2-request "GET" url :headers headers))

  (defn h2-post [url body &named headers]
    (h2-request "POST" url :body body :headers headers))

  ## ── Client: close ──────────────────────────────────────────────────────

  (defn h2-close [sess]
    "Close an HTTP/2 session gracefully."
    (when (not sess:closed?)
      (put sess :closed? true)
      (session:send-goaway sess sess:last-stream-id C:err-no-error)
      # Shutdown writer — join it before closing transport (defect 8)
      (sess:write-queue:put :shutdown)
      (when sess:writer-fiber
        (ev/join-protected sess:writer-fiber))
      # Close transport — unblocks reader fiber
      (protect (sess:transport:close))
      (when sess:reader-fiber
        (ev/join-protected sess:reader-fiber)))
    nil)

  ## ── Tests ──────────────────────────────────────────────────────────────

  (defn run-tests []
    # ── URL parsing ──
    (let [p (parse-url "https://example.com:8443/path?q=1")]
      (assert (= p:scheme "https") "url: scheme")
      (assert (= p:host "example.com") "url: host")
      (assert (= p:port 8443) "url: port")
      (assert (= p:path "/path") "url: path")
      (assert (= p:query "q=1") "url: query"))

    (let [p (parse-url "http://localhost/")]
      (assert (= p:scheme "http") "url: http scheme")
      (assert (= p:port 80) "url: default port"))

    # ── Loopback test: client connect/send against a raw H2 server ──
    (let* [listener (tcp/listen "127.0.0.1" 0)
           listen-path (port/path listener)
           listen-port (parse-int (slice listen-path
                                        (+ 1 (string/find listen-path ":"))))
           server-fiber (ev/spawn
             (fn []
               (let* [tcp (tcp/accept listener)
                      t (tcp-transport tcp)]
                 (frame:read-exact t 24)
                 (frame:read-frame t 16384)
                 (let [[ft fl si pl] (frame:make-settings-frame session:default-settings)]
                   (frame:write-frame t ft fl si pl))
                 (let [[ft fl si pl] (frame:make-settings-ack)]
                   (frame:write-frame t ft fl si pl))
                 (t:flush)
                 (forever
                   (let [[ok? f] (protect (frame:read-frame t 16384))]
                     (when (or (not ok?) (nil? f)) (break nil))
                     (cond
                       (= f:type C:type-settings) nil
                       (= f:type C:type-window-update) nil
                       (= f:type C:type-goaway) (break nil)
                       (= f:type C:type-headers)
                        (let* [dec (hpack:make-decoder)
                               hdrs (hpack:decode dec f:payload)
                               path-h (first (filter (fn [h] (= (get h 0) ":path")) hdrs))
                               path (if path-h (get path-h 1) "/")
                               enc (hpack:make-encoder :use-huffman false)
                               resp-hdr (hpack:encode enc [[":status" "200"]])
                               [ft fl si pl] (frame:make-headers-frame
                                               f:stream-id resp-hdr false true)]
                          (frame:write-frame t ft fl si pl)
                          (let [[ft fl si pl]
                                (frame:make-data-frame f:stream-id
                                  (bytes (concat "hello " path)) true)]
                            (frame:write-frame t ft fl si pl))
                          (t:flush))
                       true nil)))
                 (protect (t:close)))))]
      (let* [url (concat "http://127.0.0.1:" (string listen-port))
             sess (h2-connect url)]
        (let [resp (h2-send sess "GET" "/test")]
          (assert (= resp:status 200) "loopback: status 200")
          (assert (= (string resp:body) "hello /test") "loopback: body"))
        (assert (= (length (keys sess:streams)) 0)
                "loopback: stream removed after response")
        (h2-close sess)))

    true)

  ## ── Exports ────────────────────────────────────────────────────────────

  {:get      h2-get
   :post     h2-post
   :request  h2-request
   :connect  h2-connect
   :send     h2-send
   :send-raw h2-send-raw
   :close    h2-close
   :serve    server:serve
   :parse-url parse-url
   :test     run-tests})
