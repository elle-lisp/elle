(elle/epoch 12)
# Bidi streams under a deadline, over a session that keeps getting
# replaced — varying what the SERVER does under the churn.
#
# `ev/timeout` races the work against a timer and aborts whichever loses,
# so every call tears a fiber down — twice per call, counting the timer.
# A bidi stream leaves a reader parked on the stream's data queue, and
# recycling the session closes the connection that reader sits on. Run
# the two together and the teardown of one stream overlaps the setup of
# the next.
#
# What must hold across all of it is that the session still answers. Each
# case below runs bidi streams under a budget, closes the session,
# reconnects, runs more, and finishes with a unary request. The unary
# request is the check: a session whose reader, stream table or
# flow-control window did not survive the teardown answers it with a
# hang rather than a 200.
#
# h2-timeout-recycle.lisp varies the recycle itself and carries the
# bare-queue control that separates the scheduler from the protocol. The
# cases here vary what the teardown has to interrupt: a handler that is
# genuinely slow rather than idle, a budget tight enough to bite, streams
# running concurrently so several teardowns overlap one recycle, unary
# requests after the streams, and a server that streams its response back
# rather than buffering it.
#
# See docs/concurrency.md § ev/timeout and docs/scheduler.md.

(def http2 ((import "std/http2")))

# A budget no unblocked stream here can reach. One case deliberately
# runs a tighter one.
(def deadline 30)

(defn listen-ephemeral []
  "A listening socket on a kernel-chosen port, with that port."
  (let* [l (tcp/listen "127.0.0.1" 0)
         p (port/path l)
         port (parse-int (slice p (+ 1 (string/find p ":"))))]
    [l port]))

# ── gRPC framing ─────────────────────────────────────────────────────

(defn grpc-frame [payload]
  "Prefix `payload` with the gRPC message header: a compression byte and
   a 4-byte big-endian length."
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

(def body-chunk (bytes 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19))

(defn make-body [n]
  "A body of `n` bytes by repeated doubling — O(log n) concats. Building
   it one chunk at a time is quadratic and outgrows the budget above long
   before the h2 work does."
  (let [@b body-chunk]
    (while (< (length b) n) (assign b (concat b b)))
    (slice b 0 n)))

# ── Handlers ─────────────────────────────────────────────────────────

(defn echo-handler [req]
  "Echo the request body with gRPC trailers."
  {:status 200
   :headers {:content-type "application/grpc"}
   :body (or req:body (bytes))
   :trailers [["grpc-status" "0"]]})

(defn delayed-echo-handler [delay-ms]
  "Echo after `delay-ms`, so the client's deadline runs against a server
   that is genuinely slow rather than idle."
  (fn [req]
    (ev/sleep (/ delay-ms 1000.0))
    {:status 200
     :headers {:content-type "application/grpc"}
     :body (or req:body (bytes))
     :trailers [["grpc-status" "0"]]}))

(defn streaming-echo-handler [req ctrl]
  "Echo message by message instead of buffering the whole body: read the
   request to its end, then send each framed message back as its own
   DATA frame."
  (def @body (bytes))
  (forever
    (let [data (ctrl:recv)]
      (when (nil? data) (break nil))
      (assign body (concat body data))))
  (ctrl:send-headers 200 :headers {:content-type "application/grpc"})
  (def @pos 0)
  (while (>= (- (length body) pos) 5)
    (let [len (bit/or (bit/shl (get body (+ pos 1)) 24)
                      (bit/shl (get body (+ pos 2)) 16)
                      (bit/shl (get body (+ pos 3)) 8) (get body (+ pos 4)))
          end (+ pos 5 len)]
      (when (> end (length body)) (break nil))
      (ctrl:send-data (slice body pos end))
      (assign pos end)))
  (ctrl:send-trailers [["grpc-status" "0"]]))

# ── One bidi exchange ────────────────────────────────────────────────

(defn do-bidi [session n-msgs msg-size]
  "Send `n-msgs` framed messages of `msg-size` bytes on one stream and
   require the echo to return every one of them."
  (let [framed (grpc-frame (make-body msg-size))]
    (def [sid s]
      (http2:open-stream session "POST" "/test.Svc/Method"
                         :headers [["content-type" "application/grpc"]
                                   ["te" "trailers"]]))
    (each _ in (range 0 n-msgs)
      (http2:stream-send session sid framed))
    (http2:stream-end session sid)
    (def @buf (bytes))
    (def @received 0)
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
              (assign received (+ received 1))
              (assign buf (get r 1)))))))
    (assert (= received n-msgs)
            (string "the echo returned " (string received) " of "
                    (string n-msgs) " messages"))
    received))

# ── The driver every case shares ─────────────────────────────────────

(defn run-recycles [opts]
  "Serve `opts:handler`, then repeat `opts:cycles` times: run
   `opts:streams` bidi streams under `opts:budget`, close the session and
   reconnect. Finish with a unary request the session must answer, plus
   `opts:unary-tail` more under their own budget.

   `opts:concurrent` runs the cycle's streams in parallel rather than one
   after another, so several teardowns overlap one recycle.
   `opts:streaming` serves through `http2:serve-streaming`."
  (let* [label opts:label
         handler (or (get opts :handler) echo-handler)
         cycles (or (get opts :cycles) 1)
         streams (or (get opts :streams) 5)
         msgs (or (get opts :msgs) 5)
         size (or (get opts :size) 500)
         budget (or (get opts :budget) deadline)
         tail (or (get opts :unary-tail) 0)
         [listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         sf (ev/spawn (fn []
                        (protect (if (get opts :streaming)
                                   (http2:serve-streaming listener handler)
                                   (http2:serve listener handler)))))
         @session (http2:connect url)]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each cycle in (range 0 cycles)
        (if (get opts :concurrent)
          (let* [fibers (map (fn [_]
                               (ev/spawn (fn []
                                 (ev/timeout budget
                                 (fn [] (do-bidi session msgs size))))))
                             (range 0 streams))
                 results (map ev/join fibers)]
            (each r in results
              (assert (not (nil? r))
                      (string label ": a concurrent stream in cycle "
                              (string cycle) " reached its budget"))))
          (each i in (range 0 streams)
            (let [r (ev/timeout budget (fn [] (do-bidi session msgs size)))]
              (assert (not (nil? r))
                      (string label ": stream " (string i) " of cycle "
                              (string cycle) " reached its budget")))))
        # Recycle: the streams just torn down were reading this connection.
        (http2:close session)
        (assign session (http2:connect url)))
      (let [resp (http2:send session "GET" "/health")]
        (assert (= resp:status 200)
                (string label ": the unary request after the last recycle")))
      (each i in (range 0 tail)
        (let [r (ev/timeout budget
                            (fn []
                              (http2:send session "GET"
                              (concat "/health?i=" (string i)))))]
          (assert (not (nil? r))
                  (string label ": trailing unary request " (string i)))))
      true)))

(defn run-case [label opts]
  "Run one case and name it on the way past."
  (println "  " label)
  (run-recycles (merge opts {:label label})))

# ── The cases ────────────────────────────────────────────────────────

(println "timed bidi streams against a server that varies...")

(run-case "a server that takes 50 ms per message"
          {:cycles 2
           :streams 5
           :msgs 5
           :size 500
           :handler (delayed-echo-handler 50)})

(run-case "a 2 s budget against a 50 ms server"
          {:cycles 5
           :streams 3
           :msgs 3
           :size 500
           :budget 2
           :handler (delayed-echo-handler 50)})

(run-case "four concurrent streams per recycle"
          {:cycles 5 :streams 4 :msgs 5 :size 500 :concurrent true})

(run-case "ten timed unary requests after the streams"
          {:cycles 2 :streams 10 :msgs 10 :size 1000 :unary-tail 10})

(run-case "a server that streams its response back"
          {:cycles 2
           :streams 10
           :msgs 10
           :size 1000
           :handler streaming-echo-handler
           :streaming true})

(println "h2 timeout serving: every session answered after its teardown")
