(elle/epoch 15)
# audited: 2026-11-12
# The page bytes a write claims per byte it sends, through the primitives and through each library that writes a protocol.
# docs/impl/io/bytes.md
# docs/impl/region/diagnostics.md
#
# A write that joins a message's values before it sends them copies the body
# into fresh region pages, and frees the copy once the write returns. A leak
# gauge reads nothing then, because nothing stays alive, and io_copies.rs
# counts the Rust allocator, which a region page does not come from.
# `arena/page-bytes` counts the bytes of every page the heap claims, so a copy
# of a megabyte claims a megabyte even when it is freed at once.
#
# Each shape writes a payload of a megabyte and then of nine to a file, and
# the reading is the difference in page bytes over the difference in payload.
# The payload is built before the counter is read, so only what the write
# itself claims is counted. A constant cost, such as a frame header or a
# handshake, cancels in the difference. The rows are in
# tests/ledger/write-copies.lisp: a quarter of a byte per byte at most for
# every shape that must not copy.
#
# The control joins the payload with concat before it writes, and must claim a
# byte per byte at least. It holds the gauge to counting: a gauge that read
# nothing would pass every other row.

(def r ((import "std/ratchet")))
(def framing ((import "std/framing")))
(def redis ((import "std/redis")))
(def http-transport ((import "std/http/transport")))
(def h2-transport ((import "std/http2/transport")))
(def h2-frame ((import "std/http2/frame")))
(def ws
  ((import "std/websocket") :hash {:sha1 (fn [b]
                                           (apply bytes
                                             (map (fn [_] 0) (range 20))))}))

(def small 1000000)
(def large 9000000)

(defn copies [subject make shape]
  "Print the page bytes (shape port payload) claims per byte of payload,
   between a payload of `small` bytes and one of `large`. (make n) builds the
   payload."
  (with-temp-dir dir
    (let [run (fn [n]
                (let* [payload (make n)
                       p (port/open-bytes (path/join dir (string "out-" n))
                           :write)
                       before (arena/page-bytes)]
                  (shape p payload)
                  (let [spent (- (arena/page-bytes) before)]
                    (port/close p)
                    spent)))]
      (run small)
      (let [at-small (run small)
            at-large (run large)]
        (r:read subject
                :copies (/ (float (- at-large at-small)) (- large small))
                :unit "page bytes/byte")))))

(defn bytes-of [n]
  (bytes (string/repeat "x" n)))

(defn text-of [n]
  (string/repeat "x" n))

(copies "concat then port/write" bytes-of
        (fn [p payload] (port/write p (concat (bytes "head:") payload))))

(copies "port/write" bytes-of (fn [p payload] (port/write p payload)))

(copies "port/write-many" bytes-of
        (fn [p payload] (port/write-many p ["head:" payload "\n"])))

(copies "gather flush" bytes-of
        (fn [p payload]
          (let [g (framing:gather p)]
            (framing:write g "head:")
            (framing:write g payload)
            (framing:flush g))))

(copies "http transport" bytes-of
        (fn [p payload]
          (let [t (http-transport:tcp p)]
            (http-transport:write t "HTTP/1.1 200 OK\r\n")
            (http-transport:write t
                                  (string "content-length: " (length payload)
                                    "\r\n\r\n"))
            (http-transport:write t payload)
            (http-transport:flush t))))

(copies "http2 frame" bytes-of
        (fn [p payload]
          (let [t (h2-transport:tcp p)]
            (h2-frame:write-frame t 0 0 1 payload)
            (t:flush))))

# A server connection, whose frames go unmasked, upgraded over a transport
# that writes through a gather on the file port as the tcp transport does.
(defn ws-send [p payload]
  (let* [g (framing:gather p)
         t {:read nil
            :read-line nil
            :write (fn [d] (framing:write g d))
            :flush (fn [] (framing:flush g))
            :close (fn [] nil)}
         conn (ws:upgrade {:headers {:sec-websocket-key "dGhlIHNhbXBsZSBub25jZQ=="}}
                          t)]
    (ws:send conn payload)))

(copies "websocket send" bytes-of ws-send)
(copies "websocket text" text-of ws-send)

# A connection that writes its commands to the file port and reads its replies
# from a file of its own, bound by redis:using for the thunk send.
(defn redis-send [p send]
  (with-temp-dir dir
    (let [replies (path/join dir "replies")]
      (file/write replies "+OK\r\n+OK\r\n")
      (let [in (port/open-bytes replies :read)]
        (defer
          (port/close in)
          (redis:using (merge (redis:connection in) {:port p}) send))))))

(copies "redis command" bytes-of
        (fn [p payload] (redis-send p (fn [] (redis:set "k" payload)))))

(copies "redis pipeline" bytes-of
        (fn [p payload]
          (redis-send p
                      (fn []
                        (redis:pipeline ["SET" "k" payload] ["SET" "j" payload])))))

(println "write-copies: ok")
