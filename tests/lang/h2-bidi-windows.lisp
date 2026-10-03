(elle/epoch 12)
# What one bidi stream carries in each direction.
#
# A bidi stream is opened, written to many times, half-closed, and then
# read — so the client holds a send side and a receive side of the same
# stream at once, and every message crosses two flow-control windows: the
# stream's and the connection's. The two cases here load those windows
# from opposite ends: a hundred messages out and back, then five bytes
# out against two megabytes back.
#
# The connection window starts near a megabyte, so the amplified case
# only completes if WINDOW_UPDATEs keep returning credit while the
# response is in flight. A window that stalls mid-stream and a stream
# that ends early both produce a short read, so each case asserts what
# arrived — a message count for the framed case, a byte total for the
# raw one — rather than that the read returned.
#
# The framing is gRPC's: a compression byte and a 4-byte big-endian
# length per message.
#
# See lib/http2/stream.lisp and lib/http2/session.lisp.

(def http2 ((import "std/http2")))

# A budget no unblocked case here can reach.
(def deadline 120)

(defn listen-ephemeral []
  "A listening socket on a kernel-chosen port, with that port."
  (let* [l (tcp/listen "127.0.0.1" 0)
         p (port/path l)
         port (parse-int (slice p (+ 1 (string/find p ":"))))]
    [l port]))

(def body-chunk (bytes 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19))

(defn make-body [n]
  "A body of `n` bytes by repeated doubling — O(log n) concats. Building
   it one chunk at a time is quadratic and outgrows the budget above long
   before the h2 work does."
  (let [@b body-chunk]
    (while (< (length b) n) (assign b (concat b b)))
    (slice b 0 n)))

(defn grpc-frame [payload]
  "Prefix `payload` with the gRPC message header."
  (let [len (length payload)]
    (concat (bytes 0 (bit/shr len 24) (bit/and (bit/shr len 16) 0xff)
                   (bit/and (bit/shr len 8) 0xff) (bit/and len 0xff)) payload)))

(defn grpc-read-frame [buf]
  "Split one framed message off `buf`, or nil if it holds less than one."
  (when (>= (length buf) 5)
    (let [len (bit/or (bit/shl (get buf 1) 24) (bit/shl (get buf 2) 16)
                      (bit/shl (get buf 3) 8) (get buf 4))
          end (+ 5 len)]
      (when (>= (length buf) end) [(slice buf 5 end) (slice buf end)]))))

(defn echo-handler [req]
  "Echo the request body with gRPC trailers."
  {:status 200
   :headers {:content-type "application/grpc"}
   :body (or req:body (bytes))
   :trailers [["grpc-status" "0"]]})

(defn serve-on [handler]
  "Serve `handler` on a fresh listener. Returns [listener url fiber]."
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn (fn [] (protect (http2:serve listener handler))))]
    [listener (concat "http://127.0.0.1:" (string lport)) sf]))

(defn with-server [handler test-fn]
  "Serve `handler` and call `test-fn` with a session on it."
  (let* [[listener url sf] (serve-on handler)
         session (http2:connect url)]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (test-fn session))))

(defn bidi-messages [session path framed n-msgs]
  "Open a bidi stream, send `framed` `n-msgs` times, half-close, and
   return every framed message the response carried."
  (def [sid s]
    (http2:open-stream session "POST" path
                       :headers [["content-type" "application/grpc"]
                                 ["te" "trailers"]]))
  (each _ in (range 0 n-msgs)
    (http2:stream-send session sid framed))
  (http2:stream-end session sid)
  (def @buf (bytes))
  (def @messages @[])
  (def @done false)
  (while (not done)
    (let [msg (s:data-queue:take)]
      (match msg:type
        :headers (when msg:end-stream (assign done true))
        :data
          (begin
            (assign buf (concat buf msg:data))
            (when msg:end-stream (assign done true)))
        _ (assign done true)))
    (while true
      (let [r (grpc-read-frame buf)]
        (if (nil? r)
          (break nil)
          (begin
            (push messages (get r 0))
            (assign buf (get r 1)))))))
  (assert (= (length buf) 0) "the stream ended on a message boundary")
  (freeze messages))

(defn timed [label thunk]
  "Run `thunk` under the file's budget and name it."
  (let* [t0 (clock/monotonic)
         r (ev/timeout deadline thunk)
         elapsed (- (clock/monotonic) t0)]
    (assert (not (nil? r))
            (string label ": no result in " (string deadline) " s"))
    (println "  " label ": " (string (round elapsed)) " s")
    r))

# ── 1. One stream, a hundred messages ────────────────────────────────

(defn bidi-hundred []
  "100 messages of 500 bytes out and back on one stream."
  (with-server echo-handler
               (fn [session]
                 (let [messages (bidi-messages session "/test.Svc/Bidi"
                       (grpc-frame (make-body 500)) 100)]
                   (assert (= (length messages) 100)
                           (string "bidi 100: 100 messages, got "
                                   (string (length messages))))
                   (each m in messages
                     (assert (= (length m) 500)
                             "bidi 100: every message is 500 bytes")))
                 true)))

# ── 2. A small request, a large response ─────────────────────────────

(defn amplified-bidi []
  "Five bytes out, two megabytes back. The connection window starts near
   a megabyte, so the response only completes if WINDOW_UPDATEs keep
   returning credit while it is in flight."
  (let [response-size (* 2 1024 1024)
        body (make-body response-size)]
    (with-server (fn [req]
                   {:status 200
                    :headers {:content-type "application/grpc"}
                    :body body
                    :trailers [["grpc-status" "0"]]})
                 (fn [session]
                   (def [sid s]
                     (http2:open-stream session "POST" "/test.Svc/Amplified"
                                        :headers [["content-type"
                                        "application/grpc"] ["te" "trailers"]]))
                   (http2:stream-send session sid (grpc-frame (bytes "hello")))
                   (http2:stream-end session sid)
                   (def @total 0)
                   (def @done false)
                   (while (not done)
                     (let [msg (s:data-queue:take)]
                       (match msg:type
                         :headers (when msg:end-stream (assign done true))
                         :data
                           (begin
                             (assign total (+ total (length msg:data)))
                             (when msg:end-stream (assign done true)))
                         _ (assign done true))))
                   # The handler answers with the raw body, so what must arrive is
                   # exactly the response size — no framing header of its own.
                   (assert (= total response-size)
                           (string "amplified: the whole response arrived, got "
                                   (string total) " of " (string response-size)
                                   " bytes"))
                   true))))

# ── Run ──────────────────────────────────────────────────────────────

(println "one bidi stream against both flow-control windows...")

(timed "100 messages on one stream" bidi-hundred)
(timed "5 bytes out, 2 MB back" amplified-bidi)

(println "h2 bidi windows: every stream returned every byte it was owed")
