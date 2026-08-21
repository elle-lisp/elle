(elle/epoch 12)
# A server-streaming response sharing a session with ordinary requests.
#
# A streaming response is one h2 stream held open across many DATA
# frames while the connection carries whatever else the caller is doing.
# That is where a session's two halves can come apart: the reader fiber
# that demultiplexes frames serves every stream at once, so a stream
# nobody is draining, or a response far larger than the flow-control
# window, stalls the requests behind it rather than only itself.
#
# Each case below puts a stream and a request in each other's way and
# asserts that both finish: every framed message arrives, every unary
# request answers 200, and the stream table comes back empty.
#
# The cases escalate: one long response, a submit/stream/fetch round
# trip, a slow stream with requests running past it, then a stream
# abandoned half-read.
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

# ── 1. One response of many messages ─────────────────────────────────

(defn stream-reader []
  "100 messages of 1200 bytes in one response — more than one DATA frame
   holds, so the reader has to rejoin messages split across frames."
  (let [body (apply concat
                    (map (fn [_] (grpc-frame (make-body 1200))) (range 0 100)))]
    (with-server (fn [req]
                   (if (= req:path "/stream")
                     {:status 200
                      :headers {:content-type "application/grpc"}
                      :body body
                      :trailers [["grpc-status" "0"]]}
                     {:status 200 :body "ok"}))
                 (fn [session _]
                   (let [messages (drain-stream (open-grpc session "/stream"))]
                     (assert (= (length messages) 100)
                             (string "stream reader: 100 messages, got "
                                     (string (length messages))))
                     (each m in messages
                       (assert (= (length m) 1200)
                               "stream reader: every message is 1200 bytes")))
                   true))))

# ── 2. Submit, stream the completions, fetch each result ─────────────

(defn bulk-submit-stream-fetch []
  "One POST, one stream of 200 completion events, then 200 result
   requests 32 at a time — all on one session."
  (let [jobs 200
        events (event-frames jobs)]
    (with-server (fn [req]
                   (cond
                     (= req:path "/submit") {:status 200 :body "submitted"}
                     (= req:path "/events")
                       {:status 200
                        :headers {:content-type "application/grpc"}
                        :body events
                        :trailers [["grpc-status" "0"]]}
                     (string/starts-with? req:path "/result/")
                       {:status 200 :body (concat "result:" (slice req:path 8))}
                     true {:status 200 :body "ok"}))
                 (fn [session _]
                   (assert (= (get (http2:send session "POST" "/submit"
                                   :body "jobs") :status) 200)
                           "bulk: the submit was accepted")
                   (let [messages (drain-stream (open-grpc session "/events"))]
                     (assert (= (length messages) jobs)
                             (string "bulk: " (string jobs) " completions, got "
                                     (string (length messages))))
                     (let [ids (map (fn [m]
                                      (bit/or (bit/shl (get m 0) 8) (get m 1)))
                                    messages)
                           @fetched 0
                           @offset 0]
                       (while (< offset (length ids))
                         (let* [end (min (+ offset 32) (length ids))
                                batch (->list (slice (->array ids) offset end))
                                results (map ev/join
                                (map (fn [id]
                                       (ev/spawn (fn []
                                         (http2:send session "GET"
                                         (concat "/result/" (string id))))))
                                     batch))]
                           (each r in results
                             (assert (= r:status 200)
                                     "bulk: every result was 200")
                             (assign fetched (+ fetched 1)))
                           (assign offset end)))
                       (assert (= fetched jobs)
                               (string "bulk: fetched " (string fetched) " of "
                                       (string jobs)))))
                   (no-stream-leak session "bulk")
                   true))))

# ── 3. Requests run past a stream that has not answered yet ──────────

(defn stream-plus-unary []
  "Open a response the server delays half a second, make 20 requests
   while it is outstanding, then read it."
  (let [body (event-frames 5)]
    (with-server (fn [req]
                   (cond
                     (= req:path "/slow-stream")
                       (begin
                         (ev/sleep 0.5)
                         {:status 200
                          :headers {:content-type "application/grpc"}
                          :body body
                          :trailers [["grpc-status" "0"]]})
                     true {:status 200 :body "ok"}))
                 (fn [session _]
                   (def s (open-grpc session "/slow-stream"))
                   (each i in (range 0 20)
                     (let [resp (http2:send session "GET"
                           (concat "/fixed?i=" (string i)))]
                       (assert (= resp:status 200)
                               (string "stream+unary: request " (string i)
                                       " answered while the stream was open"))))
                   (assert (= (length (drain-stream s)) 5)
                           "stream+unary: the slow stream still delivered its messages")
                   true))))

# ── 4. A stream nobody finishes reading ──────────────────────────────

(defn abandoned-stream []
  "Read three messages of a hundred and stop. The reader fiber serves
   every stream, so an abandoned queue must not hold up the requests
   that follow."
  (let [body (apply concat
                    (map (fn [_] (grpc-frame (make-body 200))) (range 0 100)))]
    (with-server (fn [req]
                   (cond
                     (= req:path "/stream")
                       (begin
                         (ev/sleep 0.1)
                         {:status 200
                          :headers {:content-type "application/grpc"}
                          :body body
                          :trailers [["grpc-status" "0"]]})
                     true {:status 200 :body "ok"}))
                 (fn [session _]
                   (def s (open-grpc session "/stream"))
                   (def @buf (bytes))
                   (def @read 0)
                   (block :enough
                     (each _ in (range 0 10)
                       (let [msg (s:data-queue:take)]
                         (when (= msg:type :data)
                           (assign buf (concat buf msg:data))
                           (while true
                             (let [r (grpc-read-frame buf)]
                               (if (nil? r)
                                 (break nil)
                                 (begin
                                   (assign read (+ read 1))
                                   (assign buf (get r 1))
                                   (when (>= read 3) (break :enough nil))))))))))
                   (assert (>= read 3) "abandoned: three messages were read")
                   (each i in (range 0 10)
                     (let [resp (http2:send session "GET"
                           (concat "/fixed?i=" (string i)))]
                       (assert (= resp:status 200)
                               (string "abandoned: request " (string i)
                                       " answered past the abandoned stream"))))
                   true))))

# ── Run ──────────────────────────────────────────────────────────────

(println "a stream and ordinary requests sharing one session...")

(timed "100 messages in one response" stream-reader)
(timed "submit, 200-event stream, 200 parallel fetches" bulk-submit-stream-fetch)
(timed "20 requests past a half-second stream" stream-plus-unary)
(timed "a stream abandoned after three messages" abandoned-stream)

(println "h2 stream share: every stream drained and every request answered")
