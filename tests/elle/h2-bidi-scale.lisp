(elle/epoch 12)
# Bidirectional streams, one after another and at size.
#
# A bidi stream is opened, written to many times, half-closed, and then
# read — so the client holds a send side and a receive side of the same
# stream at once, and every message crosses two flow-control windows: the
# stream's and the connection's. Two things accumulate as streams come
# and go on one session: stream ids, which are never reused, and the
# window each in-flight byte holds until a WINDOW_UPDATE returns it.
#
# The cases below push on both. First one stream carrying more messages
# than a window holds, then a response far larger than the request that
# asked for it, then hundreds of streams in a row on one connection —
# plain, followed by requests, and with the connection replaced every
# fifty streams.
#
# Every case asserts the count of framed messages that came back, not the
# byte total: a window that stalls mid-stream and a stream that ends
# early both produce a short read, and the count is what names it.
#
# See lib/http2/stream.lisp and lib/http2/session.lisp.

(def http2 ((import "std/http2")))

# A budget no unblocked case here can reach. The largest moves 10000
# messages through one stream and 2 MB through another.
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

(defn settled [session label]
  "One round trip after the last stream. A session records a stream's end
   when it processes the end of the response, which is after the last
   message reaches the caller — so a count taken the instant a read
   returns can still see the stream that just finished."
  (let [resp (http2:send session "GET" "/settle")]
    (assert (= resp:status 200)
            (string label ": the session answered after its last stream"))))

(defn no-stream-leak [session label]
  "The stream table is empty once every stream has been read out."
  (assert (= (length (keys session:streams)) 0)
          (string label ": the stream table came back empty")))

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

# ── 2. One stream, ten thousand messages ─────────────────────────────

(defn bidi-ten-thousand []
  "10000 messages of 100 bytes on one stream — a megabyte of payload
   through both windows, and ten thousand round trips of stream state."
  (with-server echo-handler
               (fn [session]
                 (let [messages (bidi-messages session "/test.Svc/Bidi10k"
                       (grpc-frame (make-body 100)) 10000)]
                   (assert (= (length messages) 10000)
                           (string "bidi 10k: 10000 messages, got "
                                   (string (length messages)))))
                 (settled session "bidi 10k")
                 (no-stream-leak session "bidi 10k")
                 true)))

# ── 3. A small request, a large response ─────────────────────────────

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

# ── 4. Three hundred streams in a row ────────────────────────────────

(defn sequential-bidi []
  "300 bidi streams on one connection, five messages each. Stream ids
   are never reused, so this is 600 of them on one session."
  (let [framed (grpc-frame (make-body 200))]
    (with-server echo-handler
                 (fn [session]
                   (each i in (range 0 300)
                     (assert (= (length (bidi-messages session
                                        (concat "/test.Svc/Evolve" (string i))
                                        framed 5)) 5)
                             (string "sequential bidi: stream " (string i)
                                     " returned five messages")))
                   (settled session "sequential bidi")
                   (no-stream-leak session "sequential bidi")
                   true))))

# ── 5. Streams, then ordinary requests on the same session ───────────

(defn bidi-then-unary []
  "252 bidi streams, then 50 requests on the same connection. The
   requests are the check that the streams left the session usable."
  (let [framed (grpc-frame (make-body 100))
        request-body (make-body 500)]
    (with-server (fn [req]
                   (if (string/starts-with? req:path "/test.Svc/")
                     (echo-handler req)
                     {:status 200 :body "ok"}))
                 (fn [session]
                   (each i in (range 0 252)
                     (assert (= (length (bidi-messages session "/test.Svc/Bulk"
                                        framed 3)) 3)
                             (string "bidi then unary: stream " (string i))))
                   (each i in (range 0 50)
                     (let [resp (http2:send session "POST"
                           (concat "/evaluate?i=" (string i)) :body request-body)]
                       (assert (= resp:status 200)
                               (string "bidi then unary: request " (string i)))))
                   (settled session "bidi then unary")
                   (no-stream-leak session "bidi then unary")
                   true))))

# ── 6. The connection replaced every fifty streams ───────────────────

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

(println "bidi streams, one after another and at size...")

(timed "100 messages on one stream" bidi-hundred)
(timed "10000 messages on one stream" bidi-ten-thousand)
(timed "5 bytes out, 2 MB back" amplified-bidi)
(timed "300 sequential bidi streams" sequential-bidi)
(timed "252 bidi streams then 50 requests" bidi-then-unary)
(timed "300 bidi streams, recycling every 50" bidi-with-recycling)

(println "h2 bidi scale: every stream returned every message it was sent")
