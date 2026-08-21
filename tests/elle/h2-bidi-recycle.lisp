(elle/epoch 12)
# Bidi streams with the connection replaced every fifty streams.
#
# A bidi stream is opened, written to many times, half-closed, and then
# read — so the client holds a send side and a receive side of the same
# stream at once. Two things accumulate as streams come and go on one
# session: stream ids, which are never reused, and the flow-control
# window each in-flight byte holds until a WINDOW_UPDATE returns it.
#
# This case discards both, repeatedly: 300 bidi streams, closing and
# reconnecting every 50. Each reconnect throws away a connection whose
# ids and window are half spent while the server keeps the same listener,
# so the server has to retire six readers and stream tables in turn. The
# requests after the final recycle are the check that it did.
#
# Each stream asserts the count of framed messages that came back, not
# the byte total — a window that stalls mid-stream and a stream that ends
# early both produce a short read, and the count is what names it.
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

# ── The connection replaced every fifty streams ──────────────────────

(defn bidi-with-recycling []
  "300 bidi streams, closing and reconnecting every 50. Each reconnect
   discards a connection whose stream ids and windows are half spent,
   and the requests at the end must still be answered."
  (let* [framed (grpc-frame (make-body 100))
         request-body (make-body 500)
         [listener url sf] (serve-on (fn [req]
                                       (if (string/starts-with? req:path
                                         "/test.Svc/")
                                         (echo-handler req)
                                         {:status 200 :body "ok"})))
         @session (http2:connect url)
         @recycles 0]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each i in (range 0 300)
        (when (and (> i 0) (= 0 (mod i 50)))
          (protect (http2:close session))
          (assign session (http2:connect url))
          (assign recycles (+ recycles 1)))
        (assert (= (length (bidi-messages session "/test.Svc/Bulk" framed 3)) 3)
                (string "bidi recycling: stream " (string i))))
      (assert (= recycles 5)
              (string "bidi recycling: five recycles, counted "
                      (string recycles)))
      (protect (http2:close session))
      (assign session (http2:connect url))
      (each i in (range 0 20)
        (let [resp (http2:send session "POST" (concat "/evaluate?i=" (string i))
                               :body request-body)]
          (assert (= resp:status 200)
                  (string "bidi recycling: request " (string i)
                          " after the final recycle"))))
      true)))

# ── Run ──────────────────────────────────────────────────────────────

(println "bidi streams with the connection replaced every fifty...")

(timed "300 bidi streams, recycling every 50" bidi-with-recycling)

(println "h2 bidi recycle: every stream returned every message it was sent")
