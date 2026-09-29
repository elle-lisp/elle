(elle/epoch 12)
# Many streams, one after another, on one session.
#
# A streaming response is one h2 stream held open across many DATA
# frames while the connection carries whatever else the caller is doing.
# The reader fiber that demultiplexes frames serves every stream at once,
# so what accumulates when streams come and go is the stream table: an
# entry left behind after a drain returns is a stream the session will
# never close.
#
# Both cases here open streams in bulk and then ask the session to keep
# working. The first drains 78 streams back to back and follows them with
# ordinary requests. The second interleaves the two per layer: 13 chunked
# submits, then 13 streams, six layers deep, with the submit body growing
# each layer. Each asserts that every framed message arrived, every
# request answered 200, and the stream table came back empty.
#
# The framing is gRPC's: a compression byte and a 4-byte big-endian
# length per message, which is what makes "how many messages arrived" a
# question separate from "how many bytes arrived".
#
# See lib/http2/session.lisp and lib/http2/stream.lisp.

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

(defn event-frames [n]
  "`n` framed messages, each carrying its own index in two bytes."
  (apply concat
         (map (fn [i] (grpc-frame (bytes (bit/shr i 8) (bit/and i 0xff) 0 0)))
              (range 0 n))))

(defn with-server [handler test-fn]
  "Serve `handler` on a fresh listener and call `test-fn` with a session."
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn (fn [] (protect (http2:serve listener handler))))
         session (http2:connect (concat "http://127.0.0.1:" (string lport)))]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (test-fn session lport))))

(defn open-grpc [session path]
  "Start a gRPC-shaped request and return its stream handle."
  (http2:send-raw session "POST" path :body (bytes)
                  :headers [["content-type" "application/grpc"]
                            ["te" "trailers"]]))

(defn drain-stream [s]
  "Read `s` to its end and return every framed message it carried."
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

(defn settled [session label]
  "One round trip after the last stream. A session records a stream's end
   when it processes the end of the response, which is after the last
   message reaches the caller — so a count taken the instant a drain
   returns can still see the stream that just finished. This request both
   waits for that and proves the session still answers."
  (let [resp (http2:send session "GET" "/settle")]
    (assert (= resp:status 200)
            (string label ": the session answered after its last stream"))))

(defn no-stream-leak [session label]
  "The stream table is empty once every stream has been read out."
  (assert (= (length (keys session:streams)) 0)
          (string label ": the stream table came back empty")))

(defn timed [label thunk]
  "Run `thunk` under the file's budget and name it."
  (let* [t0 (clock/monotonic)
         r (ev/timeout deadline thunk)
         elapsed (- (clock/monotonic) t0)]
    (assert (not (nil? r))
            (string label ": no result in " (string deadline) " s"))
    (println "  " label ": " (string (round elapsed)) " s")
    r))

# ── 1. Many streams back to back, then requests ──────────────────────

(defn many-streams-then-unary []
  "Drain 78 streams of 50 messages, then make 20 requests. The requests
   are the check that 78 finished streams left nothing behind."
  (let [body (event-frames 50)]
    (with-server (fn [req]
                   (if (= req:path "/stream")
                     {:status 200
                      :headers {:content-type "application/grpc"}
                      :body body
                      :trailers [["grpc-status" "0"]]}
                     {:status 200 :body "ok"}))
                 (fn [session _]
                   (each i in (range 0 78)
                     (assert (= (length (drain-stream (open-grpc session
                                        "/stream"))) 50)
                             (string "many streams: stream " (string i)
                                     " delivered 50 messages")))
                   (each i in (range 0 20)
                     (let [resp (http2:send session "GET"
                           (concat "/unary?i=" (string i)))]
                       (assert (= resp:status 200)
                               (string "many streams: request " (string i)
                                       " after 78 streams"))))
                   (settled session "many streams")
                   (no-stream-leak session "many streams")
                   true))))

# ── 2. Chunked submits, one stream per chunk ─────────────────────────

# The layer sizes the chunked submits walk.
(def windows [5 20 60 120 180 250])

(defn chunked-multi-subscribe []
  "Per layer: 13 POSTs of 50 jobs each, then 13 streams of 50 events.
   Six layers on one session."
  (let [chunks 13
        per-chunk 50
        body (event-frames 50)]
    (with-server (fn [req]
                   (cond
                     (string/starts-with? req:path "/submit") {:status 200
                     :body "ok"}
                     (string/starts-with? req:path "/stream")
                       {:status 200
                        :headers {:content-type "application/grpc"}
                        :body body
                        :trailers [["grpc-status" "0"]]}
                     true {:status 200 :body "ok"}))
                 (fn [session _]
                   (each layer in (range 0 6)
                     (let [chunk-body (make-body (* per-chunk
                           (get windows layer) 33 4))]
                       (each c in (range 0 chunks)
                         (let [resp (http2:send session "POST"
                               (concat "/submit/L" (string layer) "/C"
                                       (string c)) :body chunk-body)]
                           (assert (= resp:status 200)
                                   (string "chunked: layer " (string layer)
                                   " chunk " (string c) " submitted"))))
                       (each c in (range 0 chunks)
                         (assert (= (length (drain-stream (open-grpc session
                                    (concat "/stream/L" (string layer) "/C"
                                    (string c))))) per-chunk)
                                 (string "chunked: layer " (string layer)
                                 " chunk " (string c) " streamed "
                                 (string per-chunk) " events")))))
                   (settled session "chunked")
                   (no-stream-leak session "chunked")
                   true))))

# ── Run ──────────────────────────────────────────────────────────────

(println "many streams one after another on one session...")

(timed "78 streams then 20 requests" many-streams-then-unary)
(timed "13 chunked submits and streams per layer" chunked-multi-subscribe)

(println "h2 stream fanout: every stream drained and every request answered")
