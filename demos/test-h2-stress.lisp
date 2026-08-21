(elle/epoch 12)
## infra/test-h2-stress.lisp — h2 stress tests for grace usage patterns
##
## Pure h2 loopback tests — no grace code, no grace server dependency.
## Uses http2:serve for the server side, http2:connect/send/close for client.
##
## Motivated by cohort.lisp hangs: sequential large payloads, concurrent
## polling, close+reconnect cycles, and the combined pattern.
##
## Usage:
##   cd ~/git/elle && ELLE_PATH=target/release elle ~/git/grace/infra/test-h2-stress.lisp

(def http2 ((import "std/http2")))
(def h2-frame ((import "std/http2/frame")))
(def FC h2-frame:constants)
(def sync ((import "std/sync")))

## ── Helpers ──────────────────────────────────────────────────────────────

(defn round4 [x]
  (/ (round (* x 10000.0)) 10000.0))

(defn listen-ephemeral []
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
    [listener lport]))

(defn make-body [size]
  "Create a body of approximately `size` bytes."
  (let [@chunks @[]]
    (each _ in (range 0 (/ size 20))
      (push chunks (bytes 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19)))
    (apply concat chunks)))

## ── Server ───────────────────────────────────────────────────────────────
##
## Stateful handler supporting multiple routes:
##   /echo       — echo request body back
##   /delay      — sleep 100ms then respond
##   /status/:id — returns "pending" first N times, then "done"
##   /fixed      — immediate 200 with small body

(defn make-handler []
  "Create a request handler with per-path state tracking."
  (let [@status-counts @{}]
    (fn [req]
      (let [path req:path]
        (cond
          (= path "/echo")
            {:status 200 :body (or req:body (bytes ""))}
          (= path "/delay") (begin
                              (ev/sleep 0.1)
                              {:status 200 :body "delayed"})

          (string/starts-with? path "/status/")
            (let* [id (slice path 8)
                   count (or (get status-counts id) 0)
                   _ (put status-counts id (+ count 1))]
              (if (< count 3)
                {:status 200 :body "pending"}
                {:status 200 :body "done"}))
          (= path "/fixed") {:status 200 :body "ok"}

          true {:status 200 :body (concat "echo:" path)})))))

(defn with-server [handler test-fn]
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn (fn []
                        (let [[ok? _] (protect (http2:serve listener handler))]
                          nil)))
         session (http2:connect (concat "http://127.0.0.1:" (string lport)))]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (test-fn session lport))))

(def h2-transport ((import "std/http2/transport")))

## ── Test harness ─────────────────────────────────────────────────────────

(def @test-count 0)
(def @pass-count 0)
(def @fail-count 0)
(def @failures @[])

(defn run-test [name thunk]
  (assign test-count (+ test-count 1))
  (let [[ok? err] (protect (ev/timeout 30 thunk))]
    (cond
      (and ok? (not (nil? err)))
        (begin
          (assign pass-count (+ pass-count 1))
          (println "  PASS: " name))
      (and ok? (nil? err))
        (begin
          (assign fail-count (+ fail-count 1))
          (push failures name)
          (println "  FAIL: " name " (timeout after 30s)"))
      true
        (begin
          (assign fail-count (+ fail-count 1))
          (push failures name)
          (println "  FAIL: " name " — " err)))))

## ── Test 1: Many sequential streams with large bodies ────────────────────

(defn test-sequential-large-bodies []
  "Submit 200 sequential requests with 50KB request bodies on one session."
  (let [body-50k (make-body 50000)]
    (with-server (make-handler)
                 (fn [session _]
                   (each i in (range 0 200)
                     (when (= 0 (mod i 50)) (println "    progress: " i "/200"))
                     (let [resp (http2:send session "POST" "/echo"
                           :body body-50k)]
                       (assert (= resp:status 200)
                               (concat "seq-large: request " (string i)
                                       " status"))
                       (assert (= (length resp:body) (length body-50k))
                               (concat "seq-large: request " (string i)
                                       " body size"))))
                   (assert (= (length (keys session:streams)) 0)
                           "seq-large: no stream leak")
                   true))))

## ── Test 2: High concurrency (32 parallel requests) ──────────────────────

(defn test-high-concurrency []
  "Spawn 32 fibers each making a request simultaneously, repeat 10 times."
  (with-server (make-handler)
               (fn [session _]
                 (each round in (range 0 10)
                   (when (= 0 (mod round 5)) (println "    round: " round "/10"))
                   (let [fibers (map (fn [i]
                                       (ev/spawn (fn []
                                         (http2:send session "GET"
                                         (concat "/fixed?r=" (string round)
                                         "&i=" (string i)))))) (range 0 32))
                         results (map ev/join fibers)]
                     (each r in results
                       (assert (= r:status 200) "concurrent: status 200"))
                     (assert (= (length results) 32)
                             (concat "concurrent: round " (string round) " got "
                                     (string (length results)) " results"))))
                 (assert (= (length (keys session:streams)) 0)
                         "concurrent: no stream leak")
                 true)))

## ── Test 3: Close + reconnect cycles under load ─────────────────────────

(defn test-reconnect-cycles []
  "Loop 20 times: connect -> send 50 requests -> close -> reconnect."
  (let* [[listener lport] (listen-ephemeral)
         handler (make-handler)
         sf (ev/spawn (fn []
                        (let [[ok? _] (protect (http2:serve listener handler))]
                          nil)))
         url (concat "http://127.0.0.1:" (string lport))]
    (defer
      (begin
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each cycle in (range 0 20)
        (when (= 0 (mod cycle 5)) (println "    cycle: " cycle "/20"))
        (let [session (http2:connect url)]
          (each i in (range 0 50)
            (let [resp (http2:send session "GET"
                                   (concat "/fixed?c=" (string cycle) "&i="
                                   (string i)))]
              (assert (= resp:status 200)
                      (concat "reconnect: cycle " (string cycle) " req "
                              (string i)))))
          (http2:close session)))
      true)))

## ── Test 4: Concurrent polling loop (await-runs pattern) ─────────────────

(defn test-concurrent-polling []
  "Submit 64 requests, then poll 64 status endpoints concurrently (32 at a
   time) until all return done. Mirrors grace:await-runs."
  (with-server (make-handler)
               (fn [session _]  ## Phase 1: submit 64 requests (getting back 'pending')
                 (println "    submitting 64 requests...")
                 (each i in (range 0 64)
                   (let [resp (http2:send session "POST" "/fixed" :body "submit")]
                     (assert (= resp:status 200)
                             (concat "polling-submit: req " (string i)))))

                 ## Phase 2: poll /status/:id concurrently in rounds until all done
                 (let [@pending (->list (map (fn [i] (string i)) (range 0 64)))
                       @poll-round 0]
                   (while (and (not (empty? pending)) (< poll-round 20))
                     (when (= 0 (mod poll-round 5))
                       (println "    poll round " (string poll-round) ", "
                                (string (length pending)) " pending"))  ## Poll current pending set, 32 at a time
                     (let [@next-pending @[]
                           n (length pending)
                           batch1 (if (<= n 32)
                                    pending
                                    (->list (slice (->array pending) 0 32)))
                           batch2 (if (<= n 32)
                                    []
                                    (->list (slice (->array pending) 32)))
                           batches (if (empty? batch2) [batch1] [batch1 batch2])]
                       (each batch in batches
                         (let [fibers (map (fn [id]
                                 (ev/spawn (fn []
                                   [id
                                    (http2:send session "GET"
                                    (concat "/status/" id))]))) batch)
                               results (map ev/join fibers)]
                           (each result in results
                             (let [id (get result 0)
                                   resp (get result 1)]
                               (assert (= resp:status 200)
                                       (concat "poll: id " id " status"))
                               (when (= (string resp:body) "pending")
                                 (push next-pending id))))))
                       (assign pending next-pending))
                     (assign poll-round (+ poll-round 1)))
                   (assert (empty? pending)
                           (concat "polling: " (string (length pending))
                                   " still pending after 20 rounds")))
                 (assert (= (length (keys session:streams)) 0)
                         "polling: no stream leak")
                 true)))

## ── Test 5: Large request + concurrent poll interleave ───────────────────

(defn test-large-then-concurrent []
  "Submit 20 large (50KB) requests sequentially, then immediately spawn 20
   concurrent requests on the same session. Repeat 5 cycles."
  (let [body-50k (make-body 50000)]
    (with-server (make-handler)
                 (fn [session _]
                   (each cycle in (range 0 5)
                     (println "    cycle " cycle "/5")  ## Sequential large sends
                     (each i in (range 0 20)
                       (let [resp (http2:send session "POST" "/echo"
                             :body body-50k)]
                         (assert (= resp:status 200)
                                 (concat "interleave: cycle " (string cycle)
                                 " large req " (string i)))))  ## Concurrent polls
                     (let [fibers (map (fn [i]
                                         (ev/spawn (fn []
                                           (http2:send session "GET"
                                           (concat "/fixed?c=" (string cycle)
                                           "&i=" (string i)))))) (range 0 20))
                           results (map ev/join fibers)]
                       (each r in results
                         (assert (= r:status 200)
                                 "interleave: concurrent status 200"))
                       (assert (= (length results) 20)
                               (concat "interleave: cycle " (string cycle)
                                       " concurrent results"))))
                   (assert (= (length (keys session:streams)) 0)
                           "interleave: no stream leak")
                   true))))

## ── Test 6: Session durability (500 requests, no reconnect) ──────────────

(defn test-session-durability []
  "Send 500 sequential requests on one session with 10KB request and 10KB
   response bodies. Tests cumulative memory/stream-id/window behavior."
  (let [body-10k (make-body 10000)]
    (with-server (fn [req] {:status 200 :body (make-body 10000)})
                 (fn [session _]
                   (each i in (range 0 500)
                     (when (= 0 (mod i 100)) (println "    progress: " i "/500"))
                     (let [resp (http2:send session "POST" "/echo"
                           :body body-10k)]
                       (assert (= resp:status 200)
                               (concat "durability: request " (string i)
                                       " status"))))
                   (assert (= (length (keys session:streams)) 0)
                           "durability: no stream leak")
                   true))))

## ── gRPC framing helpers ───────────────────────────────────────────────────

(defn grpc-frame [payload-bytes]
  "Wrap payload in gRPC 5-byte length-prefixed frame (no compression)."
  (let [len (length payload-bytes)]
    (concat (bytes 0 (bit/shr len 24) (bit/and (bit/shr len 16) 0xff)
                   (bit/and (bit/shr len 8) 0xff) (bit/and len 0xff))
            payload-bytes)))

(defn grpc-read-frame [buf]
  "Try to extract one gRPC frame from buf.
   Returns [payload remaining-buf] or nil if incomplete."
  (when (>= (length buf) 5)
    (let [len (bit/or (bit/shl (get buf 1) 24) (bit/shl (get buf 2) 16)
                      (bit/shl (get buf 3) 8) (get buf 4))
          frame-end (+ 5 len)]
      (when (>= (length buf) frame-end)
        [(slice buf 5 frame-end) (slice buf frame-end)]))))

## ── Test 7: Stream reader pattern (Subscribe mock) ────────────────────────

(defn test-stream-reader []
  "Server returns 100 gRPC-framed messages in one response body (>100KB total,
   spanning many DATA frames). Client uses send-raw + data-queue to read and
   parse frames incrementally. Mirrors grpc:call-stream / grace:subscribe."
  (let [## Build 100 messages, each ~1200 bytes (total ~120KB > 7 DATA frames)
        msgs (map (fn [i] (make-body 1200)) (range 0 100))
        big-body (apply concat (map grpc-frame msgs))]
    (with-server (fn [req]
                   (if (= req:path "/stream")
                     {:status 200
                      :headers {:content-type "application/grpc"}
                      :body big-body
                      :trailers [["grpc-status" "0"]]}
                     {:status 200 :body "ok"}))
                 (fn [session _]
                   (def s
                     (http2:send-raw session "POST" "/stream" :body (bytes)
                                     :headers [["content-type"
                                     "application/grpc"] ["te" "trailers"]]))  ## Read from data-queue, accumulate buffer, extract gRPC frames
                   (def @buf (bytes))
                   (def @received @[])
                   (def @done false)
                   (while (not done)
                     (let [msg (s:data-queue:take)]
                       (match msg:type
                         :headers (when msg:end-stream (assign done true))
                         :data
                           (begin
                             (assign buf (concat buf msg:data))
                             (when msg:end-stream (assign done true)))
                         _ (assign done true)))  ## Extract complete frames from buffer
                     (while true
                       (let [result (grpc-read-frame buf)]
                         (if (nil? result)
                           (break nil)
                           (begin
                             (push received (get result 0))
                             (assign buf (get result 1)))))))
                   (assert (= (length received) 100)
                           (concat "stream: expected 100 messages, got "
                                   (string (length received))))  ## Verify each message matches
                   (each i in (range 0 100)
                     (assert (= (length (get received i)) 1200)
                             (concat "stream: message " (string i) " size")))
                   (assert (= (length buf) 0) "stream: no leftover bytes")
                   true))))

## ── Test 8: Bulk submit + stream await + parallel fetch ───────────────────

(defn test-bulk-submit-stream-fetch []
  "Full Phase 1 pattern on one session, no reconnects:
   1. POST /submit (bulk-evolve mock) → get back 200 'job IDs'
   2. POST /events (Subscribe mock) → stream of gRPC-framed completion events
   3. For each completed ID, parallel GET /result/:id (GetRun mock)
   All on the same h2 session."
  (let [num-jobs 200  ## Build completion event stream: each event is a small message
        ## containing the job id as a 4-byte big-endian uint
        event-body (apply concat
                          (map (fn [i]
                                 (grpc-frame (bytes (bit/shr i 8)
                                 (bit/and i 0xff) 0 0))) (range 0 num-jobs)))]
    (with-server (fn [req]
                   (cond
                     (= req:path "/submit") {:status 200 :body "submitted"}
                     (= req:path "/events")
                       {:status 200
                        :headers {:content-type "application/grpc"}
                        :body event-body
                        :trailers [["grpc-status" "0"]]}
                     (string/starts-with? req:path "/result/")
                       (let [id (slice req:path 8)]
                         {:status 200 :body (concat "result:" id)})
                     true {:status 200 :body "ok"}))
                 (fn [session _]  ## Step 1: bulk submit
                   (println "    submitting...")
                   (def submit-resp
                     (http2:send session "POST" "/submit" :body "200 jobs"))
                   (assert (= submit-resp:status 200) "bulk: submit status")

                   ## Step 2: stream read completions
                   (println "    reading event stream...")
                   (def s
                     (http2:send-raw session "POST" "/events" :body (bytes)
                                     :headers [["content-type"
                                     "application/grpc"] ["te" "trailers"]]))
                   (def @buf (bytes))
                   (def @completed @[])
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
                       (let [result (grpc-read-frame buf)]
                         (if (nil? result)
                           (break nil)
                           (begin
                             (let [payload (get result 0)
                                   id (bit/or (bit/shl (get payload 0) 8)
                                   (get payload 1))]
                               (push completed id))
                             (assign buf (get result 1)))))))
                   (assert (= (length completed) num-jobs)
                           (concat "bulk: expected " (string num-jobs)
                                   " completions, got "
                                   (string (length completed))))

                   ## Step 3: parallel fetch results for all completed IDs (32 at a time)
                   (println "    fetching " (string (length completed))
                            " results in parallel...")
                   (def @fetched @[])
                   (def @offset 0)
                   (while (< offset (length completed))
                     (def batch-end (min (+ offset 32) (length completed)))
                     (def batch-ids
                       (->list (slice (->array completed) offset batch-end)))
                     (def fibers
                       (map (fn [id]
                              (ev/spawn (fn []
                                          [id
                                          (http2:send session "GET"
                                          (concat "/result/" (string id)))])))
                            batch-ids))
                     (def results (map ev/join fibers))
                     (each r in results
                       (let [id (get r 0)
                             resp (get r 1)]
                         (assert (= resp:status 200)
                                 (concat "bulk: fetch id " (string id) " status"))
                         (push fetched id)))
                     (assign offset batch-end))
                   (assert (= (length fetched) num-jobs)
                           (concat "bulk: fetched " (string (length fetched))
                                   " of " (string num-jobs)))
                   (println "    all " (string (length fetched))
                            " results fetched")
                   (assert (= (length (keys session:streams)) 0)
                           "bulk: no stream leak")
                   true))))

## ── Test 9: Long-lived stream + concurrent unary RPCs ─────────────────────

(defn test-stream-plus-unary []
  "Open a server-streaming response (delayed 0.5s) on one h2 stream while
   making 20 unary requests on other streams. Both must succeed on the same
   session. This is the Subscribe + evolve/GetRun pattern."
  (let [stream-body (apply concat
                           (map (fn [i] (grpc-frame (bytes i 0 0 0)))
                                (range 0 5)))]
    (with-server (fn [req]
                   (cond
                     (= req:path "/slow-stream")
                       (begin
                         (ev/sleep 0.5)  ## simulate delayed event delivery
                         {:status 200
                          :headers {:content-type "application/grpc"}
                          :body stream-body
                          :trailers [["grpc-status" "0"]]})
                     (= req:path "/fixed") {:status 200 :body "ok"}
                     true {:status 200 :body "unknown"}))
                 (fn [session _]  ## Open streaming request (will take ~0.5s to get response)
                   (def stream-s
                     (http2:send-raw session "POST" "/slow-stream" :body (bytes)
                                     :headers [["content-type"
                                     "application/grpc"] ["te" "trailers"]]))
                   (println "    stream opened, making unary requests...")

                   ## Make 20 unary requests while stream is pending
                   (def @unary-ok 0)
                   (each i in (range 0 20)
                     (let [resp (http2:send session "GET"
                           (concat "/fixed?i=" (string i)))]
                       (assert (= resp:status 200)
                               (concat "stream+unary: request " (string i)
                                       " failed"))
                       (assign unary-ok (+ unary-ok 1))))
                   (println "    " unary-ok " unary requests completed")
                   (assert (= unary-ok 20)
                           "stream+unary: all 20 unary requests must complete")

                   ## Now read the stream response
                   (def @buf (bytes))
                   (def @received @[])
                   (def @done false)
                   (while (not done)
                     (let [msg (stream-s:data-queue:take)]
                       (match msg:type
                         :headers (when msg:end-stream (assign done true))
                         :data
                           (begin
                             (assign buf (concat buf msg:data))
                             (when msg:end-stream (assign done true)))
                         _ (assign done true)))
                     (while true
                       (let [result (grpc-read-frame buf)]
                         (if (nil? result)
                           (break nil)
                           (begin
                             (push received (get result 0))
                             (assign buf (get result 1)))))))
                   (assert (= (length received) 5)
                           (concat "stream+unary: expected 5 stream messages, got "
                                   (string (length received))))
                   (println "    stream delivered " (length received)
                            " messages")
                   true))))

## ── Test 10: Abandoned stream must not block session ──────────────────────

(defn test-abandoned-stream []
  "Open a streaming response, read a few messages, stop reading, then make
   more unary requests. The abandoned stream's data-queue must not block
   the h2 reader fiber. Mirrors cohort: Subscribe per layer, close after."
  (let [## 100 messages — more than data-queue capacity (64)
        big-body (apply concat
                        (map (fn [i] (grpc-frame (make-body 200))) (range 0 100)))]
    (with-server (fn [req]
                   (cond
                     (= req:path "/stream")
                       (begin
                         (ev/sleep 0.1)
                         {:status 200
                          :headers {:content-type "application/grpc"}
                          :body big-body
                          :trailers [["grpc-status" "0"]]})
                     (= req:path "/fixed") {:status 200 :body "ok"}
                     true {:status 200 :body "unknown"}))
                 (fn [session _]  ## Open stream and read only 3 messages
                   (def s1
                     (http2:send-raw session "POST" "/stream" :body (bytes)
                                     :headers [["content-type"
                                     "application/grpc"] ["te" "trailers"]]))
                   (println "    stream 1 opened, reading 3 messages...")
                   (def @msgs-read 0)
                   (def @s1-buf (bytes))
                   (each _ in (range 0 10)
                     (let [msg (s1:data-queue:take)]
                       (when (= msg:type :data)
                         (assign s1-buf (concat s1-buf msg:data))
                         (while true
                           (let [result (grpc-read-frame s1-buf)]
                             (if (nil? result)
                               (break nil)
                               (begin
                                 (assign msgs-read (+ msgs-read 1))
                                 (assign s1-buf (get result 1))
                                 (when (>= msgs-read 3) (break nil))))))
                         (when (>= msgs-read 3) (break nil)))))
                   (println "    read " msgs-read " messages, abandoning stream")

                   ## Now make unary requests — these must not block
                   (println "    making 10 unary requests on same session...")
                   (each i in (range 0 10)
                     (let [resp (http2:send session "GET"
                           (concat "/fixed?i=" (string i)))]
                       (assert (= resp:status 200)
                               (concat "abandoned: unary " (string i) " failed"))))
                   (println "    all 10 unary requests succeeded")
                   true))))

## ── Test 11: Cohort pipeline simulation (6 layers, growing body sizes) ────

(defn test-cohort-pipeline []
  "Simulate the cohort Phase 1 pipeline: 6 layers with growing request
   bodies (matching 5/20/60/120/180/250-day training windows × 10 jobs ×
   33 features). Each layer: one large POST (bulk-evolve), then 10
   parallel GETs (GetRun). All on one session, no reconnects."

  ## Each job ≈ window_days × 33 features × 8 bytes (protobuf overhead)
  ## × 10 jobs per layer
  (let [windows [5 20 60 120 180 250]
        make-layer-body (fn [window] (make-body (* window 33 8 10)))]
    (with-server (make-handler)
                 (fn [session _]
                   (each layer-idx in (range 0 6)
                     (def window (get windows layer-idx))
                     (def body-size (* window 33 8 10))
                     (def body (make-layer-body window))
                     (println "    L" layer-idx " window=" window "d body="
                              body-size "B")

                     ## Large POST (bulk-evolve simulation)
                     (def resp (http2:send session "POST" "/echo" :body body))
                     (assert (= resp:status 200)
                             (concat "cohort-L" (string layer-idx)
                                     ": POST failed"))
                     (assert (= (length resp:body) (length body))
                             (concat "cohort-L" (string layer-idx)
                                     ": body size mismatch"))

                     ## 10 parallel GETs (GetRun simulation)
                     (def fibers
                       (map (fn [i]
                              (ev/spawn (fn []
                                          (http2:send session "GET"
                                          (concat "/fixed?L" (string layer-idx)
                                          "&r=" (string i)))))) (range 0 10)))
                     (def results (map ev/join fibers))
                     (each r in results
                       (assert (= r:status 200)
                               (concat "cohort-L" (string layer-idx)
                                       ": GET failed"))))
                   (println "    all 6 layers complete")
                   (assert (= (length (keys session:streams)) 0)
                           "cohort-pipeline: no stream leak")
                   true))))

## ── Test 12: Cohort pipeline at real scale (650 jobs, real body sizes) ────

(defn test-cohort-pipeline-real-scale []
  "Reproduce the actual cohort Phase 1 payload sizes:
   650 jobs per layer, each job ≈ window × 33 features × 4 bytes (f32).
   Total request body per layer:
     L0 (5d):   650 × 5 × 33 × 4 ≈ 429KB
     L1 (20d):  650 × 20 × 33 × 4 ≈ 1.7MB
     L2 (60d):  650 × 60 × 33 × 4 ≈ 5.1MB
     L3 (120d): 650 × 120 × 33 × 4 ≈ 10.3MB
     L4 (180d): 650 × 180 × 33 × 4 ≈ 15.4MB
     L5 (250d): 650 × 250 × 33 × 4 ≈ 21.5MB
   Single POST per layer + Subscribe stream read. Hangs at L2+ in production."
  (let [windows [5 20 60 120 180 250]
        num-jobs 650
        num-features 33
        bytes-per-float 4  ## Server echoes back a small response (real server returns run_ids)
        make-layer-body (fn [window]
                          (make-body (* num-jobs window num-features
                                        bytes-per-float)))]
    (with-server (fn [req]
                   (cond
                     (= req:path "/submit") {:status 200 :body "submitted"}
                     (= req:path "/stream")
                       (let [## Simulate Subscribe: return num-jobs gRPC-framed result events
                             events (apply concat
                             (map (fn [i]
                                    (grpc-frame (bytes (bit/shr i 8)
                                    (bit/and i 0xff) 0 0))) (range 0 num-jobs)))]
                         {:status 200
                          :headers {:content-type "application/grpc"}
                          :body events
                          :trailers [["grpc-status" "0"]]})
                     true {:status 200 :body "ok"}))
                 (fn [session _]
                   (each layer-idx in (range 0 6)
                     (def window (get windows layer-idx))
                     (def body-size
                       (* num-jobs window num-features bytes-per-float))
                     (def body (make-layer-body window))
                     (println "    L" layer-idx " window=" window "d body="
                              (/ body-size 1024) "KB")

                     ## Large POST (bulk-evolve)
                     (def t0 (clock/monotonic))
                     (def resp (http2:send session "POST" "/submit" :body body))
                     (def t1 (clock/monotonic))
                     (assert (= resp:status 200)
                             (concat "real-L" (string layer-idx) ": POST failed"))
                     (println "      POST " (round4 (- t1 t0)) "s")

                     ## Subscribe stream read
                     (def s
                       (http2:send-raw session "POST" "/stream" :body (bytes)
                                       :headers [["content-type"
                                       "application/grpc"] ["te" "trailers"]]))
                     (def @buf (bytes))
                     (def @event-count 0)
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
                         (let [result (grpc-read-frame buf)]
                           (if (nil? result)
                             (break nil)
                             (begin
                               (assign event-count (+ event-count 1))
                               (assign buf (get result 1)))))))
                     (def t2 (clock/monotonic))
                     (assert (= event-count num-jobs)
                             (concat "real-L" (string layer-idx) ": expected "
                                     (string num-jobs) " events, got "
                                     (string event-count)))
                     (println "      stream " event-count " events "
                              (round4 (- t2 t1)) "s"))
                   (println "    all 6 layers complete")
                   (assert (= (length (keys session:streams)) 0)
                           "real-scale: no stream leak")
                   true))))

## ── Test 13: Many Subscribe streams then unary RPCs (Phase 1→3 pattern) ──

(defn test-many-streams-then-unary []
  "Open N server-streaming responses (Subscribe pattern), read them to
   completion, then make unary requests on the same session. Reproduces
   the Phase 1 → Phase 3 transition where 78 Subscribe streams precede
   evaluate RPCs. Checks for stream/session state corruption."
  (let [num-streams 78  ## 13 batches × 6 layers
        msgs-per-stream 50
        stream-body (apply concat
                           (map (fn [i]
                                  (grpc-frame (bytes (bit/shr i 8)
                                  (bit/and i 0xff) 0 0)))
                                (range 0 msgs-per-stream)))]
    (with-server (fn [req]
                   (cond
                     (= req:path "/stream")
                       {:status 200
                        :headers {:content-type "application/grpc"}
                        :body stream-body
                        :trailers [["grpc-status" "0"]]}
                     (= req:path "/unary") {:status 200 :body "ok"}
                     true {:status 200 :body "unknown"}))
                 (fn [session _]  ## Phase 1: open and drain N streams
                   (each si in (range 0 num-streams)
                     (def s
                       (http2:send-raw session "POST" "/stream" :body (bytes)
                                       :headers [["content-type"
                                       "application/grpc"] ["te" "trailers"]]))
                     (def @buf (bytes))
                     (def @count 0)
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
                         (let [result (grpc-read-frame buf)]
                           (if (nil? result)
                             (break nil)
                             (begin
                               (assign count (+ count 1))
                               (assign buf (get result 1)))))))
                     (assert (= count msgs-per-stream)
                             (concat "stream " (string si) ": expected "
                                     (string msgs-per-stream) " msgs, got "
                                     (string count)))
                     (when (= 0 (mod si 20))
                       (println "    " si "/" num-streams " streams drained")))
                   (println "    all " num-streams " streams drained")
                   (println "    active streams: "
                            (length (keys session:streams)))

                   ## Phase 3: unary RPCs
                   (println "    making 20 unary requests...")
                   (each i in (range 0 20)
                     (let [resp (http2:send session "GET"
                           (concat "/unary?i=" (string i)))]
                       (assert (= resp:status 200)
                               (concat "post-stream unary " (string i) " failed"))))
                   (println "    all 20 unary requests succeeded")
                   (assert (= (length (keys session:streams)) 0)
                           "many-streams: stream leak after unary")
                   true))))

## ── Test 14: Chunked bulk-submit + multi-subscribe per layer (cohort pattern) ─

(defn test-chunked-multi-subscribe []
  "Reproduce the exact cohort Phase 1 pattern: per layer, submit 13 chunks
   of 50 jobs each (each chunk = separate POST), then await 13 Subscribe
   streams (one per chunk). Repeat for 6 layers on the same session.
   This is the pattern that hangs at layer 1 in production."
  (let [num-layers 6
        chunks-per-layer 13
        jobs-per-chunk 50
        windows [5 20 60 120 180 250]
        num-features 33
        bytes-per-float 4  ## Each chunk's submit body ≈ jobs × window × features × 4 bytes
        make-chunk-body (fn [window]
                          (make-body (* jobs-per-chunk window num-features
                                        bytes-per-float)))  ## Each Subscribe stream returns jobs-per-chunk events
        make-stream-body (fn []
                           (apply concat
                                  (map (fn [i]
                                         (grpc-frame (bytes (bit/shr i 8)
                                         (bit/and i 0xff) 0 0)))
                                       (range 0 jobs-per-chunk))))]
    (with-server (fn [req]
                   (cond
                     (string/starts-with? req:path "/submit") {:status 200
                     :body "submitted"}
                     (string/starts-with? req:path "/stream")
                       {:status 200
                        :headers {:content-type "application/grpc"}
                        :body (make-stream-body)
                        :trailers [["grpc-status" "0"]]}
                     true {:status 200 :body "ok"}))
                 (fn [session _]
                   (each layer-idx in (range 0 num-layers)
                     (def window (get windows layer-idx))
                     (def chunk-body (make-chunk-body window))
                     (println "    L" layer-idx " (" chunks-per-layer
                              " chunks × " jobs-per-chunk " jobs, "
                              (/ (length chunk-body) 1024) "KB each)")

                     ## Phase A: submit all chunks
                     (def @chunk-idx 0)
                     (while (< chunk-idx chunks-per-layer)
                       (def resp
                         (http2:send session "POST"
                                     (concat "/submit/L" (string layer-idx) "/C"
                                     (string chunk-idx)) :body chunk-body))
                       (assert (= resp:status 200)
                               (concat "chunked-L" (string layer-idx) "-C"
                                       (string chunk-idx) ": submit failed"))
                       (assign chunk-idx (+ chunk-idx 1)))

                     ## Phase B: await all 13 Subscribe streams sequentially
                     (assign chunk-idx 0)
                     (while (< chunk-idx chunks-per-layer)
                       (def s
                         (http2:send-raw session "POST"
                         (concat "/stream/L" (string layer-idx) "/C"
                                 (string chunk-idx)) :body (bytes)
                         :headers [["content-type" "application/grpc"]
                                   ["te" "trailers"]]))
                       (def @buf (bytes))
                       (def @event-count 0)
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
                           (let [result (grpc-read-frame buf)]
                             (if (nil? result)
                               (break nil)
                               (begin
                                 (assign event-count (+ event-count 1))
                                 (assign buf (get result 1)))))))
                       (assert (= event-count jobs-per-chunk)
                               (concat "chunked-L" (string layer-idx) "-C"
                                       (string chunk-idx) ": expected "
                                       (string jobs-per-chunk) " events, got "
                                       (string event-count)))
                       (assign chunk-idx (+ chunk-idx 1)))
                     (println "      all " chunks-per-layer " streams read"))
                   (println "    all " num-layers " layers complete")
                   (assert (= (length (keys session:streams)) 0)
                           "chunked: no stream leak")
                   true))))

## ── Test 15: Bidi stream (client sends multiple, server echoes back) ─────

(defn test-bidi-stream []
  "Test h2-open-stream, h2-stream-send, and h2-stream-end primitives.
   Uses the standard Elle h2 server which collects the full request body.
   Client sends 100 gRPC-framed messages via stream-send, half-closes,
   server echoes them back in the response body. Verifies the bidi h2
   primitives work for the gRPC client-streaming path."
  (let [num-msgs 100
        msg-size 500]
    (with-server (fn [req]  ## Server receives all gRPC frames in req:body, echoes them back
                 ## with gRPC trailers (simulating a server that processes all
                   ## client messages then responds).
                   {:status 200
                    :headers {:content-type "application/grpc"}
                    :body (or req:body (bytes))
                    :trailers [["grpc-status" "0"]]})
                 (fn [session _]  ## Open bidi stream
                   (def [sid s]
                     (http2:open-stream session "POST" "/test.Svc/BidiMethod"
                                        :headers [["content-type"
                                        "application/grpc"] ["te" "trailers"]]))

                   ## Send num-msgs gRPC-framed messages
                   (each i in (range 0 num-msgs)
                     (let [payload (make-body msg-size)
                           framed (grpc-frame payload)]
                       (http2:stream-send session sid framed)))

                   ## Half-close
                   (http2:stream-end session sid)

                   ## Read responses
                   (def @buf (bytes))
                   (def @received @[])
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
                       (let [result (grpc-read-frame buf)]
                         (if (nil? result)
                           (break nil)
                           (begin
                             (push received (get result 0))
                             (assign buf (get result 1)))))))
                   (assert (= (length received) num-msgs)
                           (concat "bidi: expected " (string num-msgs)
                                   " messages, got " (string (length received))))
                   (each i in (range 0 num-msgs)
                     (assert (= (length (get received i)) msg-size)
                             (concat "bidi: message " (string i)
                                     " size mismatch")))
                   true))))

## ── Test 16: Bidi stream at scale (10,000 gRPC-framed messages) ──────────

(defn test-bidi-100k []
  "Send 10,000 gRPC-framed messages via stream-send on a single bidi
   stream. Server echoes all back. Verifies h2 flow control, stream
   state, and session stability at cohort-scale message counts."
  (let [num-msgs 10000
        msg-size 100]
    (with-server (fn [req]
                   {:status 200
                    :headers {:content-type "application/grpc"}
                    :body (or req:body (bytes))
                    :trailers [["grpc-status" "0"]]})
                 (fn [session _]
                   (def [sid s]
                     (http2:open-stream session "POST" "/test.Svc/Bidi100k"
                                        :headers [["content-type"
                                        "application/grpc"] ["te" "trailers"]]))

                   (println "    sending " num-msgs " messages...")
                   (def payload (make-body msg-size))
                   (def framed (grpc-frame payload))
                   (each i in (range 0 num-msgs)
                     (http2:stream-send session sid framed)
                     (when (= 0 (mod i 25000)) (println "      sent " i)))

                   (http2:stream-end session sid)
                   (println "    half-closed, reading responses...")

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
                       (let [result (grpc-read-frame buf)]
                         (if (nil? result)
                           (break nil)
                           (begin
                             (assign received (+ received 1))
                             (assign buf (get result 1)))))))
                   (println "    received " received " messages")
                   (assert (= received num-msgs)
                           (concat "bidi-100k: expected " (string num-msgs)
                                   " msgs, got " (string received)))
                   (assert (= (length (keys session:streams)) 0)
                           "bidi-100k: no stream leak")
                   true))))

## ── Test 18: Amplified bidi (small request, large response body) ─────────
##
## Simulates cohort pattern: client sends small messages, server echoes
## back a large body. Tests WU flow when response >> request.

(defn test-amplified-bidi []
  "Client sends 100 bytes, server echoes back 2MB (amplified).
   Exercises connection + stream WU across multiple window cycles.
   The initial connection window is ~1MB and threshold is 512KB,
   so receiving 2MB requires multiple WU round-trips."
  (let [response-size (* 2 1024 1024)]
    (with-server (fn [req]
                   {:status 200
                    :headers {:content-type "application/grpc"}
                    :body (make-body response-size)
                    :trailers [["grpc-status" "0"]]})
                 (fn [session _]
                   (def [sid s]
                     (http2:open-stream session "POST" "/test.Svc/Amplified"
                                        :headers [["content-type"
                                        "application/grpc"] ["te" "trailers"]]))
                   (http2:stream-send session sid (grpc-frame (bytes "hello")))
                   (http2:stream-end session sid)
                   (println "    half-closed, reading " response-size
                            " byte response...")
                   (def @total-bytes 0)
                   (def @done false)
                   (while (not done)
                     (let [msg (s:data-queue:take)]
                       (match msg:type
                         :headers (when msg:end-stream (assign done true))
                         :data
                           (begin
                             (assign
                               total-bytes
                               (+ total-bytes (length msg:data)))
                             (when msg:end-stream (assign done true)))
                         _ (assign done true))))
                   (println "    received " total-bytes " bytes")
                   (assert (> total-bytes (- response-size 1000))
                           (concat "amplified-bidi: expected ~"
                                   (string response-size) " bytes, got "
                                   (string total-bytes)))
                   true))))

## ── Test 17: RFC 9113 malformed frame rejection ─────────────────────────
##
## Raw client sends protocol-violating frames, verifies server responds
## with GOAWAY PROTOCOL_ERROR / FRAME_SIZE_ERROR as appropriate.

(defn raw-h2-handshake [t]
  "Perform client h2 handshake on raw transport. Returns nil."
  (t:write FC:client-preface)
  (let [[ft fl si pl] (h2-frame:make-settings-frame [[FC:settings-initial-window-size
        65535]])]
    (h2-frame:write-frame t ft fl si pl))
  (t:flush)  # Read server SETTINGS
  (h2-frame:read-frame t 16384)  # Read server SETTINGS ACK
  (h2-frame:read-frame t 16384)  # Possibly read server WINDOW_UPDATE for conn
  # Send our SETTINGS ACK
  (let [[ft fl si pl] (h2-frame:make-settings-ack)]
    (h2-frame:write-frame t ft fl si pl))
  (t:flush))

(defn read-goaway [t]
  "Read frames until GOAWAY, return its error code. Returns nil on EOF."
  (let [@result nil]
    (each _ in (range 0 20)
      (let [[ok? f] (protect (h2-frame:read-frame t 16384))]
        (when (or (not ok?) (nil? f)) (break nil))
        (when (= f:type FC:type-goaway)
          (assign result (h2-frame:read-u32 f:payload 4))
          (break nil))))
    result))

(defn test-rfc-compliance []
  (let [handler (fn [req] {:status 200 :body "ok"})
        [listener lport] (listen-ephemeral)
        base-url (concat "http://127.0.0.1:" (string lport))
        sf (ev/spawn (fn [] (protect (http2:serve listener handler))))]
    (defer
      (begin
        (protect (port/close listener))
        (protect (ev/abort sf)))

      # Sub-test 1: DATA on stream 0 → GOAWAY PROTOCOL_ERROR
      (let* [tcp (tcp/connect "127.0.0.1" lport)
             t (h2-transport:tcp tcp)]
        (raw-h2-handshake t)  # Send DATA on stream 0
        (let [[ft fl si pl] (h2-frame:make-data-frame 0 (bytes "bad") false)]
          (h2-frame:write-frame t ft fl si pl)
          (t:flush))
        (let [code (read-goaway t)]
          (assert (= code FC:err-protocol-error)
                  (concat "rfc: DATA on stream 0 → PROTOCOL_ERROR (got "
                          (string code) ")")))
        (protect (t:close)))

      # Sub-test 2: HEADERS on stream 0 → GOAWAY PROTOCOL_ERROR
      (let* [tcp (tcp/connect "127.0.0.1" lport)
             t (h2-transport:tcp tcp)]
        (raw-h2-handshake t)
        (let [[ft fl si pl] (h2-frame:make-headers-frame 0 (bytes 0x82) false
              true)]
          (h2-frame:write-frame t ft fl si pl)
          (t:flush))
        (let [code (read-goaway t)]
          (assert (= code FC:err-protocol-error)
                  (concat "rfc: HEADERS on stream 0 → PROTOCOL_ERROR (got "
                          (string code) ")")))
        (protect (t:close)))

      # Sub-test 3: RST_STREAM on stream 0 → GOAWAY PROTOCOL_ERROR
      (let* [tcp (tcp/connect "127.0.0.1" lport)
             t (h2-transport:tcp tcp)]
        (raw-h2-handshake t)
        (let [[ft fl si pl] (h2-frame:make-rst-stream-frame 0 FC:err-cancel)]
          (h2-frame:write-frame t ft fl si pl)
          (t:flush))
        (let [code (read-goaway t)]
          (assert (= code FC:err-protocol-error)
                  (concat "rfc: RST_STREAM on stream 0 → PROTOCOL_ERROR (got "
                          (string code) ")")))
        (protect (t:close)))

      # Sub-test 4: SETTINGS on non-zero stream → GOAWAY PROTOCOL_ERROR
      (let* [tcp (tcp/connect "127.0.0.1" lport)
             t (h2-transport:tcp tcp)]
        (raw-h2-handshake t)
        (let [payload (concat (h2-frame:u16->bytes FC:settings-initial-window-size)
                              (h2-frame:u32->bytes 65535))]
          (h2-frame:write-frame t FC:type-settings 0 1 payload)
          (t:flush))
        (let [code (read-goaway t)]
          (assert (= code FC:err-protocol-error)
                  (concat "rfc: SETTINGS on stream 1 → PROTOCOL_ERROR (got "
                          (string code) ")")))
        (protect (t:close)))

      # Sub-test 5: PING on non-zero stream → GOAWAY PROTOCOL_ERROR
      (let* [tcp (tcp/connect "127.0.0.1" lport)
             t (h2-transport:tcp tcp)]
        (raw-h2-handshake t)
        (h2-frame:write-frame t FC:type-ping 0 1 (bytes 1 2 3 4 5 6 7 8))
        (t:flush)
        (let [code (read-goaway t)]
          (assert (= code FC:err-protocol-error)
                  (concat "rfc: PING on stream 1 → PROTOCOL_ERROR (got "
                          (string code) ")")))
        (protect (t:close)))

      # Sub-test 6: PING with wrong payload size → GOAWAY FRAME_SIZE_ERROR
      (let* [tcp (tcp/connect "127.0.0.1" lport)
             t (h2-transport:tcp tcp)]
        (raw-h2-handshake t)
        (h2-frame:write-frame t FC:type-ping 0 0 (bytes 1 2 3 4))
        (t:flush)
        (let [code (read-goaway t)]
          (assert (= code FC:err-frame-size-error)
                  (concat "rfc: PING wrong size → FRAME_SIZE_ERROR (got "
                          (string code) ")")))
        (protect (t:close)))

      # Sub-test 7: CONTINUATION without prior HEADERS → GOAWAY PROTOCOL_ERROR
      (let* [tcp (tcp/connect "127.0.0.1" lport)
             t (h2-transport:tcp tcp)]
        (raw-h2-handshake t)
        (let [[ft fl si pl] (h2-frame:make-continuation-frame 1 (bytes 0x82)
              true)]
          (h2-frame:write-frame t ft fl si pl)
          (t:flush))
        (let [code (read-goaway t)]
          (assert (= code FC:err-protocol-error)
                  (concat "rfc: CONTINUATION without HEADERS → PROTOCOL_ERROR (got "
                          (string code) ")")))
        (protect (t:close)))

      # Sub-test 8: Non-CONTINUATION after HEADERS without END_HEADERS
      (let* [tcp (tcp/connect "127.0.0.1" lport)
             t (h2-transport:tcp tcp)]
        (raw-h2-handshake t)  # Send HEADERS without END_HEADERS on stream 1
        (let [[ft fl si pl] (h2-frame:make-headers-frame 1 (bytes 0x82) false
              false)]
          (h2-frame:write-frame t ft fl si pl))  # Send DATA instead of CONTINUATION — violation
        (let [[ft fl si pl] (h2-frame:make-data-frame 1 (bytes "bad") false)]
          (h2-frame:write-frame t ft fl si pl))
        (t:flush)
        (let [code (read-goaway t)]
          (assert (= code FC:err-protocol-error)
                  (concat "rfc: non-CONTINUATION interleave → PROTOCOL_ERROR (got "
                          (string code) ")")))
        (protect (t:close)))

      true)))

## ── Test 19: Many sequential bidi streams on one session (roster pattern) ────
##
## Reproduces the roster evolution phase: each "day" opens a bidi stream,
## sends a few gRPC-framed messages, reads responses, closes the stream.
## Repeat N times on one session without reconnecting.
## The production hang occurs at ~250 bidi stream cycles.

(defn test-sequential-bidi-streams []
  "Open 300 sequential bidi streams on one session. Each stream sends 5
   gRPC-framed messages, reads 5 back. Tests cumulative stream ID,
   flow control, and session state after hundreds of bidi cycles."
  (let [num-streams 300
        msgs-per-stream 5
        msg-size 200
        payload (make-body msg-size)
        framed (grpc-frame payload)]
    (with-server (fn [req]
                   {:status 200
                    :headers {:content-type "application/grpc"}
                    :body (or req:body (bytes))
                    :trailers [["grpc-status" "0"]]})
                 (fn [session _]
                   (each si in (range 0 num-streams)
                     (when (= 0 (mod si 50))
                       (println "    stream " si "/" num-streams
                                " (stream-ids used: " (* si 2) ")"))
                     (def [sid s]
                       (http2:open-stream session "POST"
                       (concat "/test.Svc/Evolve" (string si))
                       :headers [["content-type" "application/grpc"]
                                 ["te" "trailers"]]))  # Send msgs-per-stream gRPC-framed messages
                     (each _ in (range 0 msgs-per-stream)
                       (http2:stream-send session sid framed))
                     (http2:stream-end session sid)

                     # Read responses
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
                         (let [result (grpc-read-frame buf)]
                           (if (nil? result)
                             (break nil)
                             (begin
                               (assign received (+ received 1))
                               (assign buf (get result 1)))))))
                     (assert (= received msgs-per-stream)
                             (concat "seq-bidi: stream " (string si)
                                     " expected " (string msgs-per-stream)
                                     " msgs, got " (string received))))
                   (println "    all " num-streams " bidi streams completed")
                   (assert (= (length (keys session:streams)) 0)
                           "seq-bidi: stream leak after 300 bidi cycles")
                   true))))

## ── Test 20: Many bidi streams then unary RPCs (Phase 1→3 pattern) ──────
##
## The exact pattern that hangs in production: 252 bidi streams (evolution
## phase) followed by unary RPCs (evaluate phase) on the same session.

(defn test-bidi-then-unary []
  "Open 252 sequential bidi streams (Phase 1 evolution), then make 50
   unary requests (Phase 3 evaluate) on the same session without
   reconnecting. This is the exact pattern that triggers the hang."
  (let [num-bidi 252
        num-unary 50
        msgs-per-stream 3
        msg-size 100
        payload (make-body msg-size)
        framed (grpc-frame payload)]
    (with-server (fn [req]
                   (cond
                     (string/starts-with? req:path "/test.Svc/")
                       {:status 200
                        :headers {:content-type "application/grpc"}
                        :body (or req:body (bytes))
                        :trailers [["grpc-status" "0"]]}
                     true {:status 200 :body "ok"}))
                 (fn [session _]  # Phase 1: sequential bidi streams
                   (println "    Phase 1: " num-bidi " bidi streams...")
                   (each si in (range 0 num-bidi)
                     (when (= 0 (mod si 50))
                       (println "      bidi " si "/" num-bidi))
                     (def [sid s]
                       (http2:open-stream session "POST"
                       (concat "/test.Svc/BulkEvolveStream")
                       :headers [["content-type" "application/grpc"]
                                 ["te" "trailers"]]))
                     (each _ in (range 0 msgs-per-stream)
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
                         (let [result (grpc-read-frame buf)]
                           (if (nil? result)
                             (break nil)
                             (begin
                               (assign received (+ received 1))
                               (assign buf (get result 1)))))))
                     (assert (= received msgs-per-stream)
                             (concat "bidi→unary: bidi " (string si) " got "
                                     (string received) " msgs")))

                   (println "    Phase 1 done. active streams: "
                            (length (keys session:streams)))

                   # Phase 3: unary RPCs on the same session
                   (println "    Phase 3: " num-unary " unary requests...")
                   (each i in (range 0 num-unary)
                     (let [resp (http2:send session "POST"
                           (concat "/evaluate?i=" (string i))
                           :body (make-body 500))]
                       (assert (= resp:status 200)
                               (concat "bidi→unary: unary " (string i)
                                       " failed"))))
                   (println "    Phase 3 done. All " num-unary
                            " unary requests succeeded")
                   (assert (= (length (keys session:streams)) 0)
                           "bidi→unary: stream leak")
                   true))))

## ── Test 21: Bidi streams with periodic connection recycling ────────────
##
## Tests the proposed fix: close and reconnect every K bidi streams.
## After recycling, subsequent bidi and unary calls must succeed.

(defn test-bidi-with-recycling []
  "Open 300 bidi streams, recycling (close+reconnect) every 50 streams.
   After all bidi streams, make 20 unary requests. Tests that recycling
   keeps the session healthy and that post-recycle calls work."
  (let [num-streams 300
        recycle-every 50
        msgs-per-stream 3
        msg-size 100
        payload (make-body msg-size)
        framed (grpc-frame payload)]
    (let* [[listener lport] (listen-ephemeral)
           handler (fn [req]
                     (cond
                       (string/starts-with? req:path "/test.Svc/")
                         {:status 200
                          :headers {:content-type "application/grpc"}
                          :body (or req:body (bytes))
                          :trailers [["grpc-status" "0"]]}
                       true {:status 200 :body "ok"}))
           sf (ev/spawn (fn [] (protect (http2:serve listener handler))))
           url (concat "http://127.0.0.1:" (string lport))
           @session (http2:connect url)
           @recycle-count 0]
      (defer
        (begin
          (protect (http2:close session))
          (protect (port/close listener))
          (protect (ev/abort sf)))

        (each si in (range 0 num-streams)  # Recycle connection every recycle-every streams
          (when (and (> si 0) (= 0 (mod si recycle-every)))
            (protect (http2:close session))
            (assign session (http2:connect url))
            (assign recycle-count (+ recycle-count 1))
            (println "    recycled at stream " si " (recycle #" recycle-count
                     ")"))

          (def [sid s]
            (http2:open-stream session "POST" "/test.Svc/BulkEvolveStream"
                               :headers [["content-type" "application/grpc"]
                               ["te" "trailers"]]))
          (each _ in (range 0 msgs-per-stream)
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
              (let [result (grpc-read-frame buf)]
                (if (nil? result)
                  (break nil)
                  (begin
                    (assign received (+ received 1))
                    (assign buf (get result 1)))))))
          (assert (= received msgs-per-stream)
                  (concat "recycle-bidi: stream " (string si) " got "
                          (string received) " msgs")))

        (println "    all " num-streams " bidi streams done (" recycle-count
                 " recycles)")

        # Post-bidi unary requests
        (println "    making 20 unary requests after final recycle...")
        (protect (http2:close session))
        (assign session (http2:connect url))
        (each i in (range 0 20)
          (let [resp (http2:send session "POST"
                                 (concat "/evaluate?i=" (string i))
                                 :body (make-body 500))]
            (assert (= resp:status 200)
                    (concat "recycle-bidi: unary " (string i) " failed"))))
        (println "    all 20 unary requests succeeded")
        true))))

## ── Race test helpers (ev/timeout + bidi + recycle) ──────────────────────

(defn do-bidi [session n-msgs msg-size]
  "Open a bidi stream, send n-msgs gRPC-framed messages, read them all back."
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
        (let [result (grpc-read-frame buf)]
          (if (nil? result)
            (break nil)
            (begin
              (assign received (+ received 1))
              (assign buf (get result 1)))))))
    (assert (= received n-msgs)
            (concat "bidi: expected " (string n-msgs) " got " (string received)))
    received))

(defn echo-handler [req]
  {:status 200
   :headers {:content-type "application/grpc"}
   :body (or req:body (bytes))
   :trailers [["grpc-status" "0"]]})

(defn delayed-echo-handler [delay-ms]
  (fn [req]
    (ev/sleep (/ delay-ms 1000.0))
    {:status 200
     :headers {:content-type "application/grpc"}
     :body (or req:body (bytes))
     :trailers [["grpc-status" "0"]]}))

## ── Test 22: ev/timeout + bidi + recycle (basic race) ────────────────────

(defn test-timeout-bidi-recycle []
  (let* [[listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         sf (ev/spawn (fn [] (protect (http2:serve listener echo-handler))))
         @session (http2:connect url)]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each i in (range 0 5)
        (let [result (ev/timeout 30 (fn [] (do-bidi session 5 500)))]
          (assert (not (nil? result))
                  (concat "timeout-recycle: bidi " (string i) " timed out"))))
      (http2:close session)
      (assign session (http2:connect url))
      (each i in (range 5 10)
        (let [result (ev/timeout 30 (fn [] (do-bidi session 5 500)))]
          (assert (not (nil? result))
                  (concat "timeout-recycle: bidi " (string i) " timed out"))))
      (let [resp (http2:send session "GET" "/health")]
        (assert (= resp:status 200) "timeout-recycle: unary failed"))
      true)))

## ── Test 23: ev/timeout + bidi + recycle (large payloads) ────────────────

(defn test-timeout-bidi-recycle-large []
  (let* [[listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         sf (ev/spawn (fn [] (protect (http2:serve listener echo-handler))))
         @session (http2:connect url)]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each i in (range 0 10)
        (let [result (ev/timeout 30 (fn [] (do-bidi session 20 2000)))]
          (assert (not (nil? result))
                  (concat "timeout-large: bidi " (string i) " timed out"))))
      (http2:close session)
      (assign session (http2:connect url))
      (each i in (range 10 20)
        (let [result (ev/timeout 30 (fn [] (do-bidi session 20 2000)))]
          (assert (not (nil? result))
                  (concat "timeout-large: bidi " (string i) " timed out"))))
      (let [resp (http2:send session "GET" "/health")]
        (assert (= resp:status 200) "timeout-large: unary failed"))
      true)))

## ── Test 24: ev/timeout + bidi + immediate recycle (tight race) ──────────

(defn test-timeout-bidi-immediate-recycle []
  (let* [[listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         sf (ev/spawn (fn [] (protect (http2:serve listener echo-handler))))
         @session (http2:connect url)]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each cycle in (range 0 10)
        (each _ in (range 0 2)
          (let [result (ev/timeout 30 (fn [] (do-bidi session 10 1000)))]
            (assert (not (nil? result))
                    (concat "immediate-recycle: timed out at cycle "
                            (string cycle)))))
        (http2:close session)
        (assign session (http2:connect url)))
      (let [resp (http2:send session "GET" "/health")]
        (assert (= resp:status 200) "immediate-recycle: unary failed"))
      true)))

## ── Test 25: ev/timeout + bidi + delayed server (50ms compute) ───────────

(defn test-timeout-bidi-delayed-server []
  (let* [[listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         handler (delayed-echo-handler 50)
         sf (ev/spawn (fn [] (protect (http2:serve listener handler))))
         @session (http2:connect url)]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each i in (range 0 5)
        (let [result (ev/timeout 30 (fn [] (do-bidi session 5 500)))]
          (assert (not (nil? result))
                  (concat "delayed-server: bidi " (string i) " timed out"))))
      (http2:close session)
      (assign session (http2:connect url))
      (each i in (range 5 10)
        (let [result (ev/timeout 30 (fn [] (do-bidi session 5 500)))]
          (assert (not (nil? result))
                  (concat "delayed-server: bidi " (string i) " timed out"))))
      (let [resp (http2:send session "GET" "/health")]
        (assert (= resp:status 200) "delayed-server: unary failed"))
      true)))

## ── Test 26: ev/timeout + bidi + 20 recycle cycles (stress) ──────────────

(defn test-timeout-bidi-many-recycles []
  (let* [[listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         sf (ev/spawn (fn [] (protect (http2:serve listener echo-handler))))
         @session (http2:connect url)]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each cycle in (range 0 20)
        (each _ in (range 0 3)
          (ev/timeout 30 (fn [] (do-bidi session 5 500))))
        (http2:close session)
        (assign session (http2:connect url)))
      (let [resp (http2:send session "GET" "/health")]
        (assert (= resp:status 200) "many-recycles: unary after 20 recycles"))
      true)))

## ── Test 27: ev/timeout near-miss + recycle ──────────────────────────────

(defn test-timeout-near-miss []
  (let* [[listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         handler (delayed-echo-handler 50)
         sf (ev/spawn (fn [] (protect (http2:serve listener handler))))
         @session (http2:connect url)]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each cycle in (range 0 5)
        (each _ in (range 0 3)
          (let [result (ev/timeout 2 (fn [] (do-bidi session 3 500)))]
            (assert (not (nil? result))
                    (concat "near-miss: timed out at cycle " (string cycle)))))
        (http2:close session)
        (assign session (http2:connect url)))
      (let [resp (http2:send session "GET" "/health")]
        (assert (= resp:status 200) "near-miss: unary failed"))
      true)))

## ── Test 28: concurrent ev/timeout bidi + recycle ────────────────────────

(defn test-concurrent-timeout-bidi-recycle []
  (let* [[listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         sf (ev/spawn (fn [] (protect (http2:serve listener echo-handler))))
         @session (http2:connect url)]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each cycle in (range 0 5)
        (let [fibers (map (fn [i]
                            (ev/spawn (fn []
                                        (ev/timeout 30
                                        (fn [] (do-bidi session 5 500))))))
                          (range 0 4))
              results (map ev/join fibers)]
          (each r in results
            (assert (not (nil? r))
                    (concat "concurrent-timeout: timed out, cycle "
                            (string cycle)))))
        (http2:close session)
        (assign session (http2:connect url)))
      (let [resp (http2:send session "GET" "/health")]
        (assert (= resp:status 200) "concurrent-timeout: unary failed"))
      true)))

## ── Test 29: ev/timeout bidi then ev/timeout unary after recycle ─────────

(defn test-timeout-bidi-then-timeout-unary []
  (let* [[listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         sf (ev/spawn (fn [] (protect (http2:serve listener echo-handler))))
         @session (http2:connect url)]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each i in (range 0 10)
        (let [result (ev/timeout 30 (fn [] (do-bidi session 10 1000)))]
          (assert (not (nil? result))
                  (concat "bidi-then-unary: bidi " (string i) " timed out"))))
      (http2:close session)
      (assign session (http2:connect url))
      (each i in (range 10 20)
        (let [result (ev/timeout 30 (fn [] (do-bidi session 10 1000)))]
          (assert (not (nil? result))
                  (concat "bidi-then-unary: bidi " (string i) " timed out"))))
      (each i in (range 0 10)
        (let [result (ev/timeout 10
                                 (fn []
                                   (http2:send session "GET"
                                   (concat "/health?i=" (string i)))))]
          (assert (not (nil? result))
                  (concat "bidi-then-unary: unary " (string i) " timed out"))))
      true)))

## ── Test 30: ev/timeout bidi + streaming server + recycle ────────────────

(defn test-timeout-bidi-streaming-server []
  (let* [[listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         handler (fn [req ctrl]
                   (def @body-buf (bytes))
                   (forever
                     (let [data (ctrl:recv)]
                       (when (nil? data) (break nil))
                       (assign body-buf (concat body-buf data))))
                   (ctrl:send-headers 200
                                      :headers {:content-type "application/grpc"})
                   (def @pos 0)
                   (while (>= (- (length body-buf) pos) 5)
                     (let [len (bit/or (bit/shl (get body-buf (+ pos 1)) 24)
                                       (bit/shl (get body-buf (+ pos 2)) 16)
                                       (bit/shl (get body-buf (+ pos 3)) 8)
                                       (get body-buf (+ pos 4)))
                           frame-end (+ pos 5 len)]
                       (when (> frame-end (length body-buf)) (break nil))
                       (ctrl:send-data (slice body-buf pos frame-end))
                       (assign pos frame-end)))
                   (ctrl:send-trailers [["grpc-status" "0"]]))
         sf (ev/spawn (fn [] (protect (http2:serve-streaming listener handler))))
         @session (http2:connect url)]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each i in (range 0 10)
        (let [result (ev/timeout 30 (fn [] (do-bidi session 10 1000)))]
          (assert (not (nil? result))
                  (concat "streaming-server: bidi " (string i) " timed out"))))
      (http2:close session)
      (assign session (http2:connect url))
      (each i in (range 10 20)
        (let [result (ev/timeout 30 (fn [] (do-bidi session 10 1000)))]
          (assert (not (nil? result))
                  (concat "streaming-server: bidi " (string i) " timed out"))))
      (let [result (ev/timeout 10 (fn [] (http2:send session "GET" "/health")))]
        (assert (not (nil? result)) "streaming-server: unary timed out"))
      true)))

## ── Test 31: ev/timeout + queue race (no h2, pure scheduler) ─────────────

(defn test-timeout-queue-race []
  (let [@q (sync:make-queue 8)]
    (each i in (range 0 100)
      (ev/timeout 5
                  (fn []
                    (q:put (string i))
                    (let [val (q:take)]
                      (assert (= val (string i))
                              (concat "queue-race: mismatch at " (string i)))))))
    (q:put "final")
    (let [val (q:take)]
      (assert (= val "final")
              "queue-race: corrupted after 100 ev/timeout cycles"))
    true))

## ── Test 32: ev/timeout + producer recycle (simulated session swap) ──────

(defn test-timeout-producer-recycle []
  (let [@q (sync:make-queue 16)]
    (def @producer
      (ev/spawn (fn []
                  (def @n 0)
                  (forever
                    (q:put (string n))
                    (assign n (+ n 1))
                    (ev/sleep 0.001)))))
    (each _ in (range 0 50)
      (let [result (ev/timeout 5 (fn [] (q:take)))]
        (assert (not (nil? result)) "producer-recycle: P1 take timed out")))
    (ev/abort producer)
    (while true
      (let [[ok? _] (protect (ev/timeout 0.01 (fn [] (q:take))))]
        (when (or (not ok?) true) (break nil))))
    (assign q (sync:make-queue 16))
    (assign
      producer
      (ev/spawn (fn []
                  (def @n 1000)
                  (forever
                    (q:put (string n))
                    (assign n (+ n 1))
                    (ev/sleep 0.001)))))
    (each _ in (range 0 50)
      (let [result (ev/timeout 5 (fn [] (q:take)))]
        (assert (not (nil? result)) "producer-recycle: P2 take timed out")))
    (ev/abort producer)
    true))

## ── Run ──────────────────────────────────────────────────────────────────

(println "h2 stress tests (grace usage patterns):")
(println)
(run-test "200 sequential streams with 50KB bodies" test-sequential-large-bodies)
(run-test "32-concurrent requests × 10 rounds" test-high-concurrency)
(run-test "20 close+reconnect cycles × 50 requests" test-reconnect-cycles)
(run-test "concurrent polling loop (await-runs pattern)" test-concurrent-polling)
(run-test "large request + concurrent poll interleave"
          test-large-then-concurrent)
(run-test "500 sequential requests, no reconnect (10KB each)"
          test-session-durability)
(run-test "stream reader (Subscribe mock, 100 gRPC frames)" test-stream-reader)
(run-test "bulk submit + stream await + parallel fetch (Phase 1 pattern)"
          test-bulk-submit-stream-fetch)
(run-test "long-lived stream + concurrent unary RPCs (Subscribe pattern)"
          test-stream-plus-unary)
(run-test "abandoned stream must not block session" test-abandoned-stream)
(run-test "cohort pipeline (6 layers, growing bodies, 10 parallel GETs)"
          test-cohort-pipeline)
(run-test "cohort pipeline REAL SCALE (650 jobs, 430KB-21MB per layer)"
          test-cohort-pipeline-real-scale)
(run-test "78 Subscribe streams then 20 unary RPCs (Phase 1→3 pattern)"
          test-many-streams-then-unary)
(run-test "13 chunks × 6 layers: chunked submit + multi-subscribe (cohort pattern)"
          test-chunked-multi-subscribe)
(run-test "bidi stream: 100 gRPC-framed messages, echo back (open-stream/stream-send/stream-end)"
          test-bidi-stream)
(run-test "bidi stream: 10,000 gRPC-framed messages (cohort scale)"
          test-bidi-100k)
(run-test "amplified bidi: 100B request → 2MB response (WU round-trips)"
          test-amplified-bidi)
(run-test "RFC 9113 malformed frame rejection (8 sub-tests)" test-rfc-compliance)
(run-test "300 sequential bidi streams on one session (roster evolution pattern)"
          test-sequential-bidi-streams)
(run-test "252 bidi streams then 50 unary RPCs (Phase 1→3 hang pattern)"
          test-bidi-then-unary)
(run-test "300 bidi streams with recycle every 50 + post-recycle unary (proposed fix)"
          test-bidi-with-recycling)
(run-test "ev/timeout + bidi + recycle (basic race)" test-timeout-bidi-recycle)
(run-test "ev/timeout + bidi + recycle (large payloads, flow control)"
          test-timeout-bidi-recycle-large)
(run-test "ev/timeout + bidi + immediate recycle (tight race)"
          test-timeout-bidi-immediate-recycle)
(run-test "ev/timeout + bidi + delayed server (50ms compute sim)"
          test-timeout-bidi-delayed-server)
(run-test "ev/timeout + bidi + 20 recycle cycles (stress)"
          test-timeout-bidi-many-recycles)
(run-test "ev/timeout near-miss + recycle (scheduling pressure)"
          test-timeout-near-miss)
(run-test "concurrent ev/timeout bidi + recycle"
          test-concurrent-timeout-bidi-recycle)
(run-test "ev/timeout bidi then ev/timeout unary after recycle"
          test-timeout-bidi-then-timeout-unary)
(run-test "ev/timeout bidi + streaming server + recycle"
          test-timeout-bidi-streaming-server)
(run-test "ev/timeout + queue race (no h2, pure scheduler)"
          test-timeout-queue-race)
(run-test "ev/timeout + producer recycle (simulated session swap)"
          test-timeout-producer-recycle)
(println)
(println "results: " pass-count "/" test-count " passed, " fail-count " failed")
(when (> fail-count 0) (println "failures: " (freeze failures)))
(assert (= fail-count 0) "all h2 stress tests must pass")
(sys/exit 0)
