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

  ## ── Convenience aliases ────────────────────────────────────────────────

  (def C frame:constants)
  (def has-flag? frame:has-flag?)

  ## ── Transport abstraction ──────────────────────────────────────────────
  ## Same pattern as lib/http.lisp — {:read :write :flush :close}

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

  ## ── URL parsing (reuse pattern from http.lisp) ─────────────────────────

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
    "Resolve hostname to IP."
    (first (sys/resolve host)))

  ## ── Connection open ────────────────────────────────────────────────────

  (defn open-transport [url-parsed]
    "Open a transport to the URL's host:port. Returns {:transport t :tls-conn conn-or-nil}."
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

  ## ── Default settings ───────────────────────────────────────────────────
  ## 1MB windows and 256KB frames for large gRPC responses.

  (def INITIAL-WINDOW (* 1024 1024))
  (def MAX-FRAME (* 256 1024))

  (def default-settings
    [[C:settings-initial-window-size INITIAL-WINDOW]
     [C:settings-max-frame-size      MAX-FRAME]
     [C:settings-enable-push         0]])

  ## ── Session struct ─────────────────────────────────────────────────────
  ## @{:transport :is-server? :streams :next-stream-id
  ##   :hpack-encoder :hpack-decoder :local-settings :remote-settings
  ##   :conn-flow :write-queue :reader-fiber :writer-fiber :closed? :host}

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
      ## Connection send window starts at 65535 per RFC 9113 Section 6.9.2.
      ## Only WINDOW_UPDATE frames change the connection window; SETTINGS
      ## INITIAL_WINDOW_SIZE only affects per-stream windows.
      :conn-flow      (stream:make-flow-control 65535)
      :write-queue    (sync:make-queue 256)
      :reader-fiber   nil
      :writer-fiber   nil
      :closed?        false
      :goaway-recvd?  false
      :last-stream-id 0})

  ## ── Writer fiber ───────────────────────────────────────────────────────
  ## Drains write-queue and writes frames to transport, batching flushes.

  (defn writer-loop [session]
    "Drain write-queue and batch-write frames to transport."
    (let [q session:write-queue
          t session:transport]
      (forever
        (let [item (q:take)]
          (when (= item :shutdown) (break nil))
          (let [@shutting-down false
                [ok? _] (protect
                  (begin
                    (let [[ftype flags sid payload] item]
                      (frame:write-frame t ftype flags sid payload))
                    (while (> (q:size) 0)
                      (let [next (q:take)]
                        (when (= next :shutdown)
                          (assign shutting-down true)
                          (break nil))
                        (let [[ftype flags sid payload] next]
                          (frame:write-frame t ftype flags sid payload))))
                    (t:flush)))]
            (when (or (not ok?) shutting-down) (break nil)))))))

  ## ── Send helpers ───────────────────────────────────────────────────────

  (defn send-frame [session ftype flags sid payload]
    "Enqueue a frame for the writer fiber."
    (session:write-queue:put [ftype flags sid payload]))

  (defn send-settings [session settings]
    "Send a SETTINGS frame."
    (let [[ftype flags sid payload] (frame:make-settings-frame settings)]
      (send-frame session ftype flags sid payload)))

  (defn send-settings-ack [session]
    "Send a SETTINGS ACK."
    (let [[ftype flags sid payload] (frame:make-settings-ack)]
      (send-frame session ftype flags sid payload)))

  (defn send-window-update [session stream-id increment]
    "Send a WINDOW_UPDATE frame."
    (let [[ftype flags sid payload] (frame:make-window-update-frame stream-id increment)]
      (send-frame session ftype flags sid payload)))

  (defn send-goaway [session last-stream-id error-code &named debug-data]
    "Send a GOAWAY frame."
    (let [[ftype flags sid payload]
          (frame:make-goaway-frame last-stream-id error-code :debug-data debug-data)]
      (send-frame session ftype flags sid payload)))

  (defn send-rst-stream [session stream-id error-code]
    "Send a RST_STREAM frame."
    (let [[ftype flags sid payload] (frame:make-rst-stream-frame stream-id error-code)]
      (send-frame session ftype flags sid payload)))

  ## ── Apply remote settings ──────────────────────────────────────────────

  (defn apply-remote-settings [session settings-payload]
    "Parse and apply peer's SETTINGS frame."
    (let [entries (frame:parse-settings settings-payload)]
      (each entry in entries
        (cond
          (= entry:id C:settings-initial-window-size)
           (put session:remote-settings :initial-window-size entry:value)
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

  ## ── Reader fiber ───────────────────────────────────────────────────────
  ## Reads frames, dispatches to per-stream queues, handles control frames.

  (defn notify-all-streams [session reason]
    "Push an error message into every live stream's data-queue so blocked
     consumers wake up instead of hanging forever."
    (each sid in (keys session:streams)
      (let [s (get session:streams sid)]
        (when s
          (let [[ok? _] (protect
                          (s:data-queue:put {:type :error
                                            :error {:error :h2-error
                                                    :reason reason
                                                    :message "session closed"}}))]
            nil))))
    (put session :streams @{}))

  (defn reader-loop [session]
    "Read frames from transport and dispatch."
    (let [t session:transport
          max-size (get session:local-settings :max-frame-size)]
      (forever
        (let [[ok? f] (protect (frame:read-frame t max-size))]
          (when (not ok?)
            (put session :closed? true)
            (session:write-queue:put :shutdown)
            (notify-all-streams session :transport-error)
            (break nil))
          (when (nil? f)
            (put session :closed? true)
            (session:write-queue:put :shutdown)
            (notify-all-streams session :eof)
            (break nil))
          (let [ftype f:type
                flags f:flags
                sid   f:stream-id
                payload f:payload]
            (cond
              ## ── SETTINGS ──
              (= ftype C:type-settings)
               (if (has-flag? flags C:flag-ack)
                 nil  # Settings ACK — nothing to do
                 (begin
                   (apply-remote-settings session payload)
                   (send-settings-ack session)))

              ## ── PING ──
              (= ftype C:type-ping)
               (unless (has-flag? flags C:flag-ack)
                 (let [[ftype flags sid payload]
                       (frame:make-ping-frame payload :ack? true)]
                   (send-frame session ftype flags sid payload)))

              ## ── GOAWAY ──
              (= ftype C:type-goaway) (begin (put session :goaway-recvd? true) (put session :last-stream-id
                    (bit/and (frame:read-u32 payload 0) 0x7fffffff)))

              ## ── WINDOW_UPDATE ──
              (= ftype C:type-window-update)
               (let [increment (bit/and (frame:read-u32 payload 0) 0x7fffffff)]
                 (if (= sid 0)
                   (stream:apply-window-update session:conn-flow increment)
                   (let [s (get session:streams sid)]
                     (when s
                       (put s :send-window (+ s:send-window increment))))))

              ## ── RST_STREAM ──
              (= ftype C:type-rst-stream)
               (let [s (get session:streams sid)]
                 (when s
                   (stream:transition s :recv-rst)
                   (let [err-code (frame:read-u32 payload 0)]
                     (put s :error-code err-code)
                     (s:data-queue:put {:type :rst :code err-code}))
                   (del session:streams sid)))

              ## ── HEADERS ──
              (= ftype C:type-headers)
               (let [s (get-stream session sid)]
                 (when (= s:state :idle)
                   (stream:transition s :recv-headers))
                 (let [headers (hpack:decode session:hpack-decoder payload)
                       end? (has-flag? flags C:flag-end-stream)]
                   (put s :headers headers)
                   (when end? (stream:transition s :recv-end-stream))
                   (s:data-queue:put {:type :headers
                                      :headers headers
                                      :end-stream end?})
                   (when end? (del session:streams sid))))

              ## ── DATA ──
              (= ftype C:type-data)
               (begin
                 # Always send connection-level WINDOW_UPDATE for received
                 # DATA, even if the stream is unknown (already closed).
                 # Skipping this leaks the connection receive window.
                 (let [len (length payload)]
                   (when (> len 0)
                     (send-window-update session 0 len)))
                 (let [s (get session:streams sid)]
                   (when s
                     (let [end? (has-flag? flags C:flag-end-stream)]
                       (s:data-queue:put {:type :data :data payload
                                          :end-stream end?})
                       (when end? (stream:transition s :recv-end-stream))
                       # Stream-level WINDOW_UPDATE
                       (let [len (length payload)]
                         (when (> len 0)
                           (send-window-update session sid len)))
                       (when end? (del session:streams sid))))))

              ## ── PUSH_PROMISE — reject ──
              (= ftype C:type-push-promise)
               (send-rst-stream session sid C:err-refused-stream)

              ## ── Unknown — ignore ──
              true nil))))))

  ## ── Client: connection preface + handshake ─────────────────────────────

  (defn client-handshake [session]
    "Send client connection preface and initial SETTINGS."
    # Write the magic preface directly (bypass write queue — not started yet)
    (session:transport:write C:client-preface)
    # Send our SETTINGS
    (let [[ftype flags sid payload] (frame:make-settings-frame default-settings)]
      (frame:write-frame session:transport ftype flags sid payload))
    # Connection window starts at 65535 per RFC 9113 regardless of SETTINGS.
    # Send WINDOW_UPDATE on stream 0 to bring it up to INITIAL-WINDOW.
    (let [delta (- INITIAL-WINDOW 65535)]
      (when (> delta 0)
        (let [[ftype flags sid payload] (frame:make-window-update-frame 0 delta)]
          (frame:write-frame session:transport ftype flags sid payload))))
    (session:transport:flush)
    # Read the server's SETTINGS (first frame must be SETTINGS)
    (let [f (frame:read-frame session:transport
              (get session:local-settings :max-frame-size))]
      (when (or (nil? f) (not (= f:type C:type-settings)))
        (error {:error :h2-error :reason :protocol-error
                :message "expected SETTINGS as first server frame"}))
      (apply-remote-settings session f:payload)
      # Write ACK directly — writer fiber not started yet
      (let [[ftype flags sid payload] (frame:make-settings-ack)]
        (frame:write-frame session:transport ftype flags sid payload))
      (session:transport:flush)))

  ## ── Client: connect ────────────────────────────────────────────────────

  (defn h2-connect [url &named transport host]
    "Open an HTTP/2 session. Returns a session struct.
     When :transport is provided, uses it directly (for Unix sockets etc.)
     instead of parsing URL and opening TCP. Optional :host sets authority header."
    (let [session
          (if transport
            (make-session transport (or host "localhost") false)
            (let* [parsed (parse-url url)
                   {:transport t :tls-conn tc} (open-transport parsed)]
              (make-session t parsed:host false :scheme parsed:scheme)))]
      (client-handshake session)
      (put session :writer-fiber
           (ev/spawn (fn [] (writer-loop session))))
      (put session :reader-fiber
           (ev/spawn (fn [] (reader-loop session))))
      session))

  ## ── Client: send a request on a session ────────────────────────────────

  (defn h2-send [session method path &named body headers]
    "Send an HTTP/2 request on an existing session. Returns response struct."
    (when session:closed?
      (error {:error :h2-error :reason :connection-closed
              :message "session is closed"}))
    (let* [sid session:next-stream-id
           _ (put session :next-stream-id (+ sid 2))
           s (get-stream session sid)
           authority session:host
           pseudo [[":method" method]
                   [":path" path]
                   [":scheme" (or session:scheme "http")]
                   [":authority" authority]]
           all-headers (if (nil? headers) pseudo (concat pseudo headers))
           header-block (hpack:encode session:hpack-encoder all-headers)
           has-body (and body (> (length body) 0))]
      # Send HEADERS
      (let [[ftype flags sid2 payload]
            (frame:make-headers-frame sid header-block (not has-body) true)]
        (stream:transition s :send-headers)
        (send-frame session ftype flags sid2 payload))
      # Send DATA if body present, respecting connection flow control
      (when has-body
        (let* [body-bytes (if (string? body) (bytes body) body)
               max-frame (get session:remote-settings :max-frame-size)
               @offset 0
               total (length body-bytes)]
          (while (< offset total)
            (let* [remaining (- total offset)
                   # Respect connection send window — blocks until space available
                   allowed (stream:consume-send-window session:conn-flow
                             (min remaining max-frame))
                   chunk (slice body-bytes offset (+ offset allowed))
                   end? (= (+ offset allowed) total)
                   [ftype flags sid2 payload]
                   (frame:make-data-frame sid chunk end?)]
              (send-frame session ftype flags sid2 payload)
              (assign offset (+ offset allowed))))
          (stream:transition s :send-end-stream)))
      # Wait for response
      (let [@resp-headers nil
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
        # Build response
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
                      (apply concat (freeze resp-body)))}))))

  ## ── Client: send-raw (returns stream for caller to collect) ────────────

  (defn h2-send-raw [session method path &named body headers]
    "Send request, return stream for caller to collect response.
     Unlike h2:send, does NOT wait for or collect the response."
    (when session:closed?
      (error {:error :h2-error :reason :connection-closed
              :message "session is closed"}))
    (let* [sid session:next-stream-id
           _ (put session :next-stream-id (+ sid 2))
           s (get-stream session sid)
           authority session:host
           pseudo [[":method" method]
                   [":path" path]
                   [":scheme" (or session:scheme "http")]
                   [":authority" authority]]
           all-headers (if (nil? headers) pseudo (concat pseudo headers))
           header-block (hpack:encode session:hpack-encoder all-headers)
           has-body (and body (> (length body) 0))]
      # Send HEADERS
      (let [[ftype flags sid2 payload]
            (frame:make-headers-frame sid header-block (not has-body) true)]
        (stream:transition s :send-headers)
        (send-frame session ftype flags sid2 payload))
      # Send DATA if body present, respecting connection flow control
      (when has-body
        (let* [body-bytes (if (string? body) (bytes body) body)
               max-frame (get session:remote-settings :max-frame-size)
               @offset 0
               total (length body-bytes)]
          (while (< offset total)
            (let* [remaining (- total offset)
                   allowed (stream:consume-send-window session:conn-flow
                             (min remaining max-frame))
                   chunk (slice body-bytes offset (+ offset allowed))
                   end? (= (+ offset allowed) total)
                   [ftype flags sid2 payload]
                   (frame:make-data-frame sid chunk end?)]
              (send-frame session ftype flags sid2 payload)
              (assign offset (+ offset allowed))))
          (stream:transition s :send-end-stream)))
      s))

  ## ── Unix socket transport ────────────────────────────────────────────

  (defn unix-transport [port]
    "Wrap a Unix socket port as an HTTP/2 transport."
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

  ## ── Client: one-shot API ───────────────────────────────────────────────

  (defn h2-request [method url &named body headers]
    "Send a one-shot HTTP/2 request. Opens and closes a session."
    (let [session (h2-connect url)]
      (defer (h2-close session)
        (let* [parsed (parse-url url)
               path (if (nil? parsed:query)
                      parsed:path
                      (concat parsed:path "?" parsed:query))]
          (h2-send session method path :body body :headers headers)))))

  (defn h2-get [url &named headers]
    "HTTP/2 GET request."
    (h2-request "GET" url :headers headers))

  (defn h2-post [url body &named headers]
    "HTTP/2 POST request."
    (h2-request "POST" url :body body :headers headers))

  ## ── Client: close ──────────────────────────────────────────────────────

  (defn h2-close [session]
    "Close an HTTP/2 session gracefully."
    (when (not session:closed?)
      (put session :closed? true)
      (send-goaway session session:last-stream-id C:err-no-error)
      # Shutdown writer
      (session:write-queue:put :shutdown)
      # Close transport — unblocks reader fiber's read
      (let [[ok? _] (protect (session:transport:close))]
        nil)
      # Wait for fibers to exit so the scheduler has no dangling work
      (when session:writer-fiber
        (ev/join-protected session:writer-fiber))
      (when session:reader-fiber
        (ev/join-protected session:reader-fiber)))
    nil)

  ## ── Server ─────────────────────────────────────────────────────────────

  (defn server-connection [transport handler session]
    "Handle one HTTP/2 server connection."
    # Read client preface
    (let [preface (frame:read-exact transport 24)]
      (when (or (nil? preface) (not (= preface C:client-preface)))
        (error {:error :h2-error :reason :protocol-error
                :message "invalid client connection preface"})))
    # Read client SETTINGS
    (let [f (frame:read-frame transport (get session:local-settings :max-frame-size))]
      (when (or (nil? f) (not (= f:type C:type-settings)))
        (error {:error :h2-error :reason :protocol-error
                :message "expected SETTINGS as first client frame"}))
      (apply-remote-settings session f:payload))
    # Send our SETTINGS + ACK client's — write directly before writer starts
    (let [[ftype flags sid payload] (frame:make-settings-frame default-settings)]
      (frame:write-frame transport ftype flags sid payload))
    (let [[ftype flags sid payload] (frame:make-settings-ack)]
      (frame:write-frame transport ftype flags sid payload))
    (transport:flush)
    # Start writer fiber
    (put session :writer-fiber (ev/spawn (fn [] (writer-loop session))))
    # Reader loop inline (handles requests by spawning fibers)
    (let [max-size (get session:local-settings :max-frame-size)]
      (forever
        (let [[ok? f] (protect (frame:read-frame transport max-size))]
          (when (or (not ok?) (nil? f))
            (session:write-queue:put :shutdown)
            (break nil))
          (let [ftype f:type
                flags f:flags
                sid   f:stream-id
                payload f:payload]
            (cond
              (= ftype C:type-settings)
               (if (has-flag? flags C:flag-ack)
                 nil
                 (begin
                   (apply-remote-settings session payload)
                   (send-settings-ack session)))

              (= ftype C:type-ping)
               (unless (has-flag? flags C:flag-ack)
                 (let [[ft fl si pl] (frame:make-ping-frame payload :ack? true)]
                   (send-frame session ft fl si pl)))

              (= ftype C:type-window-update)
               (let [increment (bit/and (frame:read-u32 payload 0) 0x7fffffff)]
                 (when (= sid 0)
                   (stream:apply-window-update session:conn-flow increment)))

              (= ftype C:type-goaway) (begin (session:write-queue:put :shutdown) (break nil))

              (= ftype C:type-headers)
               (let [s (get-stream session sid)]
                 (when (= s:state :idle)
                   (stream:transition s :recv-headers))
                 (let [hdrs (hpack:decode session:hpack-decoder payload)
                       end? (has-flag? flags C:flag-end-stream)]
                   (put s :headers hdrs)
                   (when end? (stream:transition s :recv-end-stream))
                   # Spawn a fiber to handle this request
                   (ev/spawn
                     (fn []
                       # Collect body if not end-stream
                       (def @body-parts @[])
                       (unless end?
                         (forever
                           (let [msg (s:data-queue:take)]
                             (cond
                               (= msg:type :data) (begin (push body-parts msg:data) (when msg:end-stream (break nil)))
                               true (break nil)))))
                       # Build request struct
                       (let* [method-pair (first (filter (fn [h] (= (get h 0) ":method")) hdrs))
                              path-pair (first (filter (fn [h] (= (get h 0) ":path")) hdrs))
                              req-headers @{}
                              _ (each h in hdrs
                                  (let [name (get h 0)]
                                    (unless (string/starts-with? name ":")
                                      (put req-headers (keyword (slice name 0)) (get h 1)))))
                              request {:method  (if method-pair (get method-pair 1) "GET")
                                       :path    (if path-pair (get path-pair 1) "/")
                                       :headers (freeze req-headers)
                                       :body    (if (empty? body-parts)
                                                  nil
                                                  (apply concat (freeze body-parts)))}
                              [ok? response] (protect (handler request))]
                         (if ok?
                           # Send response
                           (let* [status (string (or response:status 200))
                                  resp-headers (or response:headers {})
                                  resp-body (if (nil? response:body)
                                              (bytes)
                                              (if (string? response:body)
                                                (bytes response:body)
                                                response:body))
                                  h-pairs (concat [[":status" status]]
                                                  (map (fn [k]
                                                         [(string k) (string (get resp-headers k))])
                                                       (keys resp-headers)))
                                  h-block (hpack:encode session:hpack-encoder h-pairs)
                                  has-body (> (length resp-body) 0)]
                             (let [[ft fl si pl]
                                   (frame:make-headers-frame sid h-block (not has-body) true)]
                               (stream:transition s :send-headers)
                               (send-frame session ft fl si pl))
                             (when has-body
                               (let [[ft fl si pl]
                                     (frame:make-data-frame sid resp-body true)]
                                 (send-frame session ft fl si pl)))
                             (stream:transition s :send-end-stream))
                           # Handler error → 500
                           (let* [h-block (hpack:encode session:hpack-encoder
                                           [[":status" "500"]])
                                  [ft fl si pl]
                                  (frame:make-headers-frame sid h-block true true)]
                             (stream:transition s :send-headers)
                             (send-frame session ft fl si pl)
                             (stream:transition s :send-end-stream))))))))

              (= ftype C:type-data)
               (begin
                 # Always send connection WINDOW_UPDATE for received DATA
                 (let [len (length payload)]
                   (when (> len 0)
                     (send-window-update session 0 len)))
                 (let [s (get session:streams sid)]
                   (when s
                     (let [end? (has-flag? flags C:flag-end-stream)]
                       (s:data-queue:put {:type :data :data payload
                                          :end-stream end?})
                       (when end? (stream:transition s :recv-end-stream))
                       (let [len (length payload)]
                         (when (> len 0)
                           (send-window-update session sid len)))
                       (when end? (del session:streams sid))))))

              (= ftype C:type-rst-stream)
               (let [s (get session:streams sid)]
                 (when s
                   (stream:transition s :recv-rst)
                   (s:data-queue:put {:type :rst :code (frame:read-u32 payload 0)})
                   (del session:streams sid)))

              true nil))))))

  (defn h2-serve [listener handler &named tls-config on-error]
    "Serve HTTP/2 connections. Runs forever.
     listener: from (tcp/listen host port).
     handler:  (fn [request] response).
     Optional :tls-config for h2 over TLS."
    (forever
      (let* [tcp-port (tcp/accept listener)
             transport (if tls-config
                         (begin
                           (when (nil? tls)
                             (error {:error :h2-error :reason :tls-not-configured
                                     :message "TLS serving requires :tls plugin"}))
                           (tls-transport (tls:accept listener tls-config)))
                         (tcp-transport tcp-port))
             session (make-session transport "" true)]
        (ev/spawn
          (fn []
            (let [[ok? err] (protect (server-connection transport handler session))]
              (unless ok?
                (when on-error (on-error err)))
              (let [[_ _] (protect (transport:close))]
                nil)))))))

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
                 (frame:read-exact t 24)      # client preface
                 (frame:read-frame t 16384)   # client SETTINGS
                 # Send server SETTINGS + ACK
                 (let [[ft fl si pl] (frame:make-settings-frame default-settings)]
                   (frame:write-frame t ft fl si pl))
                 (let [[ft fl si pl] (frame:make-settings-ack)]
                   (frame:write-frame t ft fl si pl))
                 (t:flush)
                 # Read frames until GOAWAY or EOF
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
      # Client connects using the h2 module API
      (let* [url (concat "http://127.0.0.1:" (string listen-port))
             session (h2-connect url)]
        (let [resp (h2-send session "GET" "/test")]
          (assert (= resp:status 200) "loopback: status 200")
          (assert (= (string resp:body) "hello /test") "loopback: body"))
        # Stream leak regression: completed streams must be removed
        (assert (= (length (keys session:streams)) 0)
                "loopback: stream removed after response")
        (h2-close session)))

    true)

  ## ── Exports ────────────────────────────────────────────────────────────

  {:get      h2-get
   :post     h2-post
   :request  h2-request
   :connect  h2-connect
   :send     h2-send
   :send-raw h2-send-raw
   :close    h2-close
   :serve    h2-serve
   :parse-url parse-url
   :unix-transport unix-transport
   :test     run-tests})
