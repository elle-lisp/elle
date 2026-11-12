(elle/epoch 15)
# audited: 2026-11-12
## WebSocket client and server over RFC 6455, on ws:// and on wss:// through std/tls.
## docs/libraries.md
##
## Parameterized module:
##   (def hash (import "plugin/hash"))
##   (def rand (import "plugin/random"))
##   (def ws   ((import "std/websocket") :hash hash :random rand))
##
## With TLS (for wss://):
##   (def tls-plug (import "plugin/tls"))
##   (def tls ((import "std/tls") tls-plug))
##   (def ws  ((import "std/websocket") :hash hash :random rand :tls tls))
##
## Client:
##   (def conn (ws:connect "ws://localhost:8080/chat"))
##   (ws:send conn "hello")
##   (def msg (ws:recv conn))
##   (ws:close conn)
##
## Server:
##   (def listener (tcp/listen "127.0.0.1" 8080))
##   (ws:serve listener (fn [conn]
##     (forever
##       (let [msg (ws:recv conn)]
##         (when (= msg:type :close) (break nil))
##         (ws:send conn msg:data)))))

(fn [&named tls hash random]

  ## ── Internal imports ────────────────────────────────────────────────

  (def b64 ((import "std/base64")))
  (def framing ((import "std/framing")))

  ## ── Constants ───────────────────────────────────────────────────────

  (def WS-GUID "258EAFA5-E914-47DA-95CA-5AB9DC76B585")

  (def OP-CONTINUATION 0)
  (def OP-TEXT 1)
  (def OP-BINARY 2)
  (def OP-CLOSE 8)
  (def OP-PING 9)
  (def OP-PONG 10)

  ## ── URL parsing ─────────────────────────────────────────────────────

  (defn ws-parse-url [url]
    "Parse a WebSocket URL. Supports ws:// and wss://."
    (let* [is-wss (string/starts-with? url "wss://")
           is-ws (string/starts-with? url "ws://")
           _ (when (not (or is-wss is-ws))
               (error {:error :ws-error
                       :reason :unsupported-scheme
                       :url url
                       :message "URL must start with ws:// or wss://"}))
           prefix-len (if is-wss 6 5)
           scheme (if is-wss "wss" "ws")
           default-port (if is-wss 443 80)
           tail (slice url prefix-len)
           slash (string/find tail "/")
           auth (if (nil? slash) tail (slice tail 0 slash))
           path (if (nil? slash) "/" (slice tail slash))
           colon (string/find auth ":")
           host (if (nil? colon) auth (slice auth 0 colon))
           port (if (nil? colon)
                  default-port
                  (parse-int (slice auth (inc colon))))]
      (when (empty? host)
        (error {:error :ws-error
                :reason :empty-host
                :url url
                :message "empty host"}))
      {:scheme scheme :host host :port port :path path}))

  ## ── Transport abstraction ───────────────────────────────────────────

  (defn tcp-transport [port]
    "Wrap a TCP port as a transport. A write queues its value and a flush
     sends the queue in one gathered write, which takes the queue first, so
     another fiber's flush cannot send it again (lib/framing.md)."
    (let [g (framing:gather port)]
      {:read (fn [n] (port/read port n))
       :read-line (fn [] (port/read-line port))
       :write (fn [data] (framing:write g data))
       :flush (fn [] (framing:flush g))
       :close (fn [] (port/close port))}))

  (defn tls-transport [conn]
    "Wrap a TLS connection as a transport. A flush joins the queued values
     into one tls:write."
    (let [g (framing:gather (fn [parts] (tls:write conn (framing:join parts))))]
      {:read (fn [n] (tls:read conn n))
       :read-line (fn [] (tls:read-line conn))
       :write (fn [data] (framing:write g data))
       :flush (fn [] (framing:flush g))
       :close (fn [] (tls:close conn))}))

  (defn open-transport [parsed]
    "Open transport to parsed URL's host:port."
    (if (= parsed:scheme "wss")
      (begin
        (when (nil? tls)
          (error {:error :ws-error
                  :reason :tls-not-configured
                  :message "wss:// requires :tls plugin passed to (import \"std/websocket\")"}))
        (tls-transport (tls:connect parsed:host parsed:port {})))
      (tcp-transport (tcp/connect parsed:host parsed:port))))

  ## ── Transport helpers ───────────────────────────────────────────────

  (defn read-exact [t n]
    "Read exactly n bytes from transport, looping on short reads."
    (def @remaining n)
    (def @parts @[])
    (while (> remaining 0)
      (let [chunk (t:read remaining)]
        (when (nil? chunk)
          (error {:error :ws-error
                  :reason :unexpected-eof
                  :message "unexpected EOF reading from transport"}))
        (let [b (if (bytes? chunk) chunk (bytes chunk))]
          (push parts b)
          (assign remaining (- remaining (length b))))))
    (if (= (length parts) 1)
      (first (freeze parts))
      (apply concat (freeze parts))))

  ## ── Handshake helpers ───────────────────────────────────────────────

  (defn compute-accept-key [client-key]
    "Compute Sec-WebSocket-Accept from client key (RFC 6455 section 4.2.2)."
    (b64:encode (hash:sha1 (bytes (string client-key WS-GUID)))))

  (defn generate-key []
    "Generate a random 16-byte Sec-WebSocket-Key, base64-encoded."
    (b64:encode (random:csprng-bytes 16)))

  ## ── Frame codec ─────────────────────────────────────────────────────

  (defn apply-mask [data mask-key]
    "XOR each byte of data with mask-key[i % 4]."
    (let [result (thaw data)
          len (length data)]
      (def @i 0)
      (while (< i len)
        (put result i (bit/xor (get data i) (get mask-key (rem i 4))))
        (assign i (inc i)))
      (freeze result)))

  (defn frame-parts [opcode payload &named mask? fin]
    "A WebSocket frame as the parts a send writes: the header and the
     payload as given, a string or bytes. mask? true for client frames, whose
     parts are the header, the mask key and the payload masked with it."
    (let* [do-fin (if (nil? fin) true fin)
           do-mask (if (nil? mask?) false mask?)
           plen (if (string? payload) (string/size-of payload) (length payload))
           byte0 (bit/or (if do-fin 0x80 0) (bit/and opcode 0x0F))
           mask-bit (if do-mask 0x80 0)]
      (def @header @b[])
      (push header byte0)
      (cond
        (< plen 126) (push header (bit/or mask-bit plen))
        (< plen 65536)
          (begin
            (push header (bit/or mask-bit 126))
            (push header (bit/and (bit/shr plen 8) 0xFF))
            (push header (bit/and plen 0xFF)))
        true
          (begin
            (push header (bit/or mask-bit 127))
            (push header (bit/and (bit/shr plen 56) 0xFF))
            (push header (bit/and (bit/shr plen 48) 0xFF))
            (push header (bit/and (bit/shr plen 40) 0xFF))
            (push header (bit/and (bit/shr plen 32) 0xFF))
            (push header (bit/and (bit/shr plen 24) 0xFF))
            (push header (bit/and (bit/shr plen 16) 0xFF))
            (push header (bit/and (bit/shr plen 8) 0xFF))
            (push header (bit/and plen 0xFF))))
      (if do-mask
        (let [mask-key (random:csprng-bytes 4)]
          [(freeze header) mask-key
           (apply-mask (if (bytes? payload) payload (bytes payload)) mask-key)])
        [(freeze header) payload])))

  (defn send-frame [t opcode payload is-client?]
    "Write one frame's parts on transport t, and flush them as one write."
    (each part in (frame-parts opcode payload :mask? is-client?)
      (t:write part))
    (t:flush))

  (defn decode-frame [t]
    "Read and decode a WebSocket frame from transport.
     Returns {:fin :opcode :payload} or nil on EOF."
    (let [hdr (t:read 2)]
      (if (or (nil? hdr) (< (length hdr) 2))
        nil
        (let* [byte0 (get hdr 0)
               byte1 (get hdr 1)
               fin (= (bit/and byte0 0x80) 0x80)
               opcode (bit/and byte0 0x0F)
               masked (= (bit/and byte1 0x80) 0x80)
               plen (bit/and byte1 0x7F)
               payload-len (cond
                             (= plen 126)
                               (let [ext (read-exact t 2)]
                                 (bit/or (bit/shl (get ext 0) 8) (get ext 1)))
                             (= plen 127)
                               (let [ext (read-exact t 8)]
                                 (bit/or (bit/shl (get ext 0) 56)
                                   (bit/shl (get ext 1) 48)
                                   (bit/shl (get ext 2) 40)
                                   (bit/shl (get ext 3) 32)
                                   (bit/shl (get ext 4) 24)
                                   (bit/shl (get ext 5) 16)
                                   (bit/shl (get ext 6) 8) (get ext 7)))
                             true plen)
               mask-key (when masked (read-exact t 4))
               raw (if (> payload-len 0) (read-exact t payload-len) (bytes))
               payload (if masked (apply-mask raw mask-key) raw)]
          {:fin fin :opcode opcode :payload payload}))))

  ## ── Message reassembly ──────────────────────────────────────────────

  (defn recv-message [conn]
    "Read frames until a complete message is assembled.
     Auto-pongs on ping. Returns {:type :text/:binary/:close :data payload}."
    (let [t conn:transport
          @parts @[]
          @msg-opcode nil
          @result nil]
      (forever
        (let [frame (decode-frame t)]
          (if (nil? frame)
            (begin
              (assign
                result
                {:type :close :data (bytes) :code 1006 :reason "EOF"})
              (break nil))
            (let [op frame:opcode]
              (cond
                (= op OP-PING)
                  (send-frame t OP-PONG frame:payload conn:is-client?)
                (= op OP-PONG) nil
                (= op OP-CLOSE)
                  (let* [payload frame:payload
                         code (if (>= (length payload) 2)
                                (bit/or (bit/shl (get payload 0) 8)
                                        (get payload 1))
                                1005)
                         reason (if (> (length payload) 2)
                                  (string (slice payload 2))
                                  "")]
                    (assign
                      result
                      {:type :close :data payload :code code :reason reason})
                    (break nil))
                true
                  (begin
                    (when (not (= op OP-CONTINUATION)) (assign msg-opcode op))
                    (push parts frame:payload)
                    (when frame:fin
                      (let* [data (if (= (length parts) 1)
                                    (first (freeze parts))
                                    (apply concat (freeze parts)))
                             type (if (= msg-opcode OP-TEXT) :text :binary)]
                        (assign result {:type type :data data})
                        (break nil)))))))))
      result))

  ## ── Connection struct ───────────────────────────────────────────────

  (defn make-conn [transport is-client?]
    {:transport transport :is-client? is-client?})

  ## ── Client API ──────────────────────────────────────────────────────

  (defn ws-connect [url &named headers]
    "Connect to a WebSocket server. Returns a connection struct."
    (let* [parsed (ws-parse-url url)
           t (open-transport parsed)
           key (generate-key)
           host-str (if (or (and (= parsed:scheme "ws") (= parsed:port 80))
                            (and (= parsed:scheme "wss") (= parsed:port 443)))
                      parsed:host
                      (string parsed:host ":" parsed:port))
           req (string "GET " parsed:path " HTTP/1.1\r\n" "Host: " host-str
                       "\r\n" "Upgrade: websocket\r\n" "Connection: Upgrade\r\n"
                       "Sec-WebSocket-Key: " key "\r\n"
                       "Sec-WebSocket-Version: 13\r\n")]
      (def @req-str req)
      (when headers
        (each [name value] in headers
          (assign req-str (string req-str name ": " value "\r\n"))))
      (assign req-str (string req-str "\r\n"))
      (t:write req-str)
      (t:flush)
      (let [status-line (t:read-line)]
        (when (or (nil? status-line) (not (string/contains? status-line "101")))
          (error {:error :ws-error
                  :reason :handshake-failed
                  :message (string "expected 101 Switching Protocols, got: "
                                   status-line)}))
        (def @accept-key nil)
        (forever
          (let [line (t:read-line)]
            (when (or (nil? line) (empty? line) (= line "\r")) (break nil))
            (let [colon (string/find line ":")]
              (when colon
                (let [name (string/lowercase (string/trim (slice line 0 colon)))
                      value (string/trim (slice line (+ colon 1)))]
                  (when (= name "sec-websocket-accept")
                    (assign accept-key value)))))))
        (let [expected (compute-accept-key key)]
          (when (not (= accept-key expected))
            (t:close)
            (error {:error :ws-error
                    :reason :invalid-accept-key
                    :expected expected
                    :got accept-key
                    :message "server Sec-WebSocket-Accept does not match"}))
          (make-conn t true)))))

  (defn ws-send [conn data]
    "Send a text or binary message over a WebSocket connection. Two fibers
     that send on one connection each deliver their own message once."
    (send-frame conn:transport (if (string? data) OP-TEXT OP-BINARY) data
                conn:is-client?))

  (defn ws-recv [conn]
    "Receive a complete WebSocket message. Returns {:type :data}."
    (recv-message conn))

  (defn ws-ping [conn]
    "Send a ping frame."
    (send-frame conn:transport OP-PING (bytes) conn:is-client?))

  (defn ws-close [conn &named code reason]
    "Close the WebSocket connection gracefully."
    (let* [close-code (or code 1000)
           close-reason (or reason "")
           payload (concat (bytes (bit/and (bit/shr close-code 8) 0xFF)
                                  (bit/and close-code 0xFF))
                           (bytes close-reason))]
      (send-frame conn:transport OP-CLOSE payload conn:is-client?)
      (let [[ok? _] (protect (recv-message conn))]
        nil)
      (let [[ok? _] (protect (conn:transport:close))]
        nil)))

  ## ── Server API ──────────────────────────────────────────────────────

  (defn ws-upgrade [req t]
    "Upgrade an HTTP request to WebSocket. Returns a connection struct."
    (let* [headers req:headers
           key (get headers :sec-websocket-key)]
      (when (nil? key)
        (error {:error :ws-error
                :reason :missing-key
                :message "missing Sec-WebSocket-Key header"}))
      (let* [accept (compute-accept-key key)
             response (string "HTTP/1.1 101 Switching Protocols\r\n"
                              "Upgrade: websocket\r\n" "Connection: Upgrade\r\n"
                              "Sec-WebSocket-Accept: " accept "\r\n" "\r\n")]
        (t:write response)
        (t:flush)
        (make-conn t false))))

  (defn ws-serve [listener handler]
    "Accept WebSocket connections and handle them."
    (forever
      (let* [[ok? tcp-port] (protect (tcp/accept listener))]
        (unless ok? (break nil))
        (ev/spawn (fn []
                    (let [t (tcp-transport tcp-port)]
                      (defer
                        (protect (t:close))
                        (let [req-line (t:read-line)]
                          (when (not (nil? req-line))
                            (let [parts (string/split req-line " ")]
                              (when (>= (length parts) 2)
                                (def @headers @{})
                                (forever
                                  (let [line (t:read-line)]
                                    (when (or (nil? line) (empty? line)
                                            (= line "\r"))
                                      (break nil))
                                    (let [colon (string/find line ":")]
                                      (when colon
                                        (let [hdr-name (slice line 0 colon)
                                              name (keyword
                                                     (string/lowercase
                                                       (string/trim hdr-name)))
                                              value (string/trim
                                                      (slice line (+ colon 1)))]
                                          (put headers name value))))))
                                (let* [req {:method (get parts 0)
                                            :path (get parts 1)
                                            :headers (freeze headers)
                                            :body nil}
                                       conn (ws-upgrade req t)]
                                  (let [[ok? _] (protect (handler conn))]
                                    nil)))))))))))))

  ## ── Internal tests ──────────────────────────────────────────────────

  (defn run-tests []

    ## ── URL parsing ──
    (let [p (ws-parse-url "ws://example.com/chat")]
      (assert (= p:scheme "ws") "ws: scheme")
      (assert (= p:host "example.com") "ws: host")
      (assert (= p:port 80) "ws: default port")
      (assert (= p:path "/chat") "ws: path"))

    (let [p (ws-parse-url "wss://secure.io:9443/ws")]
      (assert (= p:scheme "wss") "wss: scheme")
      (assert (= p:host "secure.io") "wss: host")
      (assert (= p:port 9443) "wss: port")
      (assert (= p:path "/ws") "wss: path"))

    (let [p (ws-parse-url "ws://localhost/")]
      (assert (= p:port 80) "ws: default 80"))

    (let [p (ws-parse-url "wss://example.com")]
      (assert (= p:port 443) "wss: default 443")
      (assert (= p:path "/") "wss: default path"))

    (let [[ok? _] (protect (ws-parse-url "http://bad.com"))]
      (assert (not ok?) "rejects http://"))

    ## ── Accept key (RFC 6455 section 4.2.2 test vector) ──
    (let [key "dGhlIHNhbXBsZSBub25jZQ=="
          expected "NAwr/jm285Ly94AfF1mwjRaNwgQ="]
      (assert (= (compute-accept-key key) expected)
              "accept key: RFC 6455 test vector"))

    ## ── Masking ──
    (let* [data (bytes 1 2 3 4 5)
           mask (bytes 0xAA 0xBB 0xCC 0xDD)
           masked (apply-mask data mask)
           unmasked (apply-mask masked mask)]
      (assert (= unmasked data) "mask roundtrip"))

    (assert (= (apply-mask (bytes) (bytes 1 2 3 4)) (bytes)) "mask empty")

    ## ── Frame codec ──
    ## A frame is its parts: the header, then the payload as it was given, so
    ## a send writes the payload without copying it.
    (let* [payload (bytes 72 101 108 108 111)
           parts (frame-parts OP-BINARY payload)
           header (get parts 0)]
      (assert (= (length parts) 2) "unmasked frame: a header and the payload")
      (assert (= header (bytes 0x82 5))
              "unmasked frame: FIN, opcode 2, length 5")
      (assert (= (get parts 1) payload) "unmasked frame: the payload itself"))

    (let* [parts (frame-parts OP-TEXT "héllo")
           header (get parts 0)]
      (assert (= header (bytes 0x81 6)) "text frame: the length counts bytes")
      (assert (= (get parts 1) "héllo") "text frame: the string, not its bytes"))

    (let* [payload (apply bytes (map (fn [i] (rem i 256)) (range 200)))
           header (get (frame-parts OP-BINARY payload) 0)]
      (assert (= (length header) 4) "medium frame: a 16-bit extended length")
      (assert (= (bit/and (get header 1) 0x7F) 126)
              "medium frame: extended length marker")
      (let [ext-len (bit/or (bit/shl (get header 2) 8) (get header 3))]
        (assert (= ext-len 200) "medium frame: extended length value")))

    (let [byte0 (get (get (frame-parts OP-TEXT (bytes 65) :fin false) 0) 0)]
      (assert (= (bit/and byte0 0x80) 0) "non-fin: FIN bit clear")
      (assert (= (bit/and byte0 0x0F) OP-TEXT) "non-fin: opcode"))

    (let* [payload (bytes 1 2 3 4 5)
           parts (frame-parts OP-BINARY payload :mask? true)
           [header mask-key masked] parts]
      (assert (= (length parts) 3)
              "masked frame: a header, the key, the payload")
      (assert (= header (bytes 0x82 0x85)) "masked frame: the mask bit is set")
      (assert (= (length mask-key) 4) "masked frame: a four-byte key")
      (assert (= (apply-mask masked mask-key) payload)
              "masked frame: the payload masked with the key"))

    true)

  ## ── Exports ─────────────────────────────────────────────────────────

  {:connect ws-connect
   :send ws-send
   :recv ws-recv
   :close ws-close
   :ping ws-ping
   :upgrade ws-upgrade
   :serve ws-serve
   :parse-url ws-parse-url
   :test run-tests})
