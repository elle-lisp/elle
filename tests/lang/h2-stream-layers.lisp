(elle/epoch 12)
# Payload sizes that grow layer by layer on one session.
#
# A streaming response is one h2 stream held open across many DATA
# frames while the connection carries whatever else the caller is doing.
# What a session has to survive as a payload grows is the flow-control
# window: past a certain body size a single POST no longer fits the
# connection's credit, and the request only completes if WINDOW_UPDATEs
# keep returning it while the body is in flight.
#
# Both cases here walk six layers of growing bodies over one session and
# assert that both halves finish: every framed message arrives, every
# unary request answers 200, and the stream table comes back empty. The
# second is the same shape at full size — 650 jobs per layer, bodies up
# to 21 MB, each POST followed by a stream of 650 completion events.
#
# The framing is gRPC's: a compression byte and a 4-byte big-endian
# length per message, which is what makes "how many messages arrived" a
# question separate from "how many bytes arrived".
#
# See lib/http2/session.lisp and lib/http2/stream.lisp.

(def http2 ((import "std/http2")))

# A budget no unblocked case here can reach. The larger case moves about
# 55 MB through loopback.
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

# ── The layer sizes both cases walk ──────────────────────────────────

(def windows [5 20 60 120 180 250])

# ── 1. Six layers of growing bodies ──────────────────────────────────

(defn layered-pipeline []
  "Per layer: one POST whose body grows with the layer, then ten
   requests at once."
  (with-server (fn [req] {:status 200 :body (or req:body (bytes ""))})
               (fn [session _]
                 (each layer in (range 0 6)
                   (let* [size (* (get windows layer) 33 8 10)
                          body (make-body size)
                          resp (http2:send session "POST" "/echo" :body body)]
                     (assert (= resp:status 200)
                             (string "layers: the POST of layer " (string layer)))
                     (assert (= (length resp:body) size)
                             (string "layers: layer " (string layer)
                                     " echoed the whole body"))
                     (let [results (map ev/join
                                        (map (fn [i]
                                          (ev/spawn (fn []
                                            (http2:send session "GET"
                                            (concat "/f?L" (string layer) "&r="
                                            (string i)))))) (range 0 10)))]
                       (each r in results
                         (assert (= r:status 200)
                                 (string "layers: a request in layer "
                                 (string layer)))))))
                 (no-stream-leak session "layers")
                 true)))

# ── 2. The same shape at full size ───────────────────────────────────

(defn layered-pipeline-full []
  "650 jobs per layer: bodies from 429 KB to 21 MB, each followed by a
   stream of 650 completion events."
  (let [jobs 650
        events (event-frames 650)]
    (with-server (fn [req]
                   (cond
                     (= req:path "/submit") {:status 200 :body "submitted"}
                     (= req:path "/stream")
                       {:status 200
                        :headers {:content-type "application/grpc"}
                        :body events
                        :trailers [["grpc-status" "0"]]}
                     true {:status 200 :body "ok"}))
                 (fn [session _]
                   (each layer in (range 0 6)
                     (let* [size (* jobs (get windows layer) 33 4)
                            body (make-body size)
                            resp (http2:send session "POST" "/submit" :body body)]
                       (assert (= resp:status 200)
                               (string "full layers: the POST of layer "
                                       (string layer) " ("
                                       (string (/ size 1024)) " KB)"))
                       (assert (= (length (drain-stream (open-grpc session
                                  "/stream"))) jobs)
                               (string "full layers: layer " (string layer)
                                       " streamed " (string jobs) " events"))))
                   (settled session "full layers")
                   (no-stream-leak session "full layers")
                   true))))

# ── Run ──────────────────────────────────────────────────────────────

(println "six layers of growing bodies on one session...")

(timed "six layers of growing bodies" layered-pipeline)
(timed "six layers at 650 jobs, bodies to 21 MB" layered-pipeline-full)

(println "h2 stream layers: every layer echoed and streamed in full")
